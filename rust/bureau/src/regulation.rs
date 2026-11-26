//! Régulation du flux : jamais de file d'attente. Au plus EN_VOL_MAX images envoyées et pas encore
//! décodées par l'iPad ; au-delà, les captures sont sautées (l'image encodée suivante s'appuie sur
//! la dernière envoyée, rien ne casse). Le débit baisse vite quand l'iPad accuse réception
//! tard, et remonte lentement quand tout va bien.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::protocole::Qualite;

pub const EN_VOL_MAX: usize = 2;
pub const DEBIT_MIN: u32 = 2_000_000;
/// Une image sans accusé de réception depuis ce délai est tenue pour perdue (page en arrière-plan).
pub const PERDUE: Duration = Duration::from_secs(1);
// Délais comparés au meilleur délai observé (encodage + réseau + décodage, propre à chaque installation).
// shortcut: ce meilleur délai ne remonte jamais pendant une session ; à faire vieillir si un réseau qui se
// dégrade durablement laisse le débit au plancher.
const MARGE_HAUTE: f64 = 0.060; // s au-dessus : on baisse
const MARGE_BASSE: f64 = 0.015; // s au-dessus : on peut monter

pub fn debit_max(q: Qualite) -> u32 {
    match q {
        Qualite::Econome => 8_000_000,
        Qualite::Auto => 30_000_000,
        Qualite::Maximum => 60_000_000,
    }
}

pub struct Regulation {
    en_vol: VecDeque<(u32, Instant)>,
    debit: u32,
    max: u32,
    retard_lisse: f64,
    retard_min: f64,
    derniere_hausse: Instant,
    derniere_baisse: Instant,
}

impl Regulation {
    pub fn new(q: Qualite, t: Instant) -> Self {
        let max = debit_max(q);
        Regulation { en_vol: VecDeque::new(), debit: (max / 2).max(DEBIT_MIN), max, retard_lisse: 0.0, retard_min: f64::INFINITY, derniere_hausse: t, derniere_baisse: t }
    }

    pub fn qualite(&mut self, q: Qualite) {
        // repart de la moitié du plafond : sinon un passage par « Économe » laissait le débit en bas
        self.max = debit_max(q);
        self.debit = (self.max / 2).max(DEBIT_MIN);
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
        self.en_vol.len() < EN_VOL_MAX
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
                self.debit = ((self.debit as f64 * 0.85) as u32).max(DEBIT_MIN);
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
        let mut r = Regulation::new(Qualite::Auto, t);
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
    fn le_debit_suit_les_retards() {
        let t0 = Instant::now();
        let mut r = Regulation::new(Qualite::Auto, t0);
        let depart = r.debit();
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
        assert_eq!(r.debit(), debit_max(Qualite::Auto));
        // un changement de qualité repart de la moitié du nouveau plafond, vers le haut comme vers le bas
        r.qualite(Qualite::Econome);
        assert_eq!(r.debit(), debit_max(Qualite::Econome) / 2);
        r.qualite(Qualite::Maximum);
        assert_eq!(r.debit(), debit_max(Qualite::Maximum) / 2);
    }
}
