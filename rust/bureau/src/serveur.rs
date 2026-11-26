//! Le serveur : options, routes, et la session (un WebSocket : vidéo vers l'iPad, entrées vers le
//! Mac). Modèle : web_royaumes, avec la sécurité recopiée de web_tools_commun (src/commun/).

use std::io::{self, Read, Write};
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};

use crate::commun::appairage::{self, Garde};
use crate::commun::production;
use crate::commun::serveur::{decoder_chemin, Duplex, Reponse, Requete, Serveur};
use crate::fichiers;
use crate::macos::capture::Capture;
use crate::macos::ecran_virtuel::EcranVirtuel;
use crate::macos::encodage::{Encodeur, ImageEncodee};
use crate::macos::entrees::Entrees;
use crate::macos::{self, barre_menus, presse_papiers, CMTime};
use crate::maj;
use crate::protocole::{self, Codec, Entree, Qualite};
use crate::regulation::Regulation;
use crate::video;
use crate::ws::{self, Trames};

pub const PORT: u16 = 8803;
pub const DEPOT: &str = "https://github.com/pilou33620/web_bureau";
pub const ENTETE: &str = "X-WEB_BUREAU"; // en-tête maison des écritures (CSRF)
const CORPS_MAX: usize = 4096;
/// Sans geste pendant ce délai, la session se termine (et le Mac se verrouille si le déverrouillage
/// depuis l'iPad a été vérifié).
pub const INACTIVITE: Duration = Duration::from_secs(15 * 60);
const ECRAN_DEFAUT: (u32, u32) = (2266, 1488); // iPad mini A17 Pro, en paysage

const USAGE: &str = "usage : web_bureau [--port N] [--local] [--sans-maj] [--sans-ecran-virtuel] [--nouveau-jeton]
                  [--sans-navigateur] [--sans-pause]
  --port N              port d'écoute (défaut 8803)
  --local               n'écouter que sur 127.0.0.1 (essais)
  --sans-maj            ne pas chercher de mise à jour au démarrage
  --sans-ecran-virtuel  capturer l'écran principal au lieu de créer un écran virtuel
  --nouveau-jeton       retirer l'accès à tous les appareils appairés (nouveau code à saisir)";

#[derive(Debug, PartialEq)]
pub struct Args {
    pub port: u16,
    pub local: bool,
    pub sans_maj: bool,
    pub sans_ecran_virtuel: bool,
    pub nouveau_jeton: bool,
}

pub fn lire_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args { port: PORT, local: false, sans_maj: false, sans_ecran_virtuel: false, nouveau_jeton: false };
    let mut i = 0;
    while i < argv.len() {
        let x = argv[i].as_str();
        match x {
            "--local" => a.local = true,
            "--sans-maj" => a.sans_maj = true,
            "--sans-ecran-virtuel" => a.sans_ecran_virtuel = true,
            "--nouveau-jeton" => a.nouveau_jeton = true,
            "--sans-navigateur" | "--sans-pause" => {} // demandées par les règles communes ; rien à ouvrir ni à attendre ici
            "--port" => {
                i += 1;
                a.port = argv.get(i).and_then(|p| p.parse().ok()).ok_or("--port attend un nombre")?;
            }
            _ if x.starts_with("--port=") => a.port = x[7..].parse().map_err(|_| "--port attend un nombre")?,
            _ => return Err(format!("option inconnue : {x}")),
        }
        i += 1;
    }
    Ok(a)
}

// -- état ------------------------------------------------------------------------

/// Réglages gardés d'un démarrage à l'autre (~/.web_tools/web_bureau/reglages.json).
#[derive(Clone, Debug, PartialEq)]
pub struct Reglages {
    pub deverrouillage_verifie: bool,
    pub ecran: (u32, u32),
}

fn fichier_reglages() -> PathBuf {
    appairage::fichier_jeton().with_file_name("reglages.json")
}

pub fn lire_reglages(chemin: &Path) -> Reglages {
    let defaut = Reglages { deverrouillage_verifie: false, ecran: ECRAN_DEFAUT };
    let Ok(texte) = std::fs::read_to_string(chemin) else { return defaut };
    let v: Value = match serde_json::from_str(&texte) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            // jamais « illisible donc absent » suivi d'un écrasement (règles §3.2)
            let abime = chemin.with_file_name("reglages.abime.json");
            let _ = std::fs::rename(chemin, &abime);
            println!("[!] {} illisible : mis de côté sous {}", chemin.display(), abime.display());
            return defaut;
        }
    };
    let dim = |i: usize| v["ecran"][i].as_u64().filter(|n| (protocole::ECRAN_MIN as u64..=protocole::ECRAN_MAX as u64).contains(n));
    Reglages {
        deverrouillage_verifie: v["deverrouillage_verifie"].as_bool().unwrap_or(false),
        ecran: match (dim(0), dim(1)) {
            (Some(l), Some(h)) => (l as u32, h as u32),
            _ => ECRAN_DEFAUT,
        },
    }
}

