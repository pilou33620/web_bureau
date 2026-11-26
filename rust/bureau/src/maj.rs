//! Retour arrière d'une mise à jour : sur un Mac sans écran, une version qui plante au démarrage
//! coupe l'accès. Avant chaque mise à jour, l'exécutable est copié en `<exe>.precedent` ; après,
//! `bin/essai` compte les démarrages de la nouvelle version. Au-delà d'ESSAIS_MAX démarrages qui
//! n'ont pas tenu TENUE, l'ancienne version revient (launchd la relance).

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

pub const ESSAIS_MAX: u32 = 3;
/// Une version qui tourne depuis ce délai est bonne : `bin/essai` est retiré.
pub const TENUE: Duration = Duration::from_secs(60);
const ESSAI: &str = "essai";

#[derive(Debug, PartialEq)]
pub enum Decision {
    Rien,
    Compter(u32),
    Revenir,
}

/// `essai` : le contenu de bin/essai s'il existe ; `precedent` : l'ancienne version est là.
pub fn decider(essai: Option<&str>, precedent: bool) -> Decision {
    let Some(texte) = essai else { return Decision::Rien };
    let n = texte.trim().parse::<u32>().unwrap_or(ESSAIS_MAX) + 1;
    if n > ESSAIS_MAX && precedent { Decision::Revenir } else { Decision::Compter(n) }
}

fn precedent(exe: &Path) -> std::path::PathBuf {
    let mut p = exe.as_os_str().to_owned();
    p.push(".precedent");
    p.into()
}

/// Avant une mise à jour : garder la version actuelle.
pub fn avant(exe: &Path) -> io::Result<()> {
    let p = precedent(exe);
    let tmp = exe.with_extension("precedent.tmp");
    fs::copy(exe, &tmp)?;
    fs::rename(&tmp, p)
}

/// Après une mise à jour réussie : la nouvelle version est à l'essai.
pub fn apres(bin: &Path) -> io::Result<()> {
    fs::write(bin.join(ESSAI), "0\n")
}

/// Au démarrage. Rend Decision::Revenir si l'ancienne version a été remise en place : sortir
/// alors (code 75) pour que launchd la relance.
pub fn au_demarrage(bin: &Path, exe: &Path) -> Decision {
    let essai = fs::read_to_string(bin.join(ESSAI)).ok();
    let d = decider(essai.as_deref(), precedent(exe).is_file());
    match &d {
        Decision::Rien => {}
        Decision::Compter(n) => {
            let _ = fs::write(bin.join(ESSAI), format!("{n}\n"));
        }
        Decision::Revenir => {
            if fs::rename(precedent(exe), exe).is_err() {
                return Decision::Compter(ESSAIS_MAX + 1);
            }
            let _ = fs::remove_file(bin.join(ESSAI));
        }
    }
    d
}

/// La version a tenu TENUE : elle est bonne.
pub fn tenue(bin: &Path) {
    let _ = fs::remove_file(bin.join(ESSAI));
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn decisions() {
        assert_eq!(decider(None, true), Decision::Rien);
        assert_eq!(decider(Some("0\n"), true), Decision::Compter(1));
        assert_eq!(decider(Some("2"), true), Decision::Compter(3));
        assert_eq!(decider(Some("3"), true), Decision::Revenir);
        assert_eq!(decider(Some("3"), false), Decision::Compter(4)); // rien vers quoi revenir
        assert_eq!(decider(Some("abîmé"), true), Decision::Revenir);
    }

    #[test]
    fn retour_a_l_ancienne_version() {
        let d = std::env::temp_dir().join(format!("web_bureau-maj-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let exe = d.join("web_bureau");
        fs::write(&exe, "v1").unwrap();
        avant(&exe).unwrap();
        fs::write(&exe, "v2 cassée").unwrap();
        apres(&d).unwrap();
        for n in 1..=ESSAIS_MAX {
            assert_eq!(au_demarrage(&d, &exe), Decision::Compter(n));
        }
        assert_eq!(au_demarrage(&d, &exe), Decision::Revenir);
        assert_eq!(fs::read_to_string(&exe).unwrap(), "v1");
        assert_eq!(au_demarrage(&d, &exe), Decision::Rien);
        // une version qui tient n'est plus à l'essai
        apres(&d).unwrap();
        tenue(&d);
        assert_eq!(au_demarrage(&d, &exe), Decision::Rien);
    }
}
