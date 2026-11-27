//! web_bureau : prendre le contrôle du Mac depuis l'iPad, dans Safari, sur le réseau local.
//!
//! Le Mac capture son écran (ScreenCaptureKit), l'encode avec sa puce (VideoToolbox, H.264 ou
//! HEVC) et l'envoie par un WebSocket chiffré, ou par UDP (WebTransport) si Safari le permet ;
//! Safari le décode (WebCodecs) et renvoie souris, clavier et gestes, rejoués sur le Mac (CGEvent).
//!
//! Ce qui ne dépend pas de macOS (et se teste partout) est ici ; le reste est dans `macos/`,
//! seul endroit où `unsafe` est permis (appels aux API d'Apple).
#![deny(unsafe_code)]

pub mod clavier;
pub mod commun;
pub mod fichiers;
pub mod maj;
pub mod protocole;
pub mod regulation;
pub mod udp;
pub mod video;
pub mod ws;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub mod serveur;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