pub fn ecrire_reglages(chemin: &Path, r: &Reglages) -> io::Result<()> {
    if let Some(d) = chemin.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = chemin.with_extension("json.tmp");
    let texte = json!({"deverrouillage_verifie": r.deverrouillage_verifie, "ecran": [r.ecran.0, r.ecran.1]});
    std::fs::write(&tmp, format!("{:#}\n", texte))?;
    std::fs::rename(&tmp, chemin)
}

struct Session {
    ip: IpAddr,
    /// Raison de fin demandée de l'extérieur : « reprise » (un autre appareil), « coupee_sur_mac ».
    fin: Mutex<Option<&'static str>>,
}

struct Etat {
    ecran: Mutex<Option<EcranVirtuel>>,
    entrees: Mutex<Entrees>,
    session: Mutex<Option<Arc<Session>>>,
    /// Tenu par la session qui capture : la suivante attend que la précédente ait tout arrêté.
    capture: Mutex<()>,
    reglages: Mutex<Reglages>,
}

static ETAT: OnceLock<Etat> = OnceLock::new();
static GARDE: OnceLock<Garde> = OnceLock::new();

fn etat() -> &'static Etat {
    ETAT.get().expect("état non initialisé")
}

fn verrou<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn racine() -> PathBuf {
    let exe = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()).unwrap_or_default();
    exe.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

fn dossier_personnel() -> PathBuf {
    appairage::dossier_personnel()
}

// -- fichiers de la page ---------------------------------------------------------------

/// Liste blanche : une extension absente n'est pas servie (404).
fn type_mime(p: &Path) -> Option<&'static str> {
    Some(match p.extension().and_then(|x| x.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        _ => return None,
    })
}

/// Un fichier de web/ : chaque segment ordinaire et sans point en tête, extension connue ; sinon 404.
fn fichier_web(web: &Path, chemin: &str) -> Reponse {
    let introuvable = || Reponse::texte(404, "introuvable");
    let rel = decoder_chemin(if chemin == "/" { "index.html" } else { chemin.trim_start_matches('/') });
    let p = Path::new(&rel);
    let sur = p.components().all(|c| matches!(c, Component::Normal(s) if !s.to_string_lossy().starts_with('.')));
    let Some(mime) = type_mime(p) else { return introuvable() };
    if !sur {
        return introuvable();
    }
    match std::fs::read(web.join(p)) {
        Ok(o) => Reponse::new(200, mime, o).entete("Cache-Control", "no-store, max-age=0"),
        Err(_) => introuvable(),
    }
}

fn detail(code: u16, texte: &str) -> Reponse {
    Reponse::json(code, &json!({"detail": texte}))
}

/// Le nom d'un fichier dans Content-Disposition (RFC 5987).
fn nom_telechargement(nom: &str) -> String {
    let code: String = nom
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) })
        .collect();
    format!("attachment; filename*=UTF-8''{code}")
}

// -- routes ----------------------------------------------------------------------

fn servir(r: &mut Requete) -> Reponse {
    let rep = router(r);
    // sur toutes les réponses (règles §2.4) ; X-Frame-Options est déjà posé par la caisse
    rep.entete("Content-Security-Policy", "frame-ancestors 'self'").entete("X-Content-Type-Options", "nosniff")
}

fn router(r: &mut Requete) -> Reponse {
    if let Some(rep) = GARDE.get().expect("garde").filtrer(r) {
        return rep;
    }
    let chemin = r.chemin().to_string();
    if !r.lecture() && r.entete(ENTETE) != Some("1") {
        return detail(403, "Requête refusée");
    }
    let resultat = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match (r.methode.as_str(), chemin.as_str()) {
        ("GET", "/ws") => {
            let ip = r.client.ip();
            r.websocket(&[], move |c| session(c, ip))
        }
        ("GET", "/api/etat") => api_etat(),
        ("GET", "/api/touches") => Reponse::json(200, &json!(crate::clavier::codes())),
        ("GET", "/api/diagnostic") => api_diagnostic(),
        ("POST", "/api/diagnostic/deverrouillage") => api_essai_deverrouillage(r),
        ("GET", "/api/fichiers") => api_lister(r),
        ("GET", "/api/fichiers/telecharger") => api_telecharger(r),
        ("POST", "/api/fichiers/envoyer") => api_envoyer(r),
        ("GET" | "HEAD", c) if !c.starts_with("/api/") => fichier_web(&racine().join("web"), c),
        _ => detail(404, "introuvable"),
    }));
    resultat.unwrap_or_else(|_| detail(500, "erreur interne"))
}

fn api_etat() -> Reponse {
    let e = etat();
    let ecran = verrou(&e.ecran).as_ref().map(|v| json!([v.largeur, v.hauteur]));
    Reponse::json(
        200,
        &json!({
            "version": crate::VERSION,
            "permissions": {"ecran": macos::permission_ecran(false), "accessibilite": macos::permission_accessibilite(false)},
            "verrouille": macos::ecran_verrouille(),
            "deverrouillage_verifie": verrou(&e.reglages).deverrouillage_verifie,
            "ecran_virtuel": ecran,
            "session": verrou(&e.session).as_ref().map(|s| s.ip.to_string()),
        }),
    )
}

