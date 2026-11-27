//! Régulation du flux : jamais de file d'attente. Au plus 2 images (4 à 120 i/s) envoyées et pas encore
//! décodées par l'iPad ; au-delà, les captures sont sautées (l'image encodée suivante s'appuie sur
//! la dernière envoyée, rien ne casse). Le débit part du plafond demandé (comme Moonlight, qui
//! tient un débit fixe), baisse vite quand l'iPad accuse réception tard, et remonte lentement.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const DEBIT_MIN: u32 = 2_000_000;
/// Une image sans accusé de réception depuis ce délai est tenue pour perdue (page en arrière-plan).
pub const PERDUE: Duration = Duration::from_secs(1);
// Délais comparés au meilleur délai observé (encodage + réseau + décodage, propre à chaque installation).
// shortcut: ce meilleur délai ne remonte jamais pendant une session ; à faire vieillir si un réseau qui se
// dégrade durablement laisse le débit au plancher.
const MARGE_HAUTE: f64 = 0.060; // s au-dessus : on baisse
const MARGE_BASSE: f64 = 0.015; // s au-dessus : on peut monter

/// Le débit que Moonlight conseille pour cette taille et cette cadence (bit/s) : sa table par
/// nombre de pixels (streamingpreferences.cpp), interpolée, fois la cadence rapportée à 30 i/s.
pub fn debit_defaut(largeur: u32, hauteur: u32, ips: u32) -> u32 {
    const TABLE: [(f64, f64); 6] = [
        (640.0 * 360.0, 1.0),
        (854.0 * 480.0, 2.0),
        (1280.0 * 720.0, 5.0),
        (1920.0 * 1080.0, 10.0),
        (2560.0 * 1440.0, 20.0),
        (3840.0 * 2160.0, 40.0),
    ];
    let pixels = largeur as f64 * hauteur as f64;
    let facteur = match TABLE.iter().position(|(p, _)| pixels <= *p) {
        Some(0) => TABLE[0].1,
        Some(i) => {
            let ((p0, f0), (p1, f1)) = (TABLE[i - 1], TABLE[i]);
            f0 + (pixels - p0) / (p1 - p0) * (f1 - f0)
        }
        None => TABLE[TABLE.len() - 1].1,
    };
    // au-delà de 60 i/s, le débit ne monte plus en proportion de la cadence mais de sa racine carrée
    let cadence = if ips <= 60 { ips as f64 } else { (ips as f64 / 60.0).sqrt() * 60.0 };
    ((facteur * cadence / 30.0).round() * 1_000_000.0) as u32
}

pub struct Regulation {
    en_vol: VecDeque<(u32, Instant)>,
    en_vol_max: usize,
    debit: u32,
    max: u32,
    retard_lisse: f64,
    retard_min: f64,
    derniere_hausse: Instant,
    derniere_baisse: Instant,
}

impl Regulation {
    /// `max` : le plafond du débit (bit/s), d'où l'on part. `ips` : la cadence demandée.
    pub fn new(max: u32, ips: u32, t: Instant) -> Self {
        // 33 ms d'images en vol au plus : 2 jusqu'à 60 i/s, 4 à 120 (à 2, un aller-retour de 17 ms
        // suffirait à faire tomber la cadence sous 120)
        let en_vol_max = (ips as usize / 30).max(2);
        Regulation { en_vol: VecDeque::new(), en_vol_max, debit: max, max, retard_lisse: 0.0, retard_min: f64::INFINITY, derniere_hausse: t, derniere_baisse: t }
    }

    pub fn debit(&self) -> u32 {
        self.debit
    }

    /// Une nouvelle capture peut-elle partir à l'encodeur ?
    pub fn peut_envoyer(&mut self, t: Instant) -> bool {
        // une image jamais acquittée ne bloque pas le flux pour toujours
        while self.en_vol.front().is_some_and(|(_, envoi)| t.duration_since(*envoi) > PERDUE) {
            self.en_vol.pop_front();
        }
        self.en_vol.len() < self.en_vol_max
    }

    pub fn envoyee(&mut self, n: u32, t: Instant) {
        self.en_vol.push_back((n, t));
    }

    /// L'image n ne partira pas (encodeur qui la saute, file pleine) : elle ne compte plus.
    pub fn oubliee(&mut self, n: u32) {
        self.en_vol.retain(|(m, _)| *m != n);
    }

    /// L'instant de capture de l'image n, si elle est en vol.
    pub fn instant(&self, n: u32) -> Option<Instant> {
        self.en_vol.iter().find(|(m, _)| *m == n).map(|(_, t)| *t)
    }

