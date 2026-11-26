//! L'icône de la barre des menus : elle montre que le Mac est contrôlé, et son menu coupe la
//! session. AppKit veut le fil principal : `lancer` y tourne pour toujours (boucle NSApplication).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, define_class, msg_send, sel, AllocAnyThread};

use super::*;

static ELEMENT: AtomicUsize = AtomicUsize::new(0); // le NSStatusItem, gardé pour toujours
static COUPER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

define_class!(
    // SAFETY: NSObject n'impose rien à ses sous-classes.
    #[unsafe(super(NSObject))]
    #[name = "WebBureauCible"]
    struct Cible;

    impl Cible {
        #[unsafe(method(couper:))]
        fn couper(&self, _expediteur: *mut AnyObject) {
            if let Some(f) = COUPER.get() {
                f();
            }
        }
    }
);

/// Le texte de l'icône (depuis n'importe quel fil).
pub fn afficher(texte: String) {
    sur_fil_principal(move || {
        let p = ELEMENT.load(Ordering::SeqCst) as *mut AnyObject;
        if p.is_null() {
            return;
        }
        unsafe {
            let bouton: Option<Retained<AnyObject>> = msg_send![p, button];
            if let Some(b) = bouton {
                let _: () = msg_send![&*b, setTitle: &*ns_chaine(&texte)];
            }
        }
    });
}

/// Pose l'icône puis fait tourner AppKit sur le fil principal ; ne rend jamais la main.
pub fn lancer(couper: impl Fn() + Send + Sync + 'static) -> ! {
    let _ = COUPER.set(Box::new(couper));
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let _: bool = msg_send![&*app, setActivationPolicy: 1isize]; // Accessory : ni Dock ni menu
        let barre: Retained<AnyObject> = msg_send![class!(NSStatusBar), systemStatusBar];
        let element: Retained<AnyObject> = msg_send![&*barre, statusItemWithLength: -1.0f64];
        let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
        let cible: Retained<Cible> = msg_send![super(Cible::alloc().set_ivars(())), init];
        let a: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
        let item: Retained<AnyObject> =
            msg_send![a, initWithTitle: &*ns_chaine("Couper la session"), action: sel!(couper:), keyEquivalent: &*ns_chaine("")];
        let _: () = msg_send![&*item, setTarget: &*cible];
        let _: () = msg_send![&*menu, addItem: &*item];
        let _: () = msg_send![&*element, setMenu: &*menu];
        std::mem::forget(cible); // la cible d'un menu n'est pas retenue par lui
        ELEMENT.store(Retained::into_raw(element) as usize, Ordering::SeqCst);
        afficher("web_bureau".into());
        let _: () = msg_send![&*app, run];
    }
    std::process::exit(0)
}
