//! Messages de l'iPad vers le Mac (trames texte JSON du WebSocket), validés ici : tout ce qui
//! sort de ces bornes est refusé, et le serveur ferme la session.

use serde_json::Value;

use crate::clavier;

pub const TEXTE_MAX: usize = 1000; // caractères tapés par message
pub const PRESSE_PAPIERS_MAX: usize = 1 << 20; // octets
pub const MDP_MAX: usize = 256;
pub const MOLETTE_MAX: f64 = 5000.0;
pub const ECRAN_MIN: u32 = 320;
pub const ECRAN_MAX: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Codec {
    Avc,
    Hevc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Qualite {
    Econome,
    Auto,
    Maximum,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entree {
    /// Taille de l'écran de l'iPad en pixels, codecs que Safari décode.
    Bonjour { largeur: u32, hauteur: u32, codecs: Vec<Codec> },
    /// Position normalisée dans l'image (0..1).
    Souris { x: f64, y: f64 },
    /// 0 gauche, 1 droit, 2 milieu.
    Bouton { b: u8, bas: bool },
    /// Pixels ; dy > 0 fait descendre le contenu (sens du navigateur).
    Molette { dx: f64, dy: f64 },
    /// Code virtuel macOS déjà traduit (clavier physique).
    Touche { kvk: u16, bas: bool },
    /// Texte du clavier à l'écran (Unicode).
    Texte(String),
    /// Une combinaison d'un geste : indicateurs + une touche (code) ou un caractère (selon la
    /// disposition du Mac : ⌘C reste ⌘C en AZERTY).
    Raccourci { indicateurs: u64, cible: Cible },
    /// Relâcher tout ce qui est enfoncé (l'iPad a perdu le focus).
    Relacher,
    Ack { n: u32 },
    ImageCle,
    PressePapiers(String),
    Deverrouiller(String),
    Pong { n: u64 },
    Qualite(Qualite),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cible {
    Touche(u16),
    Caractere(char),
}

fn nombre(v: &Value, cle: &str) -> Result<f64, String> {
    v[cle].as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("« {cle} » manquant ou invalide"))
}

fn booleen(v: &Value, cle: &str) -> Result<bool, String> {
    v[cle].as_bool().ok_or_else(|| format!("« {cle} » manquant"))
}

fn texte<'a>(v: &'a Value, cle: &str, max_octets: usize) -> Result<&'a str, String> {
    let s = v[cle].as_str().ok_or_else(|| format!("« {cle} » manquant"))?;
    if s.len() > max_octets {
        return Err(format!("« {cle} » trop long"));
    }
    Ok(s)
}

fn dans_l_image(x: f64) -> Result<f64, String> {
    if (0.0..=1.0).contains(&x) { Ok(x) } else { Err("position hors de l'image".into()) }
}

