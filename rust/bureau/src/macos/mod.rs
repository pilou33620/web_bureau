//! Les API d'Apple : le SEUL module où `unsafe` est permis (règle validée pour web_bureau).
//! Fonctions C (CoreFoundation, CoreGraphics, CoreMedia, VideoToolbox, IOKit) déclarées à la
//! main d'après les en-têtes du SDK ; classes Objective-C par objc2 (`msg_send!`).
#![allow(unsafe_code)]

pub mod barre_menus;
pub mod capture;
pub mod ecran_virtuel;
pub mod encodage;
pub mod entrees;
pub mod presse_papiers;

use std::ffi::{c_char, c_void, CStr, CString};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, Encode, Encoding};

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type OSStatus = i32;

/// Un objet C dont seule l'adresse compte (constantes de rappels, file principale).
#[repr(C)]
pub struct Opaque {
    _p: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CMTime {
    pub value: i64,
    pub timescale: i32,
    pub flags: u32,
    pub epoch: i64,
}

// SAFETY: même disposition que CMTime (CMTime.h), encodage « {?=qiIq} ».
unsafe impl Encode for CMTime {
    const ENCODING: Encoding = Encoding::Struct("?", &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING]);
}

impl CMTime {
    pub fn new(value: i64, timescale: i32) -> Self {
        CMTime { value, timescale, flags: 1, epoch: 0 } // kCMTimeFlags_Valid
    }
    pub const INVALIDE: CMTime = CMTime { value: 0, timescale: 0, flags: 0, epoch: 0 };
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

// SAFETY: même disposition que CGSize (CGGeometry.h).
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFTypeDictionaryKeyCallBacks: Opaque;
    pub static kCFTypeDictionaryValueCallBacks: Opaque;
    pub fn CFRelease(o: CFTypeRef);
    pub fn CFNumberCreate(alloc: *const c_void, genre: isize, valeur: *const c_void) -> CFTypeRef;
    pub fn CFDictionaryCreate(
        alloc: *const c_void,
        cles: *const CFTypeRef,
        valeurs: *const CFTypeRef,
        n: isize,
        rappels_cles: *const Opaque,
        rappels_valeurs: *const Opaque,
    ) -> CFTypeRef;
    pub fn CFDictionaryGetValue(d: CFTypeRef, cle: CFTypeRef) -> CFTypeRef;
    pub fn CFStringCreateWithBytes(alloc: *const c_void, octets: *const u8, n: isize, encodage: u32, bom: u8) -> CFStringRef;
    pub fn CFArrayGetCount(a: CFTypeRef) -> isize;
    pub fn CFArrayGetValueAtIndex(a: CFTypeRef, i: isize) -> CFTypeRef;
    pub fn CFBooleanGetValue(b: CFTypeRef) -> u8;
    pub fn CFDataGetLength(d: CFTypeRef) -> isize;
    pub fn CFDataGetBytePtr(d: CFTypeRef) -> *const u8;
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    pub static kCMSampleAttachmentKey_NotSync: CFStringRef;
    pub static kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms: CFStringRef;
    pub fn CMSampleBufferGetImageBuffer(sb: *mut c_void) -> *mut c_void;
    pub fn CMSampleBufferGetPresentationTimeStamp(sb: *mut c_void) -> CMTime;
    pub fn CMSampleBufferGetDataBuffer(sb: *mut c_void) -> *mut c_void;
    pub fn CMSampleBufferGetSampleAttachmentsArray(sb: *mut c_void, creer: u8) -> CFTypeRef;
    pub fn CMSampleBufferGetFormatDescription(sb: *mut c_void) -> CFTypeRef;
    pub fn CMFormatDescriptionGetExtension(desc: CFTypeRef, cle: CFStringRef) -> CFTypeRef;
    pub fn CMBlockBufferGetDataLength(b: *mut c_void) -> usize;
    pub fn CMBlockBufferCopyDataBytes(b: *mut c_void, decalage: usize, n: usize, dest: *mut c_void) -> OSStatus;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    pub static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
    pub fn CVPixelBufferCreate(alloc: *const c_void, l: usize, h: usize, format: u32, attributs: CFTypeRef, sortie: *mut *mut c_void) -> i32;
    pub fn CVPixelBufferRelease(b: *mut c_void);
    fn CVPixelBufferRetain(b: *mut c_void) -> *mut c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
    pub fn CGMainDisplayID() -> u32;
    pub fn CGDisplayBounds(ecran: u32) -> CGRect;
    fn CGDisplayCopyDisplayMode(ecran: u32) -> *mut c_void;
    fn CGDisplayModeGetPixelWidth(mode: *mut c_void) -> usize;
    fn CGDisplayModeGetPixelHeight(mode: *mut c_void) -> usize;
    fn CGDisplayModeRelease(mode: *mut c_void);
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMAssertionCreateWithName(genre: CFStringRef, niveau: u32, nom: CFStringRef, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
    fn IOPMAssertionDeclareUserActivity(nom: CFStringRef, genre: u32, id: *mut u32) -> i32;
}

extern "C" {
    static _dispatch_main_q: Opaque;
    fn dispatch_queue_create(nom: *const c_char, attributs: *const c_void) -> *mut c_void;
    fn dispatch_async_f(file: *const c_void, ctx: *mut c_void, f: extern "C" fn(*mut c_void));
    fn dispatch_sync_f(file: *const c_void, ctx: *mut c_void, f: extern "C" fn(*mut c_void));
    fn pthread_main_np() -> i32;
}

/// Un objet CoreFoundation, libéré à la fin de sa portée.
pub struct Cf(pub CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

pub fn cf_chaine(s: &str) -> Cf {
    Cf(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0) })
}