    /// L'iPad a décodé l'image n (et donc toutes les précédentes).
    pub fn accusee(&mut self, n: u32, t: Instant) {
        let mut retard = None;
        while let Some(&(m, envoi)) = self.en_vol.front() {
            if m > n {
                break;
            }
            self.en_vol.pop_front();
            if m == n {
                retard = Some(t.duration_since(envoi).as_secs_f64());
            }
        }
        let Some(retard) = retard else { return };
        self.retard_lisse = 0.8 * self.retard_lisse + 0.2 * retard;
        self.retard_min = self.retard_min.min(retard);
        if retard > self.retard_min + MARGE_HAUTE {
            if t.duration_since(self.derniere_baisse) > Duration::from_millis(200) {
                // le plancher ne dépasse jamais un plafond choisi plus bas que lui
                self.debit = ((self.debit as f64 * 0.85) as u32).max(DEBIT_MIN.min(self.max));
                self.derniere_baisse = t;
            }
        } else if self.retard_lisse < self.retard_min + MARGE_BASSE
            && t.duration_since(self.derniere_hausse) > Duration::from_millis(500)
            && t.duration_since(self.derniere_baisse) > Duration::from_secs(2)
        {
            self.debit = (self.debit + 1_000_000).min(self.max);
            self.derniere_hausse = t;
        }
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn jamais_plus_de_deux_en_vol() {
        let t = Instant::now();
        let mut r = Regulation::new(30_000_000, 60, t);
        assert!(r.peut_envoyer(t));
        r.envoyee(1, t);
        assert!(r.peut_envoyer(t));
        r.envoyee(2, t);
        assert!(!r.peut_envoyer(t));
        r.accusee(1, t + Duration::from_millis(10));
        assert!(r.peut_envoyer(t));
        r.envoyee(3, t);
        // l'accusé de 3 libère aussi 2 (perdu en route ou acquitté groupé)
        r.accusee(3, t + Duration::from_millis(20));
        assert!(r.peut_envoyer(t));
        r.envoyee(9, t);
        assert_eq!(r.instant(9), Some(t));
        r.oubliee(9);
        assert_eq!(r.instant(9), None);
        // une image jamais acquittée est oubliée après PERDUE
        r.envoyee(4, t);
        r.envoyee(5, t);
        assert!(!r.peut_envoyer(t + Duration::from_millis(500)));
        assert!(r.peut_envoyer(t + PERDUE + Duration::from_millis(1)));
    }

    #[test]
    fn quatre_en_vol_a_120_images_par_seconde() {
        let t = Instant::now();
        let mut r = Regulation::new(30_000_000, 120, t);
        for n in 1..=4 {
            assert!(r.peut_envoyer(t));
            r.envoyee(n, t);
        }
        assert!(!r.peut_envoyer(t));
        assert!(Regulation::new(30_000_000, 30, t).en_vol_max == 2);
    }

    #[test]
    fn le_debit_suit_les_retards() {
        let t0 = Instant::now();
        let mut r = Regulation::new(30_000_000, 60, t0);
        let depart = r.debit();
        assert_eq!(depart, 30_000_000, "on part du débit demandé");
        // le délai normal de cette installation : 40 ms (au-dessus de tout seuil fixe raisonnable)
        for i in 0..5u32 {
            let t = t0 + Duration::from_millis(10 * i as u64);
            r.envoyee(i, t);
            r.accusee(i, t + Duration::from_millis(40));
        }
        // accusés lents : le débit baisse, jusqu'au plancher
        for i in 5..100u32 {
            let t = t0 + Duration::from_millis(300 * i as u64);
            r.envoyee(i, t);
            r.accusee(i, t + Duration::from_millis(150));
        }
        assert!(r.debit() < depart);
        assert_eq!(r.debit(), DEBIT_MIN);
        // accusés rapides : il remonte, jusqu'au plafond
        for i in 100..400u32 {
            let t = t0 + Duration::from_millis(300 * i as u64);
            r.envoyee(i, t);
            r.accusee(i, t + Duration::from_millis(42));
        }
        assert_eq!(r.debit(), 30_000_000);
    }

    #[test]
    fn un_plafond_sous_le_plancher_est_tenu() {
        let t0 = Instant::now();
        let mut r = Regulation::new(500_000, 60, t0);
        r.envoyee(0, t0);
        r.accusee(0, t0 + Duration::from_millis(10));
        for i in 1..20u32 {
            let t = t0 + Duration::from_millis(300 * i as u64);
            r.envoyee(i, t);
            r.accusee(i, t + Duration::from_millis(200));
        }
        assert_eq!(r.debit(), 500_000);
    }

    #[test]
    fn debit_conseille_par_moonlight() {
        assert_eq!(debit_defaut(1280, 720, 60), 10_000_000);
        assert_eq!(debit_defaut(1920, 1080, 60), 20_000_000);
        assert_eq!(debit_defaut(1920, 1080, 30), 10_000_000);
        assert_eq!(debit_defaut(2560, 1440, 60), 40_000_000);
        assert_eq!(debit_defaut(3840, 2160, 60), 80_000_000);
        assert_eq!(debit_defaut(2266, 1488, 60), 36_000_000); // l'iPad mini, entre 1080p et 1440p
        assert_eq!(debit_defaut(320, 200, 60), 2_000_000); // jamais sous la première ligne
        assert_eq!(debit_defaut(1920, 1080, 120), 28_000_000); // 20 × √2
        assert_eq!(debit_defaut(2266, 1488, 120), 51_000_000);
        assert_eq!(debit_defaut(8192, 8192, 60), 80_000_000); // ni au-dessus de la dernière
    }
}
