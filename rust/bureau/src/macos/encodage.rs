//! Encodage par la puce du Mac (VideoToolbox) : H.264 ou HEVC, temps réel, faible latence, sans
//! images B, image clé seulement à la demande. Chaque image encodée (AVCC, longueurs sur 4
//! octets, ce que WebCodecs attend avec une description) arrive dans le rappel.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use super::*;
use crate::protocole::Codec;

pub struct ImageEncodee {
    pub cle: bool,
    pub donnees: Vec<u8>,
    /// avcC / hvcC, sur les images clés.
    pub description: Option<Vec<u8>>,
}

/// Reçoit le numéro donné à `encoder` et l'image (None : l'encodeur l'a sautée).
pub type RappelEncodeur = Box<dyn Fn(u32, Option<ImageEncodee>) + Send + Sync>;

type RappelVT = extern "C" fn(*mut c_void, *mut c_void, OSStatus, u32, *mut c_void);

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    static kVTVideoEncoderSpecification_EnableLowLatencyRateControl: CFStringRef;
    static kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: CFStringRef;
    static kVTCompressionPropertyKey_RealTime: CFStringRef;
    static kVTCompressionPropertyKey_AllowFrameReordering: CFStringRef;
    static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    static kVTCompressionPropertyKey_AverageBitRate: CFStringRef;
    static kVTCompressionPropertyKey_ExpectedFrameRate: CFStringRef;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: CFStringRef;
    static kVTProfileLevel_H264_ConstrainedHigh_AutoLevel: CFStringRef;
    static kVTProfileLevel_HEVC_Main_AutoLevel: CFStringRef;
    static kVTEncodeFrameOptionKey_ForceKeyFrame: CFStringRef;
    static kCFBooleanFalse: CFTypeRef;
    fn VTCompressionSessionCreate(
        alloc: *const c_void,
        largeur: i32,
        hauteur: i32,
        codec: u32,
        specification: CFTypeRef,
        attributs_source: CFTypeRef,
        alloc_donnees: *const c_void,
        rappel: RappelVT,
        refcon: *mut c_void,
        session: *mut *mut c_void,
    ) -> OSStatus;
    fn VTSessionSetProperty(session: *mut c_void, cle: CFStringRef, valeur: CFTypeRef) -> OSStatus;
    fn VTCompressionSessionPrepareToEncodeFrames(session: *mut c_void) -> OSStatus;
    fn VTCompressionSessionEncodeFrame(
        session: *mut c_void,
        image: *mut c_void,
        pts: CMTime,
        duree: CMTime,
        options: CFTypeRef,
        refcon_image: *mut c_void,
        infos: *mut u32,
    ) -> OSStatus;
    fn VTCompressionSessionCompleteFrames(session: *mut c_void, jusqu_a: CMTime) -> OSStatus;
    fn VTCompressionSessionInvalidate(session: *mut c_void);
}

struct Etat {
    rappel: RappelEncodeur,
    en_cours: AtomicU32,
}

pub struct Encodeur {
    session: *mut c_void,
    etat: *mut Etat,
    image_cle: Cf,
    debit: u32,
    pub codec: Codec,
}

// SAFETY: une session VideoToolbox s'utilise d'un fil à la fois (le Mutex de l'appelant).
unsafe impl Send for Encodeur {}

/// Images données à l'encodeur et pas encore rendues : au-delà, on saute la capture.
const EN_COURS_MAX: u32 = 2;

fn est_cle(sb: *mut c_void) -> bool {
    unsafe {
        let pieces = CMSampleBufferGetSampleAttachmentsArray(sb, 0);
        if pieces.is_null() || CFArrayGetCount(pieces) == 0 {
            return true;
        }
        let pas_sync = CFDictionaryGetValue(CFArrayGetValueAtIndex(pieces, 0), kCMSampleAttachmentKey_NotSync);
        pas_sync.is_null() || CFBooleanGetValue(pas_sync) == 0
    }
}

fn description(sb: *mut c_void) -> Option<Vec<u8>> {
    unsafe {
        let desc = CMSampleBufferGetFormatDescription(sb);
        if desc.is_null() {
            return None;
        }
        let atomes = CMFormatDescriptionGetExtension(desc, kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms);
        if atomes.is_null() {
            return None;
        }
        ["avcC", "hvcC"].iter().find_map(|nom| {
            let cle = cf_chaine(nom);
            let d = CFDictionaryGetValue(atomes, cle.0);
            (!d.is_null()).then(|| cf_octets(d))
        })
    }
}

extern "C" fn sortie(refcon: *mut c_void, refcon_image: *mut c_void, statut: OSStatus, _infos: u32, sb: *mut c_void) {
    let etat = unsafe { &*(refcon as *const Etat) };
    etat.en_cours.fetch_sub(1, Ordering::SeqCst);
    let n = refcon_image as usize as u32;
    let image = (|| {
        if statut != 0 || sb.is_null() {
            return None;
        }
        let bloc = unsafe { CMSampleBufferGetDataBuffer(sb) };
        if bloc.is_null() {
            return None;
        }
        let taille = unsafe { CMBlockBufferGetDataLength(bloc) };
        let mut donnees = vec![0u8; taille];
        if unsafe { CMBlockBufferCopyDataBytes(bloc, 0, taille, donnees.as_mut_ptr() as *mut c_void) } != 0 {
            return None;
        }
        let cle = est_cle(sb);
        Some(ImageEncodee { cle, donnees, description: if cle { description(sb) } else { None } })
    })();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (etat.rappel)(n, image)));
}

