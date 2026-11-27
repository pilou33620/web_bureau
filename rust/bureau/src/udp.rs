//! Vidéo par UDP : WebTransport (HTTP/3 sur QUIC), sur le même numéro de port que le serveur HTTPS
//! et avec le même certificat. Chaque image part dans son propre flux QUIC : un paquet perdu ne
//! retient que son image, les suivantes arrivent quand même, et la page décide d'attendre ou de
//! demander une image clé. Seule la vidéo passe ici ; souris, clavier, accusés de réception et
//! tout le reste gardent le WebSocket, qui porte aussi la vidéo tant que cette liaison n'est pas
//! branchée (Safari sans WebTransport, UDP filtré) ou si elle tombe.
//!
//! WebTransport n'envoie pas le cookie de la page : la session du WebSocket, elle authentifiée,
//! reçoit un billet à usage unique que la page présente ici.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use wtransport::config::IpBindConfig;
use wtransport::endpoint::IncomingSession;
use wtransport::tls::{rustls, WEBTRANSPORT_ALPN};
use wtransport::{Endpoint, ServerConfig, VarInt};

use crate::commun::appairage::{globale, heure};

type Images = mpsc::UnboundedSender<Vec<u8>>;

/// Liaisons QUIC ouvertes en même temps, poignées de main comprises.
const LIAISONS_MAX: usize = 16;

enum Liaison {
    /// L'iPad ne s'est pas encore branché.
    Attente,
    Branchee(Images),
    /// Session finie ou iPad parti : plus rien ne passera par ici.
    Finie,
}

fn verrou<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Les billets des sessions en cours.
#[derive(Default)]
pub struct Billets(Mutex<HashMap<String, Arc<Mutex<Liaison>>>>);

impl Billets {
    /// Un passage pour la session qui appelle : son billet (à donner à la page) et de quoi envoyer.
    pub fn ouvrir(self: &Arc<Self>) -> Passage {
        let mut octets = [0u8; 16];
        getrandom::getrandom(&mut octets).expect("générateur aléatoire du système");
        let billet: String = octets.iter().map(|o| format!("{o:02x}")).collect();
        let liaison = Arc::new(Mutex::new(Liaison::Attente));
        verrou(&self.0).insert(billet.clone(), liaison.clone());
        Passage { billets: self.clone(), billet, liaison }
    }

    /// Le billet ne sert qu'une fois.
    fn prendre(&self, billet: &str) -> Option<Arc<Mutex<Liaison>>> {
        verrou(&self.0).remove(billet)
    }
}

/// Le côté session d'une liaison UDP. Lâché, il la ferme et périme son billet.
pub struct Passage {
    billets: Arc<Billets>,
    pub billet: String,
    liaison: Arc<Mutex<Liaison>>,
}

impl Passage {
    /// Envoie une image par UDP. Err(l'image) : la liaison n'est pas (ou plus) branchée, à envoyer autrement.
    pub fn envoyer(&self, image: Vec<u8>) -> Result<(), Vec<u8>> {
        let mut liaison = verrou(&self.liaison);
        let Liaison::Branchee(images) = &*liaison else { return Err(image) };
        images.send(image).map_err(|refusee| {
            *liaison = Liaison::Finie;
            refusee.0
        })
    }
}

impl Drop for Passage {
    fn drop(&mut self) {
        self.billets.prendre(&self.billet);
        *verrou(&self.liaison) = Liaison::Finie; // lâche l'émetteur : la tâche de la liaison s'arrête
    }
}

/// Écoute en UDP sur `port` (0 : au choix du système), sur 127.0.0.1 seulement si `local`, avec la
/// configuration TLS du serveur HTTPS. Rend le port d'écoute.
pub fn demarrer(local: bool, port: u16, mut tls: rustls::ServerConfig, billets: Arc<Billets>) -> io::Result<u16> {
    tls.alpn_protocols = vec![WEBTRANSPORT_ALPN.to_vec()];
    let config = ServerConfig::builder()
        .with_bind_config(if local { IpBindConfig::LocalV4 } else { IpBindConfig::InAddrAnyDual }, port)
        .with_custom_tls(tls)
        // un écran immobile n'envoie rien : sans ce signal, la liaison expirerait
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();
    let (pret, attente) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("udp".into()).spawn(move || {
        let execution = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(e) => e,
            Err(e) => return drop(pret.send(Err(e))),
        };
        execution.block_on(async move {
            let point = match Endpoint::server(config) {
                Ok(p) => p,
                Err(e) => return drop(pret.send(Err(e))),
            };
            let _ = pret.send(point.local_addr().map(|a| a.port()));
            loop {
                let arrivee = point.accept().await;
                // une session à la fois, plus celle qui la remplace : au-delà, quelqu'un occupe le port
                if point.open_connections() > LIAISONS_MAX {
                    arrivee.refuse();
                    continue;
                }
                tokio::spawn(accueillir(arrivee, billets.clone()));
            }
        });
    })?;
    attente.recv().unwrap_or_else(|_| Err(io::Error::other("le fil UDP n'a pas démarré")))
}

