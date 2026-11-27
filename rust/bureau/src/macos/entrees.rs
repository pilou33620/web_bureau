//! Souris et clavier simulés (CGEvent, permission Accessibilité). Tout ce qui est enfoncé est
//! suivi, pour tout relâcher à la fin d'une session : aucun ⌘ ne reste « collé » sur le Mac.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use super::*;
use crate::clavier;
use crate::protocole::Cible;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(etat: i32) -> *mut c_void;
    fn CGEventCreateMouseEvent(source: *mut c_void, genre: u32, position: CGPoint, bouton: u32) -> *mut c_void;
    fn CGEventCreateKeyboardEvent(source: *mut c_void, touche: u16, bas: bool) -> *mut c_void;
    fn CGEventCreateScrollWheelEvent2(source: *mut c_void, unite: u32, roues: u32, r1: i32, r2: i32, r3: i32) -> *mut c_void;
    fn CGEventKeyboardSetUnicodeString(e: *mut c_void, n: usize, texte: *const u16);
    fn CGEventSetFlags(e: *mut c_void, indicateurs: u64);
    fn CGEventSetIntegerValueField(e: *mut c_void, champ: u32, valeur: i64);
    fn CGEventSetDoubleValueField(e: *mut c_void, champ: u32, valeur: f64);
    fn CGEventPost(robinet: u32, e: *mut c_void);
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, cle: CFStringRef) -> CFTypeRef;
    fn UCKeyTranslate(
        disposition: *const u8,
        touche: u16,
        action: u16,
        modificateurs: u32,
        type_clavier: u32,
        options: u32,
        etat_mort: *mut u32,
        max: usize,
        longueur: *mut usize,
        chaine: *mut u16,
    ) -> OSStatus;
    fn LMGetKbdType() -> u8;
}

const HID: u32 = 0; // kCGHIDEventTap
const CLIC_DOUBLE: Duration = Duration::from_millis(500);

/// (appui, relâchement, glisser) par bouton : gauche, droit, milieu.
const TYPES: [(u32, u32, u32); 3] = [(1, 2, 6), (3, 4, 7), (25, 26, 27)];

pub struct Entrees {
    source: *mut c_void,
    ecran: u32,
    position: CGPoint,
    boutons: [bool; 3],
    indicateurs: u64,
    touches: BTreeSet<u16>,
    dernier_clic: Option<(Instant, CGPoint, u8, i64)>,
}

// SAFETY: la source d'événements CoreGraphics s'utilise d'un fil à la fois (Mutex de l'appelant).
unsafe impl Send for Entrees {}

fn poster(e: *mut c_void, indicateurs: u64) {
    if e.is_null() {
        return;
    }
    unsafe {
        CGEventSetFlags(e, indicateurs);
        CGEventPost(HID, e);
        CFRelease(e);
    }
}

impl Entrees {
    pub fn new(ecran: u32) -> Self {
        let source = unsafe { CGEventSourceCreate(1) }; // kCGEventSourceStateHIDSystemState
        Entrees { source, ecran, position: CGPoint::default(), boutons: [false; 3], indicateurs: 0, touches: BTreeSet::new(), dernier_clic: None }
    }

    pub fn regler_ecran(&mut self, ecran: u32) {
        self.ecran = ecran;
    }

    fn point(&self, x: f64, y: f64) -> CGPoint {
        let b = unsafe { CGDisplayBounds(self.ecran) };
        CGPoint { x: b.origin.x + x * (b.size.width - 1.0), y: b.origin.y + y * (b.size.height - 1.0) }
    }

    pub fn souris(&mut self, x: f64, y: f64) {
        let avant = self.position;
        self.position = self.point(x, y);
        let (genre, bouton) = match self.boutons.iter().position(|b| *b) {
            Some(i) => (TYPES[i].2, i as u32),
            None => (5, 0), // kCGEventMouseMoved
        };
        let e = unsafe { CGEventCreateMouseEvent(self.source, genre, self.position, bouton) };
        if !e.is_null() {
            // le déplacement, que macOS ne calcule pas pour un événement fabriqué : les jeux qui
            // tournent la caméra à la souris ne lisent que lui (comme Sunshine)
            unsafe {
                CGEventSetDoubleValueField(e, 4, self.position.x - avant.x); // kCGMouseEventDeltaX
                CGEventSetDoubleValueField(e, 5, self.position.y - avant.y); // kCGMouseEventDeltaY
            }
        }
        poster(e, self.indicateurs);
    }

    pub fn bouton(&mut self, b: u8, bas: bool) {
        let i = b.min(2) as usize;
        if self.boutons[i] == bas {
            return;
        }
        self.boutons[i] = bas;
        let maintenant = Instant::now();
        let compte = if bas {
            let proche = |p: CGPoint| (p.x - self.position.x).abs() < 5.0 && (p.y - self.position.y).abs() < 5.0;
            let n = match self.dernier_clic {
                Some((t, p, d, n)) if d == b && maintenant - t < CLIC_DOUBLE && proche(p) => n + 1,
                _ => 1,
            };
            self.dernier_clic = Some((maintenant, self.position, b, n));
            n
        } else {
            self.dernier_clic.map(|c| c.3).unwrap_or(1)
        };
        let genre = if bas { TYPES[i].0 } else { TYPES[i].1 };
        let e = unsafe { CGEventCreateMouseEvent(self.source, genre, self.position, i as u32) };
        if !e.is_null() {
            unsafe { CGEventSetIntegerValueField(e, 1, compte) }; // kCGMouseEventClickState : double clic
        }
        poster(e, self.indicateurs);
    }

