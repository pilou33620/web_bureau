//! Touches : `KeyboardEvent.code` (la POSITION de la touche sur le clavier de l'iPad) vers le code
//! virtuel macOS (kVK_*, Events.h). Le Mac applique ensuite sa propre disposition : AZERTY des
//! deux côtés, on obtient ce qui est gravé sur la touche. Table fermée : un code absent est refusé.

pub const MAJ: u64 = 0x0002_0000; // kCGEventFlagMaskShift
pub const CTRL: u64 = 0x0004_0000; // kCGEventFlagMaskControl
pub const ALT: u64 = 0x0008_0000; // kCGEventFlagMaskAlternate
pub const CMD: u64 = 0x0010_0000; // kCGEventFlagMaskCommand
pub const FN: u64 = 0x0080_0000; // kCGEventFlagMaskSecondaryFn

pub const KVK_ENTREE: u16 = 0x24;

// shortcut: sur un clavier ISO, macOS échange 0x0A et 0x32 (touche à gauche de 1 / touche
// à côté de Maj gauche) ; si « @ » et « < » sont inversés sur le Mac, échanger ces deux lignes.
const TABLE: &[(&str, u16)] = &[
    ("KeyA", 0x00), ("KeyS", 0x01), ("KeyD", 0x02), ("KeyF", 0x03), ("KeyH", 0x04), ("KeyG", 0x05),
    ("KeyZ", 0x06), ("KeyX", 0x07), ("KeyC", 0x08), ("KeyV", 0x09), ("IntlBackslash", 0x0A),
    ("KeyB", 0x0B), ("KeyQ", 0x0C), ("KeyW", 0x0D), ("KeyE", 0x0E), ("KeyR", 0x0F), ("KeyY", 0x10),
    ("KeyT", 0x11), ("Digit1", 0x12), ("Digit2", 0x13), ("Digit3", 0x14), ("Digit4", 0x15),
    ("Digit6", 0x16), ("Digit5", 0x17), ("Equal", 0x18), ("Digit9", 0x19), ("Digit7", 0x1A),
    ("Minus", 0x1B), ("Digit8", 0x1C), ("Digit0", 0x1D), ("BracketRight", 0x1E), ("KeyO", 0x1F),
    ("KeyU", 0x20), ("BracketLeft", 0x21), ("KeyI", 0x22), ("KeyP", 0x23), ("Enter", 0x24),
    ("KeyL", 0x25), ("KeyJ", 0x26), ("Quote", 0x27), ("KeyK", 0x28), ("Semicolon", 0x29),
    ("Backslash", 0x2A), ("Comma", 0x2B), ("Slash", 0x2C), ("KeyN", 0x2D), ("KeyM", 0x2E),
    ("Period", 0x2F), ("Tab", 0x30), ("Space", 0x31), ("Backquote", 0x32), ("Backspace", 0x33),
    ("Escape", 0x35), ("MetaRight", 0x36), ("MetaLeft", 0x37), ("ShiftLeft", 0x38),
    ("CapsLock", 0x39), ("AltLeft", 0x3A), ("ControlLeft", 0x3B), ("ShiftRight", 0x3C),
    ("AltRight", 0x3D), ("ControlRight", 0x3E), ("Fn", 0x3F), ("F17", 0x40),
    ("NumpadDecimal", 0x41), ("NumpadMultiply", 0x43), ("NumpadAdd", 0x45), ("NumLock", 0x47),
    ("AudioVolumeUp", 0x48), ("AudioVolumeDown", 0x49), ("AudioVolumeMute", 0x4A),
    ("NumpadDivide", 0x4B), ("NumpadEnter", 0x4C), ("NumpadSubtract", 0x4E), ("F18", 0x4F),
    ("F19", 0x50), ("NumpadEqual", 0x51), ("Numpad0", 0x52), ("Numpad1", 0x53), ("Numpad2", 0x54),
    ("Numpad3", 0x55), ("Numpad4", 0x56), ("Numpad5", 0x57), ("Numpad6", 0x58), ("Numpad7", 0x59),
    ("F20", 0x5A), ("Numpad8", 0x5B), ("Numpad9", 0x5C), ("IntlYen", 0x5D), ("IntlRo", 0x5E),
    ("NumpadComma", 0x5F), ("F5", 0x60), ("F6", 0x61), ("F7", 0x62), ("F3", 0x63), ("F8", 0x64),
    ("F9", 0x65), ("Lang2", 0x66), ("F11", 0x67), ("Lang1", 0x68), ("F13", 0x69), ("F16", 0x6A),
    ("F14", 0x6B), ("F10", 0x6D), ("ContextMenu", 0x6E), ("F12", 0x6F), ("F15", 0x71),
    ("Insert", 0x72), ("Help", 0x72), ("Home", 0x73), ("PageUp", 0x74), ("Delete", 0x75),
    ("F4", 0x76), ("End", 0x77), ("F2", 0x78), ("PageDown", 0x79), ("F1", 0x7A),
    ("ArrowLeft", 0x7B), ("ArrowRight", 0x7C), ("ArrowDown", 0x7D), ("ArrowUp", 0x7E),
];

/// Le code virtuel macOS d'une position de touche du navigateur.
pub fn code_mac(code: &str) -> Option<u16> {
    TABLE.iter().find(|(c, _)| *c == code).map(|(_, k)| *k)
}

/// Les positions acceptées : la page n'envoie que celles-là (une autre fermerait la session).
pub fn codes() -> Vec<&'static str> {
    TABLE.iter().map(|(c, _)| *c).collect()
}

/// L'indicateur (CGEventFlags) que tient une touche de modification, 0 sinon.
pub fn modificateur(kvk: u16) -> u64 {
    match kvk {
        0x37 | 0x36 => CMD,
        0x38 | 0x3C => MAJ,
        0x3A | 0x3D => ALT,
        0x3B | 0x3E => CTRL,
        0x3F => FN,
        _ => 0,
    }
}

/// Les indicateurs d'une liste de noms (« cmd », « ctrl », « alt », « maj ») ; None si un nom est inconnu.
pub fn modificateurs(noms: &[&str]) -> Option<u64> {
    noms.iter().try_fold(0, |f, n| {
        Some(
            f | match *n {
                "cmd" => CMD,
                "ctrl" => CTRL,
                "alt" => ALT,
                "maj" => MAJ,
                _ => return None,
            },
        )
    })
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn table() {
        assert_eq!(code_mac("KeyA"), Some(0x00));
        assert_eq!(code_mac("ArrowUp"), Some(0x7E));
        assert_eq!(code_mac("Enter"), Some(KVK_ENTREE));
        assert_eq!(code_mac("inconnu"), None);
        assert_eq!(code_mac(""), None);
        // aucune position en double (sauf Insert/Help, la même touche sur Mac)
        for (i, (c, k)) in TABLE.iter().enumerate() {
            for (c2, k2) in &TABLE[i + 1..] {
                assert!(c != c2, "{c}");
                assert!(k != k2 || (*c == "Insert" && *c2 == "Help"), "{c} {c2}");
            }
        }
    }

    #[test]
    fn indicateurs() {
        assert_eq!(modificateur(0x37), CMD);
        assert_eq!(modificateur(0x00), 0);
        assert_eq!(modificateurs(&["cmd", "maj"]), Some(CMD | MAJ));
        assert_eq!(modificateurs(&[]), Some(0));
        assert_eq!(modificateurs(&["cmd", "super"]), None);
    }
}
