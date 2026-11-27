// Remise en ordre des images reçues. Pure (aucun accès à la page) : testée par tests/ordre.test.js.
//
// Par le WebSocket les images arrivent dans l'ordre ; par UDP chacune a son flux et l'ordre n'est
// plus garanti. Chaque image {n, precedente, cle} nomme celle qui la précède et ne se décode
// qu'après elle ; une image clé se décode seule et fait oublier tout ce qui la précède.
(function (racine) {
  'use strict';

  const ATTENTE_MAX = 50; // ms : au-delà, on n'attend plus une image en retard sans rien demander

  const horlogeNavigateur = { plusTard: (f, ms) => setTimeout(f, ms), annuler: (id) => clearTimeout(id) };

  class Ordre {
    // sorties : decoder(image), accuser(n), demanderCle()
    constructor(sorties, horloge = horlogeNavigateur) {
      this.s = sorties;
      this.h = horloge;
      this.enAttente = new Map(); // numéro → image reçue, pas encore décodable
      this.minuteur = null;
      this.recommencer();
    }

    // Nouveau décodeur (ou nouvelle session) : il lui faut une image clé, et rien de ce qui attendait ne vaut plus.
    recommencer() {
      this.attendreCle = true;
      this.derniere = 0; // numéro de la dernière image donnée au décodeur
      this.enAttente.clear();
      this.arreter();
    }

    // pret : le décodeur peut recevoir des images
    recue(image, pret) {
      if (!pret || (this.attendreCle && !image.cle)) {
        this.s.accuser(image.n); // libère le Mac, qui n'attend pas une image que l'on ne montrera pas
        if (this.attendreCle) this.s.demanderCle();
        return;
      }
      if (image.n <= this.derniere) return; // arrivée après une image clé plus récente
      this.enAttente.set(image.n, image);
      let avance = false;
      for (;;) {
        const images = [...this.enAttente.values()];
        const suivante = images.filter((i) => i.cle).sort((a, b) => b.n - a.n)[0]
          || images.find((i) => i.precedente === this.derniere);
        if (!suivante) break;
        for (const n of [...this.enAttente.keys()]) if (n <= suivante.n) this.enAttente.delete(n);
        this.attendreCle = false;
        this.derniere = suivante.n;
        avance = true;
        this.s.decoder(suivante);
      }
      if (avance || !this.enAttente.size) this.arreter();
      if (this.enAttente.size && this.minuteur === null) this.minuteur = this.h.plusTard(() => this.retard(), ATTENTE_MAX);
    }

    // Une image manque depuis trop longtemps. Le Mac n'envoie plus rien tant que celles déjà reçues
    // ne sont pas accusées : on les accuse et on demande une image clé. La première arrivée, de
    // l'image en retard ou de l'image clé, relance le décodage.
    retard() {
      this.minuteur = null;
      if (!this.enAttente.size) return;
      this.s.accuser(Math.max(...this.enAttente.keys()));
      this.s.demanderCle();
    }

    arreter() {
      if (this.minuteur !== null) { this.h.annuler(this.minuteur); this.minuteur = null; }
    }
  }

  racine.Ordre = Ordre;
  if (typeof module !== 'undefined') module.exports = { Ordre, ATTENTE_MAX };
})(typeof window !== 'undefined' ? window : globalThis);