async fn accueillir(arrivee: IncomingSession, billets: Arc<Billets>) {
    // comme le serveur HTTP : jamais depuis Internet
    if globale(arrivee.remote_address().ip().to_canonical()) {
        return arrivee.refuse();
    }
    let Ok(demande) = arrivee.await else { return };
    let ip = demande.remote_address().ip().to_canonical();
    let Some(liaison) = demande.path().strip_prefix("/udp?b=").and_then(|b| billets.prendre(b)) else {
        println!("[{}] UDP {ip} : liaison refusée (billet inconnu ou déjà servi)", heure());
        return demande.forbidden().await;
    };
    let connexion = match demande.accept().await {
        Ok(c) => c,
        Err(e) => return println!("[{}] UDP {ip} : liaison échouée ({e})", heure()),
    };
    let (images, mut suite) = mpsc::unbounded_channel::<Vec<u8>>();
    {
        let mut l = verrou(&liaison);
        if !matches!(*l, Liaison::Attente) {
            return; // la session a fini entre-temps
        }
        *l = Liaison::Branchee(images);
    }
    println!("[{}] UDP {ip} : liaison branchée, la vidéo passe par UDP", heure());
    let connexion = Arc::new(connexion);
    loop {
        tokio::select! {
            image = suite.recv() => {
                let Some(image) = image else { break }; // session finie
                let connexion = connexion.clone();
                // une tâche par image : une image retenue par un paquet perdu ne bloque pas la suivante
                tokio::spawn(async move {
                    let Ok(ouverture) = connexion.open_uni().await else { return };
                    let Ok(mut flux) = ouverture.await else { return };
                    if flux.write_all(&image).await.is_ok() {
                        let _ = flux.finish().await;
                    }
                });
            }
            // l'iPad est parti : la vidéo repasse par le WebSocket
            raison = connexion.closed() => {
                println!("[{}] UDP {ip} : liaison fermée ({raison})", heure());
                break;
            }
        }
    }
    *verrou(&liaison) = Liaison::Finie;
    connexion.close(VarInt::from_u32(0), b"fin");
}

#[cfg(test)]
mod essais {
    use super::*;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use wtransport::{ClientConfig, Identity};

    /// La même forme de configuration que le serveur HTTPS (appairage::contexte_tls).
    fn tls() -> rustls::ServerConfig {
        let identite = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        let certificats = identite.certificate_chain().as_slice().iter().map(|c| CertificateDer::from(c.der().to_vec())).collect();
        let cle = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identite.private_key().secret_der().to_vec()));
        rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certificats, cle)
            .unwrap()
    }

    async fn lire(connexion: &wtransport::Connection) -> Vec<u8> {
        let mut flux = connexion.accept_uni().await.unwrap();
        let (mut tout, mut tampon) = (Vec::new(), [0u8; 4096]);
        while let Some(n) = flux.read(&mut tampon).await.unwrap() {
            tout.extend_from_slice(&tampon[..n]);
        }
        tout
    }

    #[test]
    fn une_image_par_flux_avec_le_bon_billet() {
        let billets = Arc::new(Billets::default());
        let port = demarrer(true, 0, tls(), billets.clone()).unwrap();
        let passage = billets.ouvrir();
        // pas encore branchée : l'image revient, à envoyer par le WebSocket
        assert_eq!(passage.envoyer(vec![1, 2, 3]), Err(vec![1, 2, 3]));

        let execution = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        execution.block_on(async {
            let client = Endpoint::client(ClientConfig::builder().with_bind_default().with_no_cert_validation().build()).unwrap();
            let adresse = |billet: &str| format!("https://127.0.0.1:{port}/udp?b={billet}");
            assert!(client.connect(adresse("0123456789abcdef0123456789abcdef")).await.is_err(), "billet inconnu");
            assert!(client.connect(format!("https://127.0.0.1:{port}/autre")).await.is_err(), "autre chemin");

            let connexion = client.connect(adresse(&passage.billet)).await.unwrap();
            assert!(client.connect(adresse(&passage.billet)).await.is_err(), "un billet ne sert qu'une fois");
            // la liaison se branche juste après l'accord : quelques essais
            let grande = vec![7u8; 300_000];
            let mut image = grande.clone();
            for essai in 0.. {
                match passage.envoyer(image) {
                    Ok(()) => break,
                    Err(rendue) => image = rendue,
                }
                assert!(essai < 200, "liaison jamais branchée");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(passage.envoyer(vec![9; 10]), Ok(()));
            let mut recues = [lire(&connexion).await, lire(&connexion).await];
            recues.sort_by_key(|i| i.len()); // chaque image a son flux : l'ordre d'arrivée n'est pas garanti
            assert_eq!(recues[0], vec![9; 10]);
            assert_eq!(recues[1], grande);

            // session finie : la liaison se ferme, côté iPad aussi
            drop(passage);
            connexion.closed().await;
        });
    }
}