fn api_diagnostic() -> Reponse {
    let e = etat();
    let (id, virtuel) = ecran_capture();
    let (l, h) = macos::pixels_ecran(id);
    let essai = |c| match macos::encodage::essai(c, (l.max(640) & !1).min(4096), (h.max(480) & !1).min(4096)) {
        Ok(ms) => json!({"ok": true, "ms_par_image": (ms * 10.0).round() / 10.0}),
        Err(e) => json!({"ok": false, "erreur": e}),
    };
    Reponse::json(
        200,
        &json!({
            "version": crate::VERSION,
            "permissions": {"ecran": macos::permission_ecran(false), "accessibilite": macos::permission_accessibilite(false)},
            "ecran": {"virtuel": virtuel, "id": id, "pixels": [l, h]},
            "encodeurs": {"h264": essai(Codec::Avc), "hevc": essai(Codec::Hevc)},
            "verrouillage": {"verrouille": macos::ecran_verrouille(), "deverrouillage_verifie": verrou(&e.reglages).deverrouillage_verifie},
        }),
    )
}

/// Verrouille le Mac, tape le mot de passe, regarde s'il s'est déverrouillé. Réussi, le
/// verrouillage après inactivité est permis. À faire devant le Mac : raté, il reste verrouillé.
fn api_essai_deverrouillage(r: &mut Requete) -> Reponse {
    let corps = match r.lire_corps(CORPS_MAX) {
        Ok(c) => c,
        Err(_) => return detail(413, "Requête trop grande"),
    };
    let Ok(Value::Object(v)) = serde_json::from_slice::<Value>(&corps) else { return detail(400, "JSON invalide") };
    let Some(mdp) = v.get("mdp").and_then(Value::as_str).filter(|m| !m.is_empty() && m.len() <= protocole::MDP_MAX) else {
        return detail(400, "mot de passe manquant");
    };
    if !macos::permission_accessibilite(false) {
        return detail(409, "permission Accessibilité manquante");
    }
    let e = etat();
    verrou(&e.entrees).verrouiller();
    let verrouille = (0..40).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        macos::ecran_verrouille()
    });
    if !verrouille {
        return Reponse::json(200, &json!({"ok": false, "message": "Le Mac ne s'est pas verrouillé (⌃⌘Q sans effet)."}));
    }
    std::thread::sleep(Duration::from_secs(1));
    verrou(&e.entrees).deverrouiller(mdp);
    let ouvert = (0..80).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        !macos::ecran_verrouille()
    });
    let mut reglages = verrou(&e.reglages);
    reglages.deverrouillage_verifie = ouvert;
    let _ = ecrire_reglages(&fichier_reglages(), &reglages);
    Reponse::json(
        200,
        &json!({"ok": ouvert, "message": if ouvert {
            "Déverrouillage depuis l'iPad vérifié : le Mac se verrouillera après 15 min sans geste."
        } else {
            "macOS a refusé la saisie sur l'écran verrouillé : déverrouillez le Mac sur place. Après inactivité, la session se terminera sans verrouiller."
        }}),
    )
}

fn api_lister(r: &Requete) -> Reponse {
    let rel = r.param("chemin").unwrap_or_default();
    let Some(d) = fichiers::resoudre(&dossier_personnel(), &rel).filter(|p| p.is_dir()) else { return detail(404, "dossier introuvable") };
    match fichiers::lister(&d) {
        Ok(entrees) => Reponse::json(200, &json!({"chemin": rel.trim_matches('/'), "entrees": entrees})),
        Err(_) => detail(403, "dossier illisible"),
    }
}

fn api_telecharger(r: &Requete) -> Reponse {
    let rel = r.param("chemin").unwrap_or_default();
    let Some(p) = fichiers::resoudre(&dossier_personnel(), &rel).filter(|p| p.is_file()) else { return detail(404, "fichier introuvable") };
    let Ok(f) = std::fs::File::open(&p) else { return detail(403, "fichier illisible") };
    let taille = f.metadata().map(|m| m.len()).unwrap_or(0);
    let nom = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Reponse::flux(200, "application/octet-stream", taille, f).entete("Content-Disposition", &nom_telechargement(&nom))
}

fn api_envoyer(r: &mut Requete) -> Reponse {
    let Some(nom) = r.param("nom").as_deref().and_then(fichiers::nom_sur) else { return detail(400, "nom de fichier refusé") };
    let taille: u64 = match r.entete("Content-Length").map(|v| v.trim().parse()) {
        Some(Ok(n)) => n,
        Some(Err(_)) => return detail(400, "Content-Length invalide"),
        None => return detail(411, "Content-Length manquant"),
    };
    if taille > fichiers::ENVOI_MAX {
        return detail(413, "fichier trop gros (2 Go au plus)");
    }
    let dossier = dossier_personnel().join("Downloads").join("web_bureau");
    match fichiers::recevoir(&dossier, &nom, taille, r.flux_corps()) {
        Ok(p) => Reponse::json(200, &json!({"nom": p.file_name().map(|n| n.to_string_lossy().into_owned())})),
        Err(fichiers::ErreurEnvoi::TropGros) => detail(413, "fichier trop gros"),
        Err(fichiers::ErreurEnvoi::Interrompu) => detail(400, "envoi interrompu"),
        Err(fichiers::ErreurEnvoi::Disque(_)) => detail(507, "écriture impossible sur le Mac"),
    }
}

