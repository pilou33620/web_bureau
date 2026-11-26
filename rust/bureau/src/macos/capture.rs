//! Capture d'un écran par ScreenCaptureKit : chaque nouvelle image (NV12, à la taille demandée,
//! mise à l'échelle par le GPU) arrive dans `rappel`, sur une file série. Un écran immobile ne
//! produit pas d'image.

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObject, Sel};
use objc2::{class, define_class, msg_send, AllocAnyThread, ClassType, DefinedClass};

use super::*;

/// Reçoit l'image (CVPixelBufferRef, valable pendant l'appel) et son horodatage.
pub type Rappel = Box<dyn Fn(*mut c_void, CMTime) + Send + Sync>;

const NV12: u32 = 0x3432_3076; // kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange (« 420v »)

#[link(name = "ScreenCaptureKit", kind = "framework")]
extern "C" {
    static SCStreamFrameInfoStatus: *const c_void; // cité pour que l'éditeur de liens garde le framework
}

extern "C" {
    fn objc_msgSend();
    fn objc_getProtocol(nom: *const std::ffi::c_char) -> *mut c_void;
    fn class_addProtocol(classe: *const objc2::runtime::AnyClass, protocole: *mut c_void) -> bool;
}

define_class!(
    // SAFETY: NSObject n'impose rien à ses sous-classes ; la méthode suit SCStreamOutput.
    #[unsafe(super(NSObject))]
    #[name = "WebBureauSortieCapture"]
    #[ivars = Rappel]
    struct Sortie;

    impl Sortie {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn recu(&self, _flux: *mut AnyObject, tampon: *mut c_void, genre: isize) {
            if genre != 0 || tampon.is_null() {
                return; // 0 : SCStreamOutputTypeScreen
            }
            let image = unsafe { CMSampleBufferGetImageBuffer(tampon) };
            if image.is_null() {
                return; // image « inchangée » : rien à encoder
            }
            let pts = unsafe { CMSampleBufferGetPresentationTimeStamp(tampon) };
            let rappel = self.ivars();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rappel(image, pts)));
        }
    }
);

impl Sortie {
    fn nouvelle(rappel: Rappel) -> Retained<Self> {
        let this = Self::alloc().set_ivars(rappel);
        unsafe { msg_send![super(this), init] }
    }
}

/// Un objet Objective-C passé d'un fil à l'autre (rappels de ScreenCaptureKit).
struct Envoyable(Retained<AnyObject>);
// SAFETY: SCShareableContent est immuable une fois rendu.
unsafe impl Send for Envoyable {}

fn contenu_partageable() -> Result<Retained<AnyObject>, String> {
    let (tx, rx) = mpsc::channel::<Result<Envoyable, String>>();
    let bloc = RcBlock::new(move |contenu: *mut AnyObject, erreur: *mut AnyObject| {
        let r = match unsafe { Retained::retain(contenu) } {
            Some(c) => Ok(Envoyable(c)),
            None => Err(texte_erreur(erreur)),
        };
        let _ = tx.send(r);
    });
    let _: () = unsafe { msg_send![class!(SCShareableContent), getShareableContentWithCompletionHandler: &*bloc] };
    rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "ScreenCaptureKit ne répond pas".to_string())?.map(|e| e.0)
}

fn attendre_fin(f: impl FnOnce(&block2::Block<dyn Fn(*mut AnyObject)>), delai: Duration) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let bloc = RcBlock::new(move |erreur: *mut AnyObject| {
        let _ = tx.send(if erreur.is_null() { Ok(()) } else { Err(texte_erreur(erreur)) });
    });
    f(&bloc);
    rx.recv_timeout(delai).map_err(|_| "ScreenCaptureKit ne répond pas".to_string())?
}

pub struct Capture {
    flux: Retained<AnyObject>,
    _sortie: Retained<Sortie>,
}