pub fn cf_entier(n: i32) -> Cf {
    Cf(unsafe { CFNumberCreate(std::ptr::null(), 3, &n as *const i32 as *const c_void) }) // kCFNumberSInt32Type
}

/// Un dictionnaire CoreFoundation (les clés et valeurs sont retenues par lui).
pub fn cf_dico(paires: &[(CFTypeRef, CFTypeRef)]) -> Cf {
    let cles: Vec<CFTypeRef> = paires.iter().map(|p| p.0).collect();
    let valeurs: Vec<CFTypeRef> = paires.iter().map(|p| p.1).collect();
    Cf(unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            cles.as_ptr(),
            valeurs.as_ptr(),
            paires.len() as isize,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    })
}

/// Les octets d'un CFData.
pub fn cf_octets(d: CFTypeRef) -> Vec<u8> {
    if d.is_null() {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(d), CFDataGetLength(d) as usize).to_vec() }
}

pub fn vrai() -> CFTypeRef {
    unsafe { kCFBooleanTrue }
}

pub fn ns_chaine(s: &str) -> Retained<AnyObject> {
    let c = CString::new(s.replace('\0', "")).unwrap_or_default();
    unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
}

pub fn de_ns_chaine(o: &AnyObject) -> String {
    let p: *const c_char = unsafe { msg_send![o, UTF8String] };
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// La description d'une NSError (rappels de ScreenCaptureKit).
pub fn texte_erreur(erreur: *mut AnyObject) -> String {
    if erreur.is_null() {
        return "erreur inconnue".into();
    }
    let d: Option<Retained<AnyObject>> = unsafe { msg_send![erreur, localizedDescription] };
    d.map(|d| de_ns_chaine(&d)).unwrap_or_default()
}

/// La file principale (dispatch_get_main_queue), telle que les API Objective-C l'attendent.
pub fn file_principale() -> *mut AnyObject {
    unsafe { &_dispatch_main_q as *const Opaque as *mut AnyObject }
}

pub fn file_serie(nom: &CStr) -> *mut AnyObject {
    unsafe { dispatch_queue_create(nom.as_ptr(), std::ptr::null()) as *mut AnyObject }
}

extern "C" fn executer(ctx: *mut c_void) {
    let f = unsafe { Box::from_raw(ctx as *mut Box<dyn FnOnce() + Send>) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
}

/// Exécute `f` plus tard sur le fil principal (AppKit l'exige pour la barre des menus).
pub fn sur_fil_principal(f: impl FnOnce() + Send + 'static) {
    let b: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(f));
    unsafe { dispatch_async_f(file_principale() as *const c_void, Box::into_raw(b) as *mut c_void, executer) }
}

struct Appel<F, R> {
    f: Option<F>,
    r: Option<std::thread::Result<R>>,
}

extern "C" fn appeler<F: FnOnce() -> R, R>(ctx: *mut c_void) {
    let a = unsafe { &mut *(ctx as *mut Appel<F, R>) };
    let f = a.f.take().unwrap();
    a.r = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)));
}