// -- session -----------------------------------------------------------------------

/// L'écran capturé : l'écran virtuel s'il existe, sinon l'écran principal.
fn ecran_capture() -> (u32, bool) {
    match verrou(&etat().ecran).as_ref() {
        Some(v) => (v.id, true),
        None => (macos::ecran_principal(), false),
    }
}

/// La taille de capture pour un iPad de `l` × `h` pixels.
fn preparer_ecran(l: u32, h: u32) -> Result<(u32, usize, usize), String> {
    let e = etat();
    let mut ecran = verrou(&e.ecran);
    if let Some(v) = ecran.as_mut() {
        v.regler(l, h)?;
        let mut reglages = verrou(&e.reglages);
        if reglages.ecran != (l, h) {
            reglages.ecran = (l, h);
            let _ = ecrire_reglages(&fichier_reglages(), &reglages);
        }
        return Ok((v.id, l as usize, h as usize));
    }
    // écran principal : sa forme, réduite pour tenir dans l'iPad
    let id = macos::ecran_principal();
    let (pl, ph) = macos::pixels_ecran(id);
    let k = (l as f64 / pl.max(1) as f64).min(h as f64 / ph.max(1) as f64).min(1.0);
    Ok((id, ((pl as f64 * k) as usize) & !1, ((ph as f64 * k) as usize) & !1))
}

enum Message {
    Description(Vec<u8>),
    Image { n: u32, cle: bool, donnees: Vec<u8> },
}

/// Ce que partagent la capture (file de ScreenCaptureKit), l'encodeur et le fil du WebSocket.
struct Flux {
    regulation: Mutex<Regulation>,
    image_cle: AtomicBool,
    numero: AtomicU32,
}

fn envoyer_texte(c: &mut dyn Duplex, v: &Value) -> io::Result<()> {
    c.write_all(&ws::trame(ws::TEXTE, v.to_string().as_bytes()))?;
    c.flush()
}

/// Lit ce qui est arrivé (délai court) ; Err : connexion finie ou trame refusée.
fn recevoir(c: &mut dyn Duplex, trames: &mut Trames, tampon: &mut [u8]) -> Result<Vec<ws::Trame>, ()> {
    match c.read(tampon) {
        Ok(0) => return Err(()),
        Ok(n) => trames.ajouter(&tampon[..n]),
        Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {}
        Err(_) => return Err(()),
    }
    let mut v = Vec::new();
    while let Some(t) = trames.suivante().map_err(|_| ())? {
        v.push(t);
    }
    Ok(v)
}

fn session(c: &mut dyn Duplex, ip: IpAddr) {
    let mut trames = Trames::default();
    let mut tampon = vec![0u8; 64 * 1024];
    let _ = c.delai_lecture(Some(Duration::from_millis(50)));
    // 1. l'iPad se présente : taille de son écran, codecs que Safari décode
    let debut = Instant::now();
    let (largeur, hauteur, codecs) = 'attente: loop {
        if debut.elapsed() > Duration::from_secs(10) {
            return;
        }
        let Ok(recues) = recevoir(c, &mut trames, &mut tampon) else { return };
        for t in recues {
            if t.op == ws::TEXTE {
                if let Ok(Entree::Bonjour { largeur, hauteur, codecs }) = protocole::lire(&String::from_utf8_lossy(&t.donnees)) {
                    break 'attente (largeur & !1, hauteur & !1, codecs);
                }
            }
            return;
        }
    };
    // 2. prendre la place d'une session en cours
    let e = etat();
    let moi = Arc::new(Session { ip, fin: Mutex::new(None) });
    if let Some(ancienne) = verrou(&e.session).replace(moi.clone()) {
        *verrou(&ancienne.fin) = Some("reprise");
    }
    let _capture_tenue = verrou(&e.capture);
    let raison = derouler(c, &mut trames, &mut tampon, &moi, largeur, hauteur, &codecs);
    // 3. fin : tout relâcher ; verrouiller après inactivité si le déverrouillage est vérifié
    verrou(&e.entrees).relacher();
    if raison == "inactivite" && verrou(&e.reglages).deverrouillage_verifie {
        verrou(&e.entrees).verrouiller();
    }
    let _ = envoyer_texte(c, &json!({"t": "fin", "raison": raison}));
    let mut s = verrou(&e.session);
    if s.as_ref().is_some_and(|s| Arc::ptr_eq(s, &moi)) {
        *s = None;
        barre_menus::afficher("web_bureau".into());
    }
    println!("[{}] session {} terminée : {}", appairage::heure(), ip, raison);
}