impl Encodeur {
    pub fn nouveau(codec: Codec, largeur: usize, hauteur: usize, debit: u32, rappel: RappelEncodeur) -> Result<Encodeur, String> {
        let type_codec: u32 = match codec {
            Codec::Avc => 0x6176_6331,  // 'avc1'
            Codec::Hevc => 0x6876_6331, // 'hvc1'
        };
        let etat = Box::into_raw(Box::new(Etat { rappel, en_cours: AtomicU32::new(0) }));
        let mut session: *mut c_void = std::ptr::null_mut();
        // faible latence d'abord (H.264), puis sans (HEVC selon la puce) ; toujours en matériel
        let mut statut = 0;
        for faible_latence in [true, false] {
            let mut spec = vec![(unsafe { kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder }, vrai())];
            if faible_latence {
                spec.push((unsafe { kVTVideoEncoderSpecification_EnableLowLatencyRateControl }, vrai()));
            }
            let spec = cf_dico(&spec);
            statut = unsafe {
                VTCompressionSessionCreate(
                    std::ptr::null(),
                    largeur as i32,
                    hauteur as i32,
                    type_codec,
                    spec.0,
                    std::ptr::null(),
                    std::ptr::null(),
                    sortie,
                    etat as *mut c_void,
                    &mut session,
                )
            };
            if statut == 0 {
                break;
            }
        }
        if statut != 0 || session.is_null() {
            drop(unsafe { Box::from_raw(etat) });
            return Err(format!("encodeur matériel {:?} indisponible ({statut})", codec));
        }
        let e = Encodeur {
            session,
            etat,
            image_cle: cf_dico(&[(unsafe { kVTEncodeFrameOptionKey_ForceKeyFrame }, vrai())]),
            debit: 0,
            codec,
        };
        unsafe {
            let profil = match codec {
                Codec::Avc => kVTProfileLevel_H264_ConstrainedHigh_AutoLevel,
                Codec::Hevc => kVTProfileLevel_HEVC_Main_AutoLevel,
            };
            VTSessionSetProperty(session, kVTCompressionPropertyKey_RealTime, vrai());
            VTSessionSetProperty(session, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse);
            VTSessionSetProperty(session, kVTCompressionPropertyKey_ProfileLevel, profil);
            VTSessionSetProperty(session, kVTCompressionPropertyKey_ExpectedFrameRate, cf_entier(60).0);
            // une image clé par minute au plus tard ; les autres seulement à la demande de l'iPad
            VTSessionSetProperty(session, kVTCompressionPropertyKey_MaxKeyFrameInterval, cf_entier(3600).0);
            VTCompressionSessionPrepareToEncodeFrames(session);
        }
        let mut e = e;
        e.regler_debit(debit);
        Ok(e)
    }

    pub fn regler_debit(&mut self, debit: u32) {
        if debit != self.debit {
            self.debit = debit;
            let n = cf_entier(debit.min(i32::MAX as u32) as i32);
            unsafe { VTSessionSetProperty(self.session, kVTCompressionPropertyKey_AverageBitRate, n.0) };
        }
    }

    /// Donne une image à encoder sous le numéro `n`. false : sautée (encodeur occupé ou erreur),
    /// le rappel ne sera pas appelé pour elle.
    pub fn encoder(&mut self, image: *mut c_void, pts: CMTime, cle: bool, n: u32) -> bool {
        let etat = unsafe { &*self.etat };
        if etat.en_cours.load(Ordering::SeqCst) >= EN_COURS_MAX {
            return false;
        }
        etat.en_cours.fetch_add(1, Ordering::SeqCst);
        let options = if cle { self.image_cle.0 } else { std::ptr::null() };
        let statut = unsafe {
            VTCompressionSessionEncodeFrame(self.session, image, pts, CMTime::INVALIDE, options, n as usize as *mut c_void, std::ptr::null_mut())
        };
        if statut != 0 {
            etat.en_cours.fetch_sub(1, Ordering::SeqCst);
        }
        statut == 0
    }
}

impl Drop for Encodeur {
    fn drop(&mut self) {
        unsafe {
            VTCompressionSessionCompleteFrames(self.session, CMTime::INVALIDE);
            VTCompressionSessionInvalidate(self.session);
            CFRelease(self.session);
            drop(Box::from_raw(self.etat));
        }
    }
}

/// Diagnostic : encode 30 images d'essai à cette taille, rend le temps moyen par image (ms).
pub fn essai(codec: Codec, largeur: usize, hauteur: usize) -> Result<f64, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Instant>();
    let tx = std::sync::Mutex::new(tx);
    let mut e = Encodeur::nouveau(
        codec,
        largeur,
        hauteur,
        20_000_000,
        Box::new(move |_: u32, image: Option<ImageEncodee>| {
            if image.is_some() {
                let _ = tx.lock().unwrap().send(Instant::now());
            }
        }),
    )?;
    let attributs_surface = cf_dico(&[]);
    let attributs = cf_dico(&[(unsafe { kCVPixelBufferIOSurfacePropertiesKey }, attributs_surface.0)]);
    let mut tampon: *mut c_void = std::ptr::null_mut();
    if unsafe { CVPixelBufferCreate(std::ptr::null(), largeur, hauteur, 0x3432_3076, attributs.0, &mut tampon) } != 0 {
        return Err("image d'essai impossible".into());
    }
    let mut total = Duration::ZERO;
    let mut faites = 0u32;
    for i in 0..30u32 {
        let debut = Instant::now();
        if e.encoder(tampon, CMTime::new(i as i64, 60), i == 0, i) {
            if let Ok(fin) = rx.recv_timeout(Duration::from_secs(2)) {
                total += fin.duration_since(debut);
                faites += 1;
            }
        }
    }
    drop(e);
    unsafe { CVPixelBufferRelease(tampon) };
    if faites == 0 {
        return Err("aucune image encodée".into());
    }
    Ok(total.as_secs_f64() * 1000.0 / faites as f64)
}
