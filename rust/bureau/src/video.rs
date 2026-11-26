//! Ce que Safari (WebCodecs) doit savoir du flux : la chaîne du codec, tirée de la description
//! avcC / hvcC que VideoToolbox fournit, et l'en-tête de chaque image.

/// En-tête binaire d'une image : [1][drapeaux : bit0 = image clé][numéro u32][capture µs u64], gros-boutiste.
pub const ENTETE_IMAGE: usize = 14;

pub fn entete_image(cle: bool, n: u32, capture_us: u64) -> [u8; ENTETE_IMAGE] {
    let mut e = [0u8; ENTETE_IMAGE];
    e[0] = 1;
    e[1] = cle as u8;
    e[2..6].copy_from_slice(&n.to_be_bytes());
    e[6..14].copy_from_slice(&capture_us.to_be_bytes());
    e
}

/// « avc1.640033 » : profil, compatibilité, niveau (octets 1 à 3 de l'avcC).
pub fn codec_avc(avcc: &[u8]) -> Option<String> {
    (avcc.len() >= 4 && avcc[0] == 1).then(|| format!("avc1.{:02x}{:02x}{:02x}", avcc[1], avcc[2], avcc[3]))
}

/// « hvc1.1.6.L153.B0 » (ISO/IEC 14496-15, annexe E), d'après les 13 premiers octets du hvcC.
pub fn codec_hevc(hvcc: &[u8]) -> Option<String> {
    if hvcc.len() < 13 || hvcc[0] != 1 {
        return None;
    }
    let espace = ["", "A", "B", "C"][(hvcc[1] >> 6) as usize];
    let palier = if hvcc[1] & 0x20 != 0 { 'H' } else { 'L' };
    let profil = hvcc[1] & 0x1F;
    let compat = u32::from_be_bytes([hvcc[2], hvcc[3], hvcc[4], hvcc[5]]).reverse_bits();
    let niveau = hvcc[12];
    let mut contraintes: Vec<String> = hvcc[6..12].iter().map(|o| format!("{:X}", o)).collect();
    while contraintes.last().is_some_and(|c| c == "0") {
        contraintes.pop();
    }
    let mut s = format!("hvc1.{espace}{profil}.{compat:X}.{palier}{niveau}");
    for c in contraintes {
        s += &format!(".{c}");
    }
    Some(s)
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn chaines_de_codec() {
        assert_eq!(codec_avc(&[1, 0x64, 0x00, 0x33, 0xFF]).as_deref(), Some("avc1.640033"));
        assert_eq!(codec_avc(&[0, 0x64]), None);
        // HEVC Main, palier principal, niveau 5.1, contraintes 0xB0 : la chaîne de l'exemple de la norme
        let hvcc = [1, 0x01, 0x60, 0, 0, 0, 0xB0, 0, 0, 0, 0, 0, 153, 0xF0];
        assert_eq!(codec_hevc(&hvcc).as_deref(), Some("hvc1.1.6.L153.B0"));
        let main10_haut = [1, 0x22, 0x20, 0, 0, 0, 0x90, 0, 0, 0, 0, 0, 120];
        assert_eq!(codec_hevc(&main10_haut).as_deref(), Some("hvc1.2.4.H120.90"));
        assert_eq!(codec_hevc(&[1, 2, 3]), None);
    }

    #[test]
    fn entete() {
        let e = entete_image(true, 0x01020304, 5);
        assert_eq!(e, [1, 1, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 5]);
    }
}