/// La session elle-même ; rend la raison de sa fin.
fn derouler(
    c: &mut dyn Duplex,
    trames: &mut Trames,
    tampon: &mut [u8],
    moi: &Arc<Session>,
    largeur: u32,
    hauteur: u32,
    codecs: &[Codec],
) -> &'static str {
    let e = etat();
    if let Some(raison) = *verrou(&moi.fin) {
        return raison;
    }
    let erreur = |c: &mut dyn Duplex, texte: String| {
        println!("[!] session {} : {}", moi.ip, texte);
        let _ = envoyer_texte(c, &json!({"t": "erreur", "texte": texte}));
        "erreur"
    };
    if !macos::permission_ecran(false) {
        return erreur(c, "permission « Enregistrement de l'écran » manquante sur le Mac".into());
    }
    let (ecran, l, h) = match preparer_ecran(largeur, hauteur) {
        Ok(x) => x,
        Err(m) => return erreur(c, m),
    };
    verrou(&e.entrees).regler_ecran(ecran);
    println!("[{}] session {} : {}×{}", appairage::heure(), moi.ip, l, h);
    barre_menus::afficher(format!("● web_bureau — {}", moi.ip));
    let _veille = macos::empecher("PreventUserIdleDisplaySleep");

    let depart = Instant::now();
    let flux = Arc::new(Flux { regulation: Mutex::new(Regulation::new(Qualite::Auto, depart)), image_cle: AtomicBool::new(true), numero: AtomicU32::new(1) });
    let (tx, rx) = mpsc::sync_channel::<Message>(8);

    // encodeur : HEVC si Safari le décode et que la puce l'accepte, sinon H.264
    let rappel = |flux: Arc<Flux>, tx: mpsc::SyncSender<Message>| -> macos::encodage::RappelEncodeur {
        let tx = Mutex::new(tx);
        Box::new(move |n, image: Option<ImageEncodee>| {
            let tx = verrou(&tx);
            match image {
                None => verrou(&flux.regulation).oubliee(n),
                Some(i) => {
                    if let Some(d) = i.description {
                        let _ = tx.try_send(Message::Description(d));
                    }
                    if tx.try_send(Message::Image { n, cle: i.cle, donnees: i.donnees }).is_err() {
                        // image perdue avant l'envoi : la suivante doit pouvoir se décoder seule
                        verrou(&flux.regulation).oubliee(n);
                        flux.image_cle.store(true, Ordering::SeqCst);
                    }
                }
            }
        })
    };
    let debit = verrou(&flux.regulation).debit();
    let encodeur = [Codec::Hevc, Codec::Avc]
        .into_iter()
        .filter(|codec| codecs.contains(codec))
        .find_map(|codec| Encodeur::nouveau(codec, l, h, debit, rappel(flux.clone(), tx.clone())).ok());
    let Some(encodeur) = encodeur else { return erreur(c, "aucun encodeur matériel pour un codec que Safari décode".into()) };
    let codec_choisi = encodeur.codec;
    let chaine_codec = |d: &[u8]| match codec_choisi {
        Codec::Avc => video::codec_avc(d),
        Codec::Hevc => video::codec_hevc(d),
    };
    let encodeur = Arc::new(Mutex::new(Some(encodeur)));

    // capture → encodeur, si la régulation le permet
    let capture = {
        let (flux, encodeur) = (flux.clone(), encodeur.clone());
        Capture::demarrer(
            ecran,
            l,
            h,
            Box::new(move |image, pts: CMTime| {
                let t = Instant::now();
                // jamais la régulation tenue pendant l'encodage : le rappel de l'encodeur la prend
                let debit = {
                    let mut reg = verrou(&flux.regulation);
                    if !reg.peut_envoyer(t) {
                        return; // l'iPad n'a pas encore décodé : on saute cette capture
                    }
                    reg.debit()
                };
                let n = flux.numero.fetch_add(1, Ordering::SeqCst);
                verrou(&flux.regulation).envoyee(n, t);
                let cle = flux.image_cle.swap(false, Ordering::SeqCst);
                let donnee = match verrou(&encodeur).as_mut() {
                    Some(enc) => {
                        enc.regler_debit(debit);
                        enc.encoder(image, pts, cle, n)
                    }
                    None => false,
                };
                if !donnee {
                    verrou(&flux.regulation).oubliee(n);
                    if cle {
                        flux.image_cle.store(true, Ordering::SeqCst);
                    }
                }
            }),
        )
    };
    let capture = match capture {
        Ok(cap) => cap,
        Err(m) => return erreur(c, m),
    };

    // boucle : images vers l'iPad, entrées vers le Mac
    let _ = c.delai_lecture(Some(Duration::from_millis(1)));
    let us = |t: Instant| t.duration_since(depart).as_micros() as u64;
    let mut description: Vec<u8> = Vec::new();
    let mut derniere_entree = Instant::now();
    let (mut n_ping, mut ping_envoye, mut rtt_ms) = (0u64, Instant::now(), 0.0f64);
    let mut dernier_ping = Instant::now() - Duration::from_secs(5);
    let mut pp_compteur = presse_papiers::compteur();
    let mut dernier_pp = Instant::now();
    let mut verrouille: Option<bool> = None;
    let mut dernier_etat = Instant::now() - Duration::from_secs(5);
    let raison: &'static str = 'boucle: loop {
        if let Some(r) = *verrou(&moi.fin) {
            break r;
        }
        // images
        let mut premiere = true;
        loop {
            let m = if premiere { rx.recv_timeout(Duration::from_millis(2)).ok() } else { rx.try_recv().ok() };
            premiere = false;
            let Some(m) = m else { break };
            let ecrit = match m {
                Message::Description(d) if d != description => {
                    let Some(codec) = chaine_codec(&d) else { continue };
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&d);
                    description = d;
                    envoyer_texte(c, &json!({"t": "config", "codec": codec, "largeur": l, "hauteur": h, "description": b64}))
                }
                Message::Description(_) => Ok(()),
                Message::Image { n, .. } if description.is_empty() => {
                    verrou(&flux.regulation).oubliee(n);
                    flux.image_cle.store(true, Ordering::SeqCst);
                    Ok(())
                }
                Message::Image { n, cle, donnees } => {
                    let capture_us = verrou(&flux.regulation).instant(n).map(us).unwrap_or(0);
                    let mut corps = video::entete_image(cle, n, capture_us).to_vec();
                    corps.extend_from_slice(&donnees);
                    c.write_all(&ws::trame(ws::BINAIRE, &corps)).and_then(|_| c.flush())
                }
            };
            if ecrit.is_err() {
                break 'boucle "connexion_perdue";
            }
        }
        // entrées
        let Ok(recues) = recevoir(c, trames, tampon) else { break 'boucle "connexion_perdue" };
        for t in recues {
            match t.op {
                ws::FERMER => break 'boucle "fermee",
                ws::PING => {
                    if c.write_all(&ws::trame(ws::PONG, &t.donnees)).is_err() {
                        break 'boucle "connexion_perdue";
                    }
                    continue;
                }
                ws::PONG => continue,
                ws::TEXTE => {}
                _ => break 'boucle "message_refuse",
            }
            let entree = match protocole::lire(&String::from_utf8_lossy(&t.donnees)) {
                Ok(x) => x,
                Err(m) => {
                    println!("[!] session {} : message refusé ({m})", moi.ip);
                    break 'boucle "message_refuse";
                }
            };
            let geste = matches!(
                entree,
                Entree::Souris { .. } | Entree::Bouton { .. } | Entree::Molette { .. } | Entree::Touche { .. } | Entree::Texte(_) | Entree::Raccourci { .. }
            );
            if geste {
                derniere_entree = Instant::now();
            }
            match entree {
                Entree::Souris { x, y } => verrou(&e.entrees).souris(x, y),
                Entree::Bouton { b, bas } => verrou(&e.entrees).bouton(b, bas),
                Entree::Molette { dx, dy } => verrou(&e.entrees).molette(dx, dy),
                Entree::Touche { kvk, bas } => verrou(&e.entrees).touche(kvk, bas),
                Entree::Texte(s) => verrou(&e.entrees).texte(&s),
                Entree::Raccourci { indicateurs, cible } => verrou(&e.entrees).raccourci(indicateurs, cible),
                Entree::Relacher => verrou(&e.entrees).relacher(),
                Entree::Ack { n } => verrou(&flux.regulation).accusee(n, Instant::now()),
                Entree::ImageCle => flux.image_cle.store(true, Ordering::SeqCst),
                Entree::PressePapiers(texte) => {
                    presse_papiers::ecrire(&texte);
                    pp_compteur = presse_papiers::compteur(); // pas d'écho vers l'iPad
                }
                Entree::Deverrouiller(mdp) => {
                    derniere_entree = Instant::now();
                    // hors du fil de la session : la saisie prend une seconde
                    std::thread::spawn(move || verrou(&etat().entrees).deverrouiller(&mdp));
                }
                Entree::Pong { n } if n == n_ping => rtt_ms = ping_envoye.elapsed().as_secs_f64() * 1000.0,
                Entree::Qualite(q) => verrou(&flux.regulation).qualite(q),
                Entree::Pong { .. } | Entree::Bonjour { .. } => {}
            }
        }
        let maintenant = Instant::now();
        // horloge et temps aller-retour, pour la mesure de latence de la page
        if maintenant - dernier_ping > Duration::from_secs(1) {
            n_ping += 1;
            ping_envoye = maintenant;
            dernier_ping = maintenant;
            let debit = verrou(&flux.regulation).debit();
            let m = json!({"t": "ping", "n": n_ping, "us": us(maintenant), "rtt": rtt_ms, "debit": debit, "codec": format!("{:?}", codec_choisi)});
            if envoyer_texte(c, &m).is_err() {
                break 'boucle "connexion_perdue";
            }
        }
        // presse-papiers du Mac vers l'iPad
        if maintenant - dernier_pp > Duration::from_millis(250) {
            dernier_pp = maintenant;
            let n = presse_papiers::compteur();
            if n != pp_compteur {
                pp_compteur = n;
                if let Some(texte) = presse_papiers::lire().filter(|t| t.len() <= protocole::PRESSE_PAPIERS_MAX) {
                    if envoyer_texte(c, &json!({"t": "presse_papiers", "texte": texte})).is_err() {
                        break 'boucle "connexion_perdue";
                    }
                }
            }
        }
        // écran verrouillé ?
        if maintenant - dernier_etat > Duration::from_secs(1) {
            dernier_etat = maintenant;
            let v = macos::ecran_verrouille();
            if verrouille != Some(v) {
                verrouille = Some(v);
                if envoyer_texte(c, &json!({"t": "etat", "verrouille": v})).is_err() {
                    break 'boucle "connexion_perdue";
                }
            }
        }
        if maintenant - derniere_entree > INACTIVITE {
            break 'boucle "inactivite";
        }
    };
    drop(capture);
    *verrou(&encodeur) = None;
    raison
}