    /// dx, dy : pixels dans le sens du navigateur (dy > 0 : le contenu monte).
    pub fn molette(&mut self, dx: f64, dy: f64) {
        let e = unsafe { CGEventCreateScrollWheelEvent2(self.source, 0, 2, -dy.round() as i32, -dx.round() as i32, 0) };
        poster(e, self.indicateurs);
    }

    pub fn touche(&mut self, kvk: u16, bas: bool) {
        let m = clavier::modificateur(kvk);
        if bas {
            self.indicateurs |= m;
            self.touches.insert(kvk);
        } else {
            self.indicateurs &= !m;
            self.touches.remove(&kvk);
        }
        poster(unsafe { CGEventCreateKeyboardEvent(self.source, kvk, bas) }, self.indicateurs);
    }

    /// Texte Unicode (clavier à l'écran) : indépendant de la disposition du Mac.
    pub fn texte(&mut self, s: &str) {
        for (i, ligne) in s.split('\n').enumerate() {
            if i > 0 {
                self.appuyer(clavier::KVK_ENTREE, 0);
            }
            let unites: Vec<u16> = ligne.encode_utf16().collect();
            for morceau in unites.chunks(16) {
                for bas in [true, false] {
                    let e = unsafe { CGEventCreateKeyboardEvent(self.source, 0, bas) };
                    if !e.is_null() {
                        unsafe { CGEventKeyboardSetUnicodeString(e, morceau.len(), morceau.as_ptr()) };
                    }
                    poster(e, 0);
                }
            }
        }
    }

    fn appuyer(&mut self, kvk: u16, indicateurs: u64) {
        poster(unsafe { CGEventCreateKeyboardEvent(self.source, kvk, true) }, indicateurs);
        poster(unsafe { CGEventCreateKeyboardEvent(self.source, kvk, false) }, indicateurs);
    }

    /// Une combinaison complète : modificateurs enfoncés, touche, modificateurs relâchés (le
    /// sélecteur d'applications de ⌘⇥ veut voir ⌘ enfoncé).
    pub fn raccourci(&mut self, indicateurs: u64, cible: Cible) {
        let kvk = match cible {
            Cible::Touche(k) => k,
            Cible::Caractere(c) => match code_pour(c) {
                Some(k) => k,
                None => return,
            },
        };
        let mods: Vec<(u64, u16)> =
            [(clavier::CTRL, 0x3B), (clavier::ALT, 0x3A), (clavier::MAJ, 0x38), (clavier::CMD, 0x37)].into_iter().filter(|(f, _)| indicateurs & f != 0).collect();
        let mut tenus = self.indicateurs;
        for (f, k) in &mods {
            tenus |= f;
            poster(unsafe { CGEventCreateKeyboardEvent(self.source, *k, true) }, tenus);
        }
        self.appuyer(kvk, tenus);
        for (f, k) in mods.iter().rev() {
            tenus &= !f;
            poster(unsafe { CGEventCreateKeyboardEvent(self.source, *k, false) }, tenus);
        }
    }

    /// Relâche tout (fin de session, iPad qui perd le focus).
    pub fn relacher(&mut self) {
        for kvk in std::mem::take(&mut self.touches) {
            self.indicateurs &= !clavier::modificateur(kvk);
            poster(unsafe { CGEventCreateKeyboardEvent(self.source, kvk, false) }, self.indicateurs);
        }
        for b in 0..3u8 {
            self.bouton(b, false);
        }
        self.indicateurs = 0;
    }

    /// Verrouille le Mac (⌃⌘Q, le raccourci du système).
    pub fn verrouiller(&mut self) {
        self.raccourci(clavier::CTRL | clavier::CMD, Cible::Caractere('q'));
    }

    /// Tape le mot de passe sur l'écran verrouillé, puis Entrée.
    pub fn deverrouiller(&mut self, mdp: &str) {
        // réveiller l'écran et faire paraître le champ
        self.appuyer(0x38, 0);
        std::thread::sleep(Duration::from_millis(800));
        self.texte(mdp);
        std::thread::sleep(Duration::from_millis(100));
        self.appuyer(clavier::KVK_ENTREE, 0);
    }
}

impl Drop for Entrees {
    fn drop(&mut self) {
        if !self.source.is_null() {
            unsafe { CFRelease(self.source) };
        }
    }
}

/// La touche qui donne `c` dans la disposition actuelle du Mac (« q » est sur la touche A en AZERTY).
pub fn code_pour(c: char) -> Option<u16> {
    let voulu = c.to_lowercase().next()?;
    let trouve = sur_fil_principal_attendre(move || unsafe {
        // les fonctions TIS exigent le fil principal
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return None;
        }
        let donnees = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        let r = (!donnees.is_null()).then(|| {
            let disposition = CFDataGetBytePtr(donnees);
            (0u16..128).find(|&k| {
                let (mut mort, mut n, mut sortie) = (0u32, 0usize, [0u16; 4]);
                let ok = UCKeyTranslate(disposition, k, 3, 0, LMGetKbdType() as u32, 1, &mut mort, 4, &mut n, sortie.as_mut_ptr()) == 0;
                ok && n > 0 && String::from_utf16_lossy(&sortie[..n]).chars().next() == Some(voulu)
            })
        });
        CFRelease(source);
        r.flatten()
    });
    // repli : la position QWERTY
    trouve.or_else(|| voulu.is_ascii_alphabetic().then(|| clavier::code_mac(&format!("Key{}", voulu.to_ascii_uppercase()))).flatten())
}
