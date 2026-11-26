//! Transfert de fichiers (règles communes §3.2) : parcourir le dossier personnel et en télécharger
//! un fichier, déposer un fichier envoyé par l'iPad dans ~/Downloads/web_bureau. Ni fichier caché,
//! ni lien symbolique, ni sortie du dossier ; écriture exclusive ; envoi coupé = fichier supprimé.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};

pub const ENVOI_MAX: u64 = 2 << 30; // 2 Go par fichier

/// Un nom de fichier venu de l'iPad, sans chemin ; None s'il est dangereux.
pub fn nom_sur(nom: &str) -> Option<String> {
    let nom = nom.trim();
    let ok = !nom.is_empty()
        && nom.len() <= 200
        && !nom.starts_with('.')
        && !nom.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
    ok.then(|| nom.to_string())
}

/// Le chemin `relatif` (« Documents/notes ») sous `racine`, s'il y existe sans rien de caché ni de
/// lien symbolique en route : le chemin canonique doit être exactement racine + relatif.
pub fn resoudre(racine: &Path, relatif: &str) -> Option<PathBuf> {
    let rel = Path::new(relatif.trim_matches('/'));
    let sur = rel
        .components()
        .all(|c| matches!(c, Component::Normal(s) if !s.to_string_lossy().starts_with('.') && !s.to_string_lossy().contains(':')));
    if !sur {
        return None;
    }
    let base = fs::canonicalize(racine).ok()?;
    let attendu = if rel.as_os_str().is_empty() { base.clone() } else { base.join(rel) };
    (fs::canonicalize(&attendu).ok()? == attendu).then_some(attendu)
}

/// Le contenu d'un dossier : dossiers d'abord, puis par nom ; ni caché ni lien symbolique.
pub fn lister(dossier: &Path) -> io::Result<Vec<Value>> {
    let mut v: Vec<(bool, String, Value)> = Vec::new();
    for e in fs::read_dir(dossier)?.flatten() {
        let nom = e.file_name().to_string_lossy().into_owned();
        let Ok(meta) = fs::symlink_metadata(e.path()) else { continue };
        if nom.starts_with('.') || meta.file_type().is_symlink() || !(meta.is_dir() || meta.is_file()) {
            continue;
        }
        let modifie = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let fiche = json!({"nom": nom, "dossier": meta.is_dir(), "taille": meta.len(), "modifie": modifie});
        v.push((!meta.is_dir(), nom.to_lowercase(), fiche));
    }
    v.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    Ok(v.into_iter().map(|(_, _, f)| f).collect())
}

/// « nom (2).ext », « nom (3).ext »… pour la n-ième tentative (n = 1 : le nom tel quel).
pub fn nom_numero(nom: &str, n: u32) -> String {
    if n <= 1 {
        return nom.to_string();
    }
    match nom.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => format!("{base} ({n}).{ext}"),
        _ => format!("{nom} ({n})"),
    }
}

#[derive(Debug)]
pub enum ErreurEnvoi {
    TropGros,
    Interrompu,
    Disque(io::Error),
}