// -- lancement -------------------------------------------------------------------------

fn mises_a_jour(args: &Args) {
    let exe = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok());
    let Some(exe) = exe else { return };
    let (racine, bin) = (racine(), exe.parent().map(Path::to_path_buf).unwrap_or_default());
    let decision = maj::au_demarrage(&bin, &exe);
    match decision {
        maj::Decision::Revenir => {
            println!("[!] La nouvelle version a échoué {} fois au démarrage : retour à la précédente.", maj::ESSAIS_MAX);
            std::process::exit(production::CODE_MIS_A_JOUR); // launchd relance l'ancienne
        }
        maj::Decision::Compter(n) => println!("  Nouvelle version à l'essai (démarrage {n}/{}).", maj::ESSAIS_MAX),
        maj::Decision::Rien => {}
    }
    let b = bin.clone();
    std::thread::spawn(move || {
        std::thread::sleep(maj::TENUE);
        maj::tenue(&b);
    });
    if args.sans_maj || !production::est_copie_prod(&racine) {
        return;
    }
    if decision == maj::Decision::Rien {
        let _ = maj::avant(&exe); // la version actuelle a fait ses preuves : on la garde
    }
    match production::mettre_a_jour(DEPOT, "web_bureau", &racine, &|t| println!("  Mises à jour : {t}")) {
        Ok(true) => {
            let _ = maj::apres(&bin);
            println!("  Nouvelle version installée : relance.");
            std::process::exit(production::CODE_MIS_A_JOUR);
        }
        Ok(false) => {}
        Err(e) => println!("  [!] {e}"),
    }
}

