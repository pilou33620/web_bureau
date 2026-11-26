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
}

// SAFETY: l'objet n'est touché que sur le fil principal (sur_fil_principal_attendre).
unsafe impl Send for EcranVirtuel {}

fn classe(nom: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(nom).ok_or_else(|| format!("{} absente de ce macOS (écran virtuel impossible)", nom.to_string_lossy()))
}

const PPP: f64 = 220.0; // densité annoncée à macOS (points par pouce, en pixels)

impl EcranVirtuel {
    /// Un écran de `largeur` × `hauteur` pixels, en HiDPI (le texte a la taille de celui de l'iPad).
    pub fn creer(largeur: u32, hauteur: u32) -> Result<EcranVirtuel, String> {
        sur_fil_principal_attendre(move || unsafe {
            let desc: Retained<AnyObject> = msg_send![classe(c"CGVirtualDisplayDescriptor")?, new];
            let _: () = msg_send![&*desc, setDispatchQueue: file_principale()];
            let _: () = msg_send![&*desc, setName: &*ns_chaine("web_bureau")];
            let _: () = msg_send![&*desc, setMaxPixelsWide: 7680u32];
            let _: () = msg_send![&*desc, setMaxPixelsHigh: 4320u32];
            let mm = CGSize { width: largeur as f64 * 25.4 / PPP, height: hauteur as f64 * 25.4 / PPP };
            let _: () = msg_send![&*desc, setSizeInMillimeters: mm];
            let _: () = msg_send![&*desc, setProductID: 0x5742u32];
            let _: () = msg_send![&*desc, setVendorID: 0x5742u32];
            let _: () = msg_send![&*desc, setSerialNum: 1u32];
            let a: Allocated<AnyObject> = msg_send![classe(c"CGVirtualDisplay")?, alloc];
            let objet: Option<Retained<AnyObject>> = msg_send![a, initWithDescriptor: &*desc];
            let objet = objet.ok_or("macOS a refusé l'écran virtuel")?;
            let id: u32 = msg_send![&*objet, displayID];
            let mut e = EcranVirtuel { objet, id, largeur: 0, hauteur: 0 };
            e.appliquer(largeur, hauteur)?;
            Ok(e)
        })
    }

    /// Change la taille (un autre iPad, une autre orientation).
    pub fn regler(&mut self, largeur: u32, hauteur: u32) -> Result<(), String> {
        if (largeur, hauteur) == (self.largeur, self.hauteur) {
            return Ok(());
        }
        sur_fil_principal_attendre(|| self.appliquer(largeur, hauteur))
    }

    fn appliquer(&mut self, largeur: u32, hauteur: u32) -> Result<(), String> {
        unsafe {
            // le mode est en points ; hiDPI double les pixels
            let a: Allocated<AnyObject> = msg_send![classe(c"CGVirtualDisplayMode")?, alloc];
            let mode: Retained<AnyObject> =
                msg_send![a, initWithWidth: (largeur / 2) as usize, height: (hauteur / 2) as usize, refreshRate: 60.0f64];
            let reglages: Retained<AnyObject> = msg_send![classe(c"CGVirtualDisplaySettings")?, new];
            let _: () = msg_send![&*reglages, setHiDPI: 1u32];
            let modes: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: &*mode];
            let _: () = msg_send![&*reglages, setModes: &*modes];
            let ok: bool = msg_send![&*self.objet, applySettings: &*reglages];
            if !ok {
                return Err(format!("mode {largeur}×{hauteur} refusé par l'écran virtuel"));
            }
        }
        self.largeur = largeur;
        self.hauteur = hauteur;
        Ok(())
    }
}