pub fn lire(message: &str) -> Result<Entree, String> {
    let v: Value = serde_json::from_str(message).map_err(|_| "JSON invalide".to_string())?;
    let t = v["t"].as_str().ok_or("type manquant")?;
    Ok(match t {
        "bonjour" => {
            let dim = |i: usize| {
                v["ecran"][i].as_u64().filter(|n| (ECRAN_MIN as u64..=ECRAN_MAX as u64).contains(n)).map(|n| n as u32)
            };
            let (Some(largeur), Some(hauteur)) = (dim(0), dim(1)) else { return Err("taille d'écran invalide".into()) };
            let codecs = v["codecs"].as_array().ok_or("codecs manquants")?;
            if codecs.len() > 4 {
                return Err("trop de codecs".into());
            }
            let codecs = codecs
                .iter()
                .filter_map(|c| match c.as_str() {
                    Some("avc") => Some(Codec::Avc),
                    Some("hevc") => Some(Codec::Hevc),
                    _ => None,
                })
                .collect();
            Entree::Bonjour { largeur, hauteur, codecs }
        }
        "souris" => Entree::Souris { x: dans_l_image(nombre(&v, "x")?)?, y: dans_l_image(nombre(&v, "y")?)? },
        "bouton" => {
            let b = v["b"].as_u64().filter(|b| *b <= 2).ok_or("bouton invalide")? as u8;
            Entree::Bouton { b, bas: booleen(&v, "bas")? }
        }
        "molette" => {
            let (dx, dy) = (nombre(&v, "dx")?, nombre(&v, "dy")?);
            if dx.abs() > MOLETTE_MAX || dy.abs() > MOLETTE_MAX {
                return Err("défilement trop grand".into());
            }
            Entree::Molette { dx, dy }
        }
        "touche" => {
            let kvk = clavier::code_mac(texte(&v, "code", 32)?).ok_or("touche inconnue")?;
            Entree::Touche { kvk, bas: booleen(&v, "bas")? }
        }
        "texte" => {
            let s = texte(&v, "s", TEXTE_MAX * 4)?;
            if s.chars().count() > TEXTE_MAX || s.is_empty() {
                return Err("texte vide ou trop long".into());
            }
            Entree::Texte(s.to_string())
        }
        "raccourci" => {
            let mods: Vec<&str> = v["mods"].as_array().ok_or("mods manquants")?.iter().filter_map(Value::as_str).collect();
            let indicateurs = clavier::modificateurs(&mods).ok_or("modificateur inconnu")?;
            let cible = if let Some(c) = v["code"].as_str() {
                Cible::Touche(clavier::code_mac(c).ok_or("touche inconnue")?)
            } else {
                let s = texte(&v, "c", 8)?;
                let mut cs = s.chars();
                match (cs.next(), cs.next()) {
                    (Some(c), None) if !c.is_control() => Cible::Caractere(c),
                    _ => return Err("un seul caractère attendu".into()),
                }
            };
            Entree::Raccourci { indicateurs, cible }
        }
        "relacher" => Entree::Relacher,
        "ack" => Entree::Ack { n: v["n"].as_u64().filter(|n| *n <= u32::MAX as u64).ok_or("ack invalide")? as u32 },
        "image_cle" => Entree::ImageCle,
        "presse_papiers" => Entree::PressePapiers(texte(&v, "texte", PRESSE_PAPIERS_MAX)?.to_string()),
        "deverrouiller" => {
            let mdp = texte(&v, "mdp", MDP_MAX)?;
            if mdp.is_empty() || mdp.chars().any(char::is_control) {
                return Err("mot de passe invalide".into());
            }
            Entree::Deverrouiller(mdp.to_string())
        }
        "pong" => Entree::Pong { n: v["n"].as_u64().ok_or("pong invalide")? },
        "qualite" => Entree::Qualite(match v["q"].as_str() {
            Some("econome") => Qualite::Econome,
            Some("auto") => Qualite::Auto,
            Some("maximum") => Qualite::Maximum,
            _ => return Err("qualité inconnue".into()),
        }),
        _ => return Err("type inconnu".into()),
    })
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn messages_valides() {
        assert_eq!(
            lire(r#"{"t":"bonjour","ecran":[2266,1488],"codecs":["hevc","avc","vp9"]}"#),
            Ok(Entree::Bonjour { largeur: 2266, hauteur: 1488, codecs: vec![Codec::Hevc, Codec::Avc] })
        );
        assert_eq!(lire(r#"{"t":"souris","x":0.5,"y":1}"#), Ok(Entree::Souris { x: 0.5, y: 1.0 }));
        assert_eq!(lire(r#"{"t":"bouton","b":1,"bas":true}"#), Ok(Entree::Bouton { b: 1, bas: true }));
        assert_eq!(lire(r#"{"t":"touche","code":"KeyQ","bas":false}"#), Ok(Entree::Touche { kvk: 0x0C, bas: false }));
        assert_eq!(lire(r#"{"t":"texte","s":"é😀"}"#), Ok(Entree::Texte("é😀".into())));
        assert_eq!(
            lire(r#"{"t":"raccourci","mods":["cmd"],"c":"c"}"#),
            Ok(Entree::Raccourci { indicateurs: clavier::CMD, cible: Cible::Caractere('c') })
        );
        assert_eq!(
            lire(r#"{"t":"raccourci","mods":["ctrl"],"code":"ArrowUp"}"#),
            Ok(Entree::Raccourci { indicateurs: clavier::CTRL, cible: Cible::Touche(0x7E) })
        );
        assert_eq!(lire(r#"{"t":"ack","n":42}"#), Ok(Entree::Ack { n: 42 }));
        assert_eq!(lire(r#"{"t":"qualite","q":"econome"}"#), Ok(Entree::Qualite(Qualite::Econome)));
        assert_eq!(lire(r#"{"t":"relacher"}"#), Ok(Entree::Relacher));
    }

    #[test]
    fn messages_refuses() {
        for m in [
            "pas du json",
            r#"{"t":"inconnu"}"#,
            r#"{"x":1}"#,
            r#"{"t":"souris","x":1.5,"y":0}"#,
            r#"{"t":"souris","x":-0.1,"y":0}"#,
            r#"{"t":"souris","x":"0.5","y":0}"#,
            r#"{"t":"bouton","b":3,"bas":true}"#,
            r#"{"t":"bouton","b":0}"#,
            r#"{"t":"molette","dx":0,"dy":99999}"#,
            r#"{"t":"touche","code":"Power","bas":true}"#,
            r#"{"t":"texte","s":""}"#,
            r#"{"t":"raccourci","mods":["hyper"],"c":"c"}"#,
            r#"{"t":"raccourci","mods":["cmd"],"c":"ab"}"#,
            r#"{"t":"raccourci","mods":["cmd"],"c":"\n"}"#,
            r#"{"t":"bonjour","ecran":[100,100],"codecs":[]}"#,
            r#"{"t":"bonjour","ecran":[2266],"codecs":[]}"#,
            r#"{"t":"deverrouiller","mdp":""}"#,
            r#"{"t":"deverrouiller","mdp":"a\nb"}"#,
            r#"{"t":"qualite","q":"ultra"}"#,
            r#"{"t":"ack","n":-1}"#,
        ] {
            assert!(lire(m).is_err(), "{m}");
        }
        let long = format!(r#"{{"t":"texte","s":"{}"}}"#, "a".repeat(TEXTE_MAX + 1));
        assert!(lire(&long).is_err());
        let mdp = format!(r#"{{"t":"deverrouiller","mdp":"{}"}}"#, "a".repeat(MDP_MAX + 1));
        assert!(lire(&mdp).is_err());
    }
}