pub fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--aide" || a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let args = match lire_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{USAGE}\nweb_bureau : {e}");
            std::process::exit(2);
        }
    };
    println!("\n  web_bureau {} — {}", crate::VERSION, appairage::heure());
    mises_a_jour(&args);
    if args.nouveau_jeton {
        let _ = std::fs::remove_file(appairage::fichier_jeton());
        println!("  Jeton remplacé : chaque appareil devra saisir un nouveau code.");
    }

    // permissions (la demande de macOS s'affiche sur l'écran du Mac, à accepter une fois)
    let ecran_ok = macos::permission_ecran(true);
    let clavier_ok = macos::permission_accessibilite(true);
    println!("  Enregistrement de l'écran : {}", if ecran_ok { "accordé" } else { "À ACCORDER (Réglages › Confidentialité et sécurité)" });
    println!("  Accessibilité (souris, clavier) : {}", if clavier_ok { "accordée" } else { "À ACCORDER (Réglages › Confidentialité et sécurité)" });
    std::mem::forget(macos::empecher("PreventUserIdleSystemSleep")); // jamais de veille tant qu'il tourne

    let reglages = lire_reglages(&fichier_reglages());
    let ecran = if args.sans_ecran_virtuel {
        None
    } else {
        match EcranVirtuel::creer(reglages.ecran.0, reglages.ecran.1) {
            Ok(e) => {
                println!("  Écran virtuel : {}×{} pixels", e.largeur, e.hauteur);
                Some(e)
            }
            Err(m) => {
                println!("  [!] {m} : capture de l'écran principal (repli : bouchon HDMI factice).");
                None
            }
        }
    };
    let id = ecran.as_ref().map(|e| e.id).unwrap_or_else(macos::ecran_principal);
    let _ = ETAT.set(Etat {
        ecran: Mutex::new(ecran),
        entrees: Mutex::new(Entrees::new(id)),
        session: Mutex::new(None),
        capture: Mutex::new(()),
        reglages: Mutex::new(reglages),
    });

    let reseau = !args.local;
    let srv = match Serveur::ouvrir(args.port, reseau, 10) {
        Ok(s) => s,
        Err(e) => {
            println!("[X] {e}");
            std::process::exit(1);
        }
    };
    if srv.port() != args.port {
        println!("  (le port {} était occupé)", args.port);
    }
    let _ = GARDE.set(Garde::new("web_bureau", reseau, None, None));
    let garde = GARDE.get().unwrap();
    garde.brancher(&srv);
    if reseau && garde.schema() != "https" {
        // WebCodecs exige HTTPS, et cet outil contrôle tout le Mac : jamais en clair sur le réseau
        println!("[X] HTTPS impossible (openssl introuvable ou WEB_BUREAU_HTTP=1) : refus du mode réseau.");
        std::process::exit(3);
    }
    println!("  Sur ce Mac : http://127.0.0.1:{}/", srv.port());
    if reseau {
        println!("  Depuis l'iPad :");
        for ip in appairage::adresses_poste().iter().filter(|i| !i.starts_with("127.")) {
            println!("      {}", garde.adresse(ip, srv.port()));
        }
        garde.annoncer();
    }
    let _ = io::stdout().flush();
    let s = srv.clone();
    std::thread::spawn(move || s.servir(Arc::new(servir)));
    barre_menus::lancer(|| {
        if let Some(s) = verrou(&etat().session).as_ref() {
            *verrou(&s.fin) = Some("coupee_sur_mac");
        }
    });
}

