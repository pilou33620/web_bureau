//! Le presse-papiers du Mac (texte seulement).

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

use super::*;

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSPasteboardTypeString: *const AnyObject;
}

fn general() -> Retained<AnyObject> {
    unsafe { msg_send![class!(NSPasteboard), generalPasteboard] }
}

/// Change à chaque copie sur le Mac.
pub fn compteur() -> isize {
    unsafe { msg_send![&*general(), changeCount] }
}

pub fn lire() -> Option<String> {
    let s: Option<Retained<AnyObject>> = unsafe { msg_send![&*general(), stringForType: &*NSPasteboardTypeString] };
    s.map(|s| de_ns_chaine(&s))
}

pub fn ecrire(texte: &str) {
    let pb = general();
    unsafe {
        let _: isize = msg_send![&*pb, clearContents];
        let _: bool = msg_send![&*pb, setString: &*ns_chaine(texte), forType: &*NSPasteboardTypeString];
    }
}