/// Écrit les `taille` octets de `flux` dans un fichier NEUF de `dossier` (jamais d'écrasement :
/// « nom (2).ext » si le nom existe ; create_new refuse aussi un lien symbolique). Tout échec
/// supprime le fichier commencé.
pub fn recevoir(dossier: &Path, nom: &str, taille: u64, flux: &mut dyn Read) -> Result<PathBuf, ErreurEnvoi> {
    if taille > ENVOI_MAX {
        return Err(ErreurEnvoi::TropGros);
    }
    fs::create_dir_all(dossier).map_err(ErreurEnvoi::Disque)?;
    let (chemin, mut f) = (1..1000)
        .find_map(|n| {
            let p = dossier.join(nom_numero(nom, n));
            match fs::OpenOptions::new().write(true).create_new(true).open(&p) {
                Ok(f) => Some(Ok((p, f))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| Err(io::ErrorKind::AlreadyExists.into()))
        .map_err(ErreurEnvoi::Disque)?;
    let ecrit = (|| {
        let copie = io::copy(&mut (&mut *flux).take(taille), &mut f).map_err(|_| ErreurEnvoi::Interrompu)?;
        if copie < taille {
            return Err(ErreurEnvoi::Interrompu);
        }
        f.flush().and_then(|_| f.sync_all()).map_err(ErreurEnvoi::Disque)
    })();
    if let Err(e) = ecrit {
        drop(f);
        let _ = fs::remove_file(&chemin);
        return Err(e);
    }
    Ok(chemin)
}

#[cfg(test)]
mod essais {
    use super::*;

    fn tmp(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("web_bureau-fichiers-{}-{}", nom, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::canonicalize(d).unwrap()
    }

    #[test]
    fn noms() {
        assert_eq!(nom_sur(" photo.jpg "), Some("photo.jpg".into()));
        for mauvais in ["", ".cache", "../x", "a/b", "a\\b", "c:x", "a\nb", &"a".repeat(201)] {
            assert_eq!(nom_sur(mauvais), None, "{mauvais:?}");
        }
        assert_eq!(nom_numero("a.txt", 1), "a.txt");
        assert_eq!(nom_numero("a.tar.gz", 2), "a.tar (2).gz");
        assert_eq!(nom_numero("LISEZMOI", 3), "LISEZMOI (3)");
        assert_eq!(nom_numero(".x", 2), ".x (2)");
    }

    #[test]
    fn confinement() {
        let d = tmp("confinement");
        fs::create_dir_all(d.join("Documents/notes")).unwrap();
        fs::create_dir_all(d.join(".cache")).unwrap();
        fs::write(d.join("Documents/a.txt"), "a").unwrap();
        assert_eq!(resoudre(&d, ""), Some(d.clone()));
        assert_eq!(resoudre(&d, "Documents/notes"), Some(d.join("Documents/notes")));
        assert_eq!(resoudre(&d, "/Documents/a.txt/"), Some(d.join("Documents/a.txt")));
        for mauvais in ["..", "Documents/../..", ".cache", "Documents/.x", "absent", "c:x"] {
            assert_eq!(resoudre(&d, mauvais), None, "{mauvais}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", d.join("lien")).unwrap();
            std::os::unix::fs::symlink(d.join("Documents"), d.join("lien_dedans")).unwrap();
            assert_eq!(resoudre(&d, "lien"), None);
            assert_eq!(resoudre(&d, "lien_dedans/a.txt"), None);
            let noms: Vec<String> = lister(&d).unwrap().iter().map(|f| f["nom"].as_str().unwrap().to_string()).collect();
            assert_eq!(noms, vec!["Documents"]); // ni .cache ni liens
        }
    }

    #[test]
    fn envoi_exclusif_et_envoi_coupe() {
        let d = tmp("envoi");
        let p = recevoir(&d, "a.txt", 3, &mut &b"abc"[..]).unwrap();
        assert_eq!(p, d.join("a.txt"));
        let p2 = recevoir(&d, "a.txt", 3, &mut &b"def"[..]).unwrap();
        assert_eq!(p2, d.join("a (2).txt"));
        assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "abc"); // jamais écrasé
        // 10 octets annoncés, 4 reçus : rien ne reste
        assert!(matches!(recevoir(&d, "b.txt", 10, &mut &b"abcd"[..]), Err(ErreurEnvoi::Interrompu)));
        assert!(!d.join("b.txt").exists());
        assert!(matches!(recevoir(&d, "c.bin", ENVOI_MAX + 1, &mut &b""[..]), Err(ErreurEnvoi::TropGros)));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp/ailleurs", d.join("piege.txt")).unwrap();
            let p = recevoir(&d, "piege.txt", 1, &mut &b"x"[..]).unwrap();
            assert_eq!(p, d.join("piege (2).txt")); // le lien n'est pas suivi
        }
    }
}