#[cfg(test)]
mod essais {
    use super::*;
    use std::net::TcpStream;

    fn a(l: &[&str]) -> Vec<String> {
        l.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn ligne_de_commande() {
        let x = lire_args(&a(&["--port", "9000", "--local", "--sans-maj", "--sans-navigateur", "--sans-pause"])).unwrap();
        assert_eq!((x.port, x.local, x.sans_maj), (9000, true, true));
        assert_eq!(lire_args(&[]).unwrap().port, PORT);
        assert_eq!(lire_args(&a(&["--port=8810"])).unwrap().port, 8810);
        assert!(lire_args(&a(&["--port"])).is_err());
        assert!(lire_args(&a(&["--port", "x"])).is_err());
        assert!(lire_args(&a(&["--inconnu"])).is_err());
    }

    #[test]
    fn reglages_abimes_mis_de_cote() {
        let d = std::env::temp_dir().join(format!("web_bureau-reglages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("reglages.json");
        assert_eq!(lire_reglages(&f).ecran, ECRAN_DEFAUT);
        let r = Reglages { deverrouillage_verifie: true, ecran: (2420, 1668) };
        ecrire_reglages(&f, &r).unwrap();
        assert_eq!(lire_reglages(&f), r);
        std::fs::write(&f, "{abîmé").unwrap();
        assert_eq!(lire_reglages(&f).deverrouillage_verifie, false);
        assert!(d.join("reglages.abime.json").exists());
        assert!(!f.exists());
    }

    #[test]
    fn fichiers_de_la_page() {
        let web = std::env::temp_dir().join(format!("web_bureau-web-{}", std::process::id()));
        std::fs::create_dir_all(web.join("js")).unwrap();
        std::fs::write(web.join("index.html"), "<p>").unwrap();
        std::fs::write(web.join("js/a.js"), "1").unwrap();
        std::fs::write(web.join("secret.toml"), "x").unwrap();
        for (chemin, code) in
            [("/", 200), ("/js/a.js", 200), ("/js/%61.js", 200), ("/../Cargo.toml", 404), ("/.git/config", 404), ("/secret.toml", 404), ("/js/../../x.js", 404)]
        {
            assert_eq!(fichier_web(&web, chemin).code, code, "{chemin}");
        }
        assert_eq!(nom_telechargement("été 1.pdf"), "attachment; filename*=UTF-8''%C3%A9t%C3%A9%201.pdf");
    }

    /// Un vrai serveur sur la boucle locale (garde inactive comme en --local) : les refus des
    /// écritures arrivent avant tout accès au disque ou à l'écran.
    #[test]
    fn ecritures_refusees() {
        let _ = GARDE.set(Garde::new("web_bureau", false, None, None));
        let srv = Serveur::ouvrir(0, false, 1).unwrap();
        srv.servir_en_fond(Arc::new(servir));
        let port = srv.port();
        let req = |brut: String| {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            c.write_all(brut.as_bytes()).unwrap();
            let mut r = String::new();
            let _ = c.read_to_string(&mut r);
            r
        };
        let code = |r: &str| r.split(' ').nth(1).unwrap_or("").to_string();
        let tete = |ligne: &str, extra: &str| format!("{ligne} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}\r\n");
        // sans l'en-tête maison : 403 (CSRF)
        let r = req(tete("POST /api/fichiers/envoyer?nom=a.txt", "Content-Length: 1\r\n") + "x");
        assert_eq!(code(&r), "403", "{r}");
        assert!(r.contains("X-Content-Type-Options: nosniff"), "{r}");
        assert!(r.contains("frame-ancestors 'self'"), "{r}");
        // nom dangereux, longueur manquante, trop gros : refusés avant d'écrire
        let h = "X-WEB_BUREAU: 1\r\n";
        assert_eq!(code(&req(tete("POST /api/fichiers/envoyer?nom=..%2Fx", &format!("{h}Content-Length: 1\r\n")))), "400");
        assert_eq!(code(&req(tete("POST /api/fichiers/envoyer?nom=a.txt", h))), "411");
        let gros = format!("{h}Content-Length: {}\r\n", fichiers::ENVOI_MAX + 1);
        assert_eq!(code(&req(tete("POST /api/fichiers/envoyer?nom=a.txt", &gros))), "413");
        // routes inconnues, méthodes refusées
        assert_eq!(code(&req(tete("GET /api/inconnue", ""))), "404");
        assert_eq!(code(&req(tete("DELETE /api/etat", h))), "404");
        // WebSocket d'un autre site
        let ws = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nOrigin: https://piege.exemple\r\n";
        assert_eq!(code(&req(tete("GET /ws", ws))), "403");
        srv.arreter();
    }
}