/// Exécute `f` sur le fil principal et attend son résultat (écran virtuel, disposition du clavier).
pub fn sur_fil_principal_attendre<F: FnOnce() -> R + Send, R: Send>(f: F) -> R {
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }
    let mut a = Appel { f: Some(f), r: None };
    unsafe { dispatch_sync_f(file_principale() as *const c_void, &mut a as *mut Appel<F, R> as *mut c_void, appeler::<F, R>) };
    match a.r.take().unwrap() {
        Ok(r) => r,
        Err(p) => std::panic::resume_unwind(p),
    }
}

// -- permissions, verrouillage, énergie -----------------------------------------

/// Enregistrement de l'écran ; `demander` affiche la demande de macOS si elle manque.
pub fn permission_ecran(demander: bool) -> bool {
    unsafe { CGPreflightScreenCaptureAccess() || (demander && CGRequestScreenCaptureAccess()) }
}

/// Accessibilité (souris et clavier simulés) ; `demander` ouvre la demande de macOS.
pub fn permission_accessibilite(demander: bool) -> bool {
    let options = if demander { cf_dico(&[(unsafe { kAXTrustedCheckOptionPrompt }, vrai())]) } else { Cf(std::ptr::null()) };
    unsafe { AXIsProcessTrustedWithOptions(options.0) != 0 }
}

/// La session est-elle verrouillée (écran de verrouillage affiché) ?
pub fn ecran_verrouille() -> bool {
    let d = Cf(unsafe { CGSessionCopyCurrentDictionary() });
    if d.0.is_null() {
        return false;
    }
    let cle = cf_chaine("CGSSessionScreenIsLocked");
    let v = unsafe { CFDictionaryGetValue(d.0, cle.0) };
    !v.is_null() && unsafe { CFBooleanGetValue(v) } != 0
}

pub fn ecran_principal() -> u32 {
    unsafe { CGMainDisplayID() }
}

/// Taille réelle en pixels d'un écran (mode HiDPI compris).
pub fn pixels_ecran(ecran: u32) -> (usize, usize) {
    unsafe {
        let mode = CGDisplayCopyDisplayMode(ecran);
        if mode.is_null() {
            return (0, 0);
        }
        let t = (CGDisplayModeGetPixelWidth(mode), CGDisplayModeGetPixelHeight(mode));
        CGDisplayModeRelease(mode);
        t
    }
}

/// Une assertion d'énergie (« PreventUserIdleSystemSleep », « PreventUserIdleDisplaySleep »),
/// relâchée à la fin de sa portée.
pub struct Assertion(u32);

impl Drop for Assertion {
    fn drop(&mut self) {
        unsafe { IOPMAssertionRelease(self.0) };
    }
}

/// Rallume l'écran s'il s'est éteint (comme un geste de l'utilisateur) : un écran éteint n'est pas capturé.
pub fn reveiller_ecran() {
    let nom = cf_chaine("web_bureau");
    let mut id = 0u32;
    unsafe { IOPMAssertionDeclareUserActivity(nom.0, 0, &mut id) }; // kIOPMUserActiveLocal
}

/// Une image de capture gardée au-delà de son rappel (CVPixelBuffer retenu), rendue à la fin de sa portée.
pub struct Image(*mut c_void);

// SAFETY: un CVPixelBuffer retenu se lit depuis n'importe quel fil.
unsafe impl Send for Image {}

impl Image {
    pub fn retenir(tampon: *mut c_void) -> Image {
        Image(unsafe { CVPixelBufferRetain(tampon) })
    }
    pub fn tampon(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { CVPixelBufferRelease(self.0) }
    }
}

pub fn empecher(genre: &str) -> Option<Assertion> {
    let (g, nom) = (cf_chaine(genre), cf_chaine("web_bureau"));
    let mut id = 0u32;
    (unsafe { IOPMAssertionCreateWithName(g.0, 255, nom.0, &mut id) } == 0).then_some(Assertion(id)) // kIOPMAssertionLevelOn
}
