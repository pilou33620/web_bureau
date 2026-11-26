//! Trames WebSocket (RFC 6455) côté serveur : la caisse fait la poignée de main, l'outil découpe
//! les trames. Le client masque toujours ses trames, le serveur jamais.

pub const TEXTE: u8 = 1;
pub const BINAIRE: u8 = 2;
pub const FERMER: u8 = 8;
pub const PING: u8 = 9;
pub const PONG: u8 = 10;
/// Une trame reçue plus grosse est refusée : le presse-papiers (1 Mo) est le plus gros message.
pub const TRAME_MAX: usize = 2 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct Trame {
    pub op: u8,
    pub donnees: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum ErreurTrame {
    NonMasquee,
    TropGrande,
    /// Les navigateurs n'envoient pas de messages fragmentés de cette taille : refusés.
    Fragmentee,
    OpInconnu(u8),
}

#[derive(Default)]
pub struct Trames {
    tampon: Vec<u8>,
}

impl Trames {
    pub fn ajouter(&mut self, octets: &[u8]) {
        self.tampon.extend_from_slice(octets);
    }

    /// La prochaine trame complète ; Ok(None) s'il manque des octets.
    pub fn suivante(&mut self) -> Result<Option<Trame>, ErreurTrame> {
        let b = &self.tampon;
        if b.len() < 2 {
            return Ok(None);
        }
        let op = b[0] & 0x0F;
        if b[0] & 0x80 == 0 || op == 0 {
            return Err(ErreurTrame::Fragmentee);
        }
        if !matches!(op, TEXTE | BINAIRE | FERMER | PING | PONG) {
            return Err(ErreurTrame::OpInconnu(op));
        }
        if b[1] & 0x80 == 0 {
            return Err(ErreurTrame::NonMasquee);
        }
        let (n, debut) = match b[1] & 0x7F {
            126 if b.len() < 4 => return Ok(None),
            126 => (u16::from_be_bytes([b[2], b[3]]) as u64, 4),
            127 if b.len() < 10 => return Ok(None),
            127 => (u64::from_be_bytes(b[2..10].try_into().unwrap()), 10),
            n => (n as u64, 2),
        };
        if n > TRAME_MAX as u64 {
            return Err(ErreurTrame::TropGrande);
        }
        let n = n as usize;
        if b.len() < debut + 4 + n {
            return Ok(None);
        }
        let masque = [b[debut], b[debut + 1], b[debut + 2], b[debut + 3]];
        let donnees = b[debut + 4..debut + 4 + n].iter().enumerate().map(|(i, o)| o ^ masque[i % 4]).collect();
        self.tampon.drain(..debut + 4 + n);
        Ok(Some(Trame { op, donnees }))
    }
}

/// Une trame du serveur, non masquée.
pub fn trame(op: u8, donnees: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(donnees.len() + 10);
    t.push(0x80 | op);
    match donnees.len() {
        n if n < 126 => t.push(n as u8),
        n if n <= 0xFFFF => {
            t.push(126);
            t.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            t.push(127);
            t.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    t.extend_from_slice(donnees);
    t
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Une trame comme un navigateur l'envoie (masquée).
    fn du_client(op: u8, donnees: &[u8]) -> Vec<u8> {
        let mut t = trame(op, donnees);
        let debut = match t[1] {
            126 => 4,
            127 => 10,
            _ => 2,
        };
        t[1] |= 0x80;
        let masque = [0x12, 0x34, 0x56, 0x78];
        for (i, o) in t[debut..].iter_mut().enumerate() {
            *o ^= masque[i % 4];
        }
        t.splice(debut..debut, masque);
        t
    }

    #[test]
    fn aller_retour_toutes_tailles() {
        for n in [0, 1, 125, 126, 65535, 65536, 300_000] {
            let donnees: Vec<u8> = (0..n).map(|i| i as u8).collect();
            let mut t = Trames::default();
            t.ajouter(&du_client(BINAIRE, &donnees));
            assert_eq!(t.suivante(), Ok(Some(Trame { op: BINAIRE, donnees })), "{n}");
            assert_eq!(t.suivante(), Ok(None));
        }
    }

    #[test]
    fn octets_arrives_par_morceaux() {
        let brut = du_client(TEXTE, b"{\"t\":\"souris\"}");
        let mut t = Trames::default();
        for o in &brut[..brut.len() - 1] {
            t.ajouter(&[*o]);
            assert_eq!(t.suivante(), Ok(None));
        }
        t.ajouter(&brut[brut.len() - 1..]);
        assert_eq!(t.suivante().unwrap().unwrap().donnees, b"{\"t\":\"souris\"}");
    }

    #[test]
    fn refus() {
        let mut t = Trames::default();
        t.ajouter(&trame(TEXTE, b"non masquee"));
        assert_eq!(t.suivante(), Err(ErreurTrame::NonMasquee));

        let mut t = Trames::default();
        t.ajouter(&[0x82, 0xFF, 0, 0, 0, 0, 0x10, 0, 0, 0]); // 256 Mo annoncés
        assert_eq!(t.suivante(), Err(ErreurTrame::TropGrande));

        let mut t = Trames::default();
        t.ajouter(&[0x01, 0x80]); // FIN absent
        assert_eq!(t.suivante(), Err(ErreurTrame::Fragmentee));

        let mut t = Trames::default();
        t.ajouter(&[0x83, 0x80]);
        assert_eq!(t.suivante(), Err(ErreurTrame::OpInconnu(3)));
    }
}