// SAFETY: SCStream s'utilise depuis n'importe quel fil (start/stop sont asynchrones).
unsafe impl Send for Capture {}

impl Capture {
    /// Capture l'écran `ecran` (CGDirectDisplayID) à `largeur` × `hauteur` pixels, 60 i/s au plus.
    pub fn demarrer(ecran: u32, largeur: usize, hauteur: usize, rappel: Rappel) -> Result<Capture, String> {
        let _ = unsafe { SCStreamFrameInfoStatus };
        // un écran virtuel tout juste créé met un instant à apparaître
        let mut ecran_sc = None;
        for _ in 0..10 {
            let contenu = contenu_partageable()?;
            let ecrans: Retained<AnyObject> = unsafe { msg_send![&*contenu, displays] };
            let n: usize = unsafe { msg_send![&*ecrans, count] };
            for i in 0..n {
                let e: Retained<AnyObject> = unsafe { msg_send![&*ecrans, objectAtIndex: i] };
                let id: u32 = unsafe { msg_send![&*e, displayID] };
                if id == ecran {
                    ecran_sc = Some(e);
                }
            }
            if ecran_sc.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        let ecran_sc = ecran_sc.ok_or("écran introuvable pour la capture")?;
        unsafe {
            let vide: Retained<AnyObject> = msg_send![class!(NSArray), array];
            let filtre: Allocated<AnyObject> = msg_send![class!(SCContentFilter), alloc];
            let filtre: Retained<AnyObject> = msg_send![filtre, initWithDisplay: &*ecran_sc, excludingWindows: &*vide];
            let cfg: Retained<AnyObject> = msg_send![class!(SCStreamConfiguration), new];
            let _: () = msg_send![&*cfg, setWidth: largeur];
            let _: () = msg_send![&*cfg, setHeight: hauteur];
            let _: () = msg_send![&*cfg, setPixelFormat: NV12];
            let _: () = msg_send![&*cfg, setMinimumFrameInterval: CMTime::new(1, 60)];
            let _: () = msg_send![&*cfg, setShowsCursor: true];
            let _: () = msg_send![&*cfg, setQueueDepth: 3isize];
            let flux: Allocated<AnyObject> = msg_send![class!(SCStream), alloc];
            let flux: Retained<AnyObject> =
                msg_send![flux, initWithFilter: &*filtre, configuration: &*cfg, delegate: std::ptr::null_mut::<AnyObject>()];

            let protocole = objc_getProtocol(c"SCStreamOutput".as_ptr());
            if !protocole.is_null() {
                class_addProtocol(Sortie::class(), protocole);
            }
            let sortie = Sortie::nouvelle(rappel);
            // addStreamOutput:type:… : « type » est un mot réservé de Rust, d'où l'appel direct
            type Ajout = unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, isize, *mut AnyObject, *mut *mut AnyObject) -> bool;
            let ajouter: Ajout = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let mut erreur: *mut AnyObject = std::ptr::null_mut();
            let ok = ajouter(
                Retained::as_ptr(&flux) as *mut AnyObject,
                Sel::register(c"addStreamOutput:type:sampleHandlerQueue:error:"),
                Retained::as_ptr(&sortie) as *mut AnyObject,
                0,
                file_serie(c"web_bureau.capture"),
                &mut erreur,
            );
            if !ok {
                return Err(format!("capture refusée : {}", texte_erreur(erreur)));
            }
            attendre_fin(|b| { let _: () = msg_send![&*flux, startCaptureWithCompletionHandler: b]; }, Duration::from_secs(10))
                .map_err(|e| format!("capture refusée (permission « Enregistrement de l'écran » ?) : {e}"))?;
            Ok(Capture { flux, _sortie: sortie })
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let flux = &self.flux;
        let _ = attendre_fin(|b| unsafe { let _: () = msg_send![&**flux, stopCaptureWithCompletionHandler: b]; }, Duration::from_secs(3));
    }
}
