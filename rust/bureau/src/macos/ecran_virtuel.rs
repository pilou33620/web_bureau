//! Écran virtuel pour le Mac sans moniteur, aux proportions de l'iPad (CGVirtualDisplay, l'API
//! PRIVÉE de CoreGraphics qu'emploient BetterDisplay et DeskPad). Les classes sont cherchées par
//! leur nom à l'exécution : si une version de macOS les retire, l'outil le dit et capture
//! l'écran principal (repli : un bouchon HDMI factice).

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{class, msg_send};

use super::*;

pub struct EcranVirtuel {
    objet: Retained<AnyObject>,
    pub id: u32,
    pub largeur: u32,
    pub hauteur: u32,
    hz: u32,
}

// SAFETY: l'objet n'est touché que sur le fil principal (sur_fil_principal_attendre).
unsafe impl Send for EcranVirtuel {}

fn classe(nom: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(nom).ok_or_else(|| format!("{} absente de ce macOS (écran virtuel impossible)", nom.to_string_lossy()))
}

const PPP: f64 = 220.0; // densité annoncée à macOS (points par pouce, en pixels)

impl EcranVirtuel {
    /// Un écran de `largeur` × `hauteur` pixels, en HiDPI s'il est assez grand (le texte a alors la
    /// taille de celui de l'iPad).
    pub fn creer(largeur: u32, hauteur: u32) -> Result<EcranVirtuel, String> {
        sur_fil_principal_attendre(move || unsafe {
            let (c_desc, c_ecran) = (classe(c"CGVirtualDisplayDescriptor")?, classe(c"CGVirtualDisplay")?);
            let desc: Retained<AnyObject> = msg_send![c_desc, new];
            let _: () = msg_send![&*desc, setDispatchQueue: file_principale()];
            let _: () = msg_send![&*desc, setName: &*ns_chaine("web_bureau")];
            let _: () = msg_send![&*desc, setMaxPixelsWide: 7680u32];
            let _: () = msg_send![&*desc, setMaxPixelsHigh: 4320u32];
            let mm = CGSize { width: largeur as f64 * 25.4 / PPP, height: hauteur as f64 * 25.4 / PPP };
            let _: () = msg_send![&*desc, setSizeInMillimeters: mm];
            let _: () = msg_send![&*desc, setProductID: 0x5742u32];
            let _: () = msg_send![&*desc, setVendorID: 0x5742u32];
            let _: () = msg_send![&*desc, setSerialNum: 1u32];
            let a: Allocated<AnyObject> = msg_send![c_ecran, alloc];
            let objet: Option<Retained<AnyObject>> = msg_send![a, initWithDescriptor: &*desc];
            let objet = objet.ok_or("macOS a refusé l'écran virtuel")?;
            let id: u32 = msg_send![&*objet, displayID];
            let mut e = EcranVirtuel { objet, id, largeur: 0, hauteur: 0, hz: 0 };
            e.appliquer(largeur, hauteur, 60)?;
            Ok(e)
        })
    }

    /// Change la taille (un autre iPad, une autre orientation, une autre résolution) ou la fréquence.
    pub fn regler(&mut self, largeur: u32, hauteur: u32, hz: u32) -> Result<(), String> {
        if (largeur, hauteur, hz) == (self.largeur, self.hauteur, self.hz) {
            return Ok(());
        }
        sur_fil_principal_attendre(|| match self.appliquer(largeur, hauteur, hz) {
            // un macOS qui refuse 120 Hz ne coûte pas la session : elle tourne à 60
            Err(_) if hz > 60 => self.appliquer(largeur, hauteur, 60),
            r => r,
        })
    }

    fn appliquer(&mut self, largeur: u32, hauteur: u32, hz: u32) -> Result<(), String> {
        unsafe {
            // le mode est en points ; hiDPI double les pixels. Sous 1400 pixels de petit côté (720p,
            // 1080p), un point par pixel : en HiDPI le bureau serait trop petit pour macOS
            let hidpi = largeur.min(hauteur) >= 1400;
            let k = if hidpi { 2 } else { 1 };
            let (c_mode, c_reglages) = (classe(c"CGVirtualDisplayMode")?, classe(c"CGVirtualDisplaySettings")?);
            let a: Allocated<AnyObject> = msg_send![c_mode, alloc];
            let mode: Retained<AnyObject> =
                msg_send![a, initWithWidth: (largeur / k) as usize, height: (hauteur / k) as usize, refreshRate: hz as f64];
            let reglages: Retained<AnyObject> = msg_send![c_reglages, new];
            let _: () = msg_send![&*reglages, setHiDPI: hidpi as u32];
            let modes: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: &*mode];
            let _: () = msg_send![&*reglages, setModes: &*modes];
            let ok: bool = msg_send![&*self.objet, applySettings: &*reglages];
            if !ok {
                return Err(format!("mode {largeur}×{hauteur} à {hz} Hz refusé par l'écran virtuel"));
            }
        }
        self.largeur = largeur;
        self.hauteur = hauteur;
        self.hz = hz;
        Ok(())
    }
}
