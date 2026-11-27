//! Le journal du serveur, montré à l'iPad (panneau Journal) : tout ce que le serveur écrit sur sa
//! sortie, erreurs et paniques comprises, que launchd range dans ~/Library/Logs/web_bureau.log.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Ce que la page reçoit au plus : la fin du fichier.
pub const TAILLE_MAX: u64 = 128 * 1024;

/// La fin du journal, depuis un début de ligne, code d'appairage masqué.
pub fn fin(chemin: &Path) -> io::Result<String> {
    let mut f = File::open(chemin)?;
    let debut = f.metadata()?.len().saturating_sub(TAILLE_MAX);
    f.seek(SeekFrom::Start(debut))?;
    let mut octets = Vec::new();
    f.take(TAILLE_MAX).read_to_end(&mut octets)?;
    let texte = String::from_utf8_lossy(&octets);
    // coupé en pleine ligne : on repart de la suivante
    let texte = if debut > 0 { texte.split_once('\n').map_or("", |(_, suite)| suite) } else { &texte };
    Ok(texte.lines().map(masquer).collect::<Vec<_>>().join("\n"))
}

/// Le code d'appairage ne sort pas du Mac : ce journal s'affiche sur l'iPad et se recopie pour
/// demander de l'aide.
fn masquer(ligne: &str) -> String {
    if !ligne.contains("Code d'appairage") {
        return ligne.to_string();
    }
    let (code, suite) = ligne.split_once('(').unwrap_or((ligne, ""));
    let code: String = code.chars().map(|c| if c.is_ascii_digit() { '•' } else { c }).collect();
    if suite.is_empty() { code } else { format!("{code}({suite}") }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn fin_du_journal() {
        let dossier = std::env::temp_dir().join(format!("web_bureau_journal_{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let f = dossier.join("journal.log");
        assert!(fin(&f).is_err(), "fichier absent");

        std::fs::write(&f, "  web_bureau 0.1.0\n  Code d'appairage web_bureau : 123 456   (à saisir sur la tablette, valable 10 min)\n[12:00:00] session 192.168.1.20 terminée\n").unwrap();
        assert_eq!(
            fin(&f).unwrap(),
            "  web_bureau 0.1.0\n  Code d'appairage web_bureau : ••• •••   (à saisir sur la tablette, valable 10 min)\n[12:00:00] session 192.168.1.20 terminée"
        );

        // trop long : la fin seulement, depuis un début de ligne ; des octets hors UTF-8 ne cassent rien
        let mut gros = "début\n".to_string().into_bytes();
        gros.extend(std::iter::repeat_n(b"0123456789abcdef0123456789abcde\n".as_slice(), 8192).flatten());
        gros.extend(b"\xFF fin\n");
        std::fs::write(&f, &gros).unwrap();
        let t = fin(&f).unwrap();
        assert!(t.len() as u64 <= TAILLE_MAX);
        assert!(t.starts_with("0123456789abcdef0123456789abcde\n"), "{:?}", &t[..40]);
        assert!(t.ends_with(" fin"));
        std::fs::remove_dir_all(&dossier).unwrap();
    }
}
