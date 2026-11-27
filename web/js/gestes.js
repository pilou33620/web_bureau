// Gestes de l'iPad → actions sur le Mac. Pure (aucun accès à la page) : testée par
// tests/gestes.test.js. La page lui passe les événements pointeur et reçoit des actions :
//   {t:'souris', x, y}            position en pixels de la page
//   {t:'bouton', b, bas}          0 gauche, 1 droit, 2 milieu
//   {t:'molette', dx, dy}         pixels, sens du navigateur (dy > 0 : le contenu monte)
//   {t:'clavier'}                 ouvrir ou fermer le clavier à l'écran
//   {t:'zoom', facteur, cx, cy, dx, dy}   pincement : zoom et déplacement de la vue (iPad seul)
//   {t:'anneau', x, y, actif}     retour visuel de l'appui long
//
// Tactile, comme le mode « écran tactile » de Moonlight (AbsoluteTouchHandler) : toucher = clic
// gauche là où est le doigt ; un doigt qui glisse = glisser, bouton gauche tenu ; appui long
// immobile = clic droit ; un second toucher tout près et tout de suite clique au même point
// (double clic). Le bouton gauche ne s'enfonce qu'au premier mouvement ou au lever du doigt, et
// pas dès le contact comme dans Moonlight : sinon un appui long ou un geste à 2 doigts cliquerait.
// En plus de Moonlight : toucher à 2 doigts = clic droit ; glisser à 2 doigts = défilement (avec
// inertie) ; pincer = zoom de la vue ; toucher à 3 doigts = clavier.
(function (racine) {
  'use strict';

  // les valeurs de Moonlight ; ses distances (1 % et 2,5 % de la vue) font 10 et 30 px sur un iPad
  const APPUI_LONG = 650; // ms
  const TOLERANCE = 10; // px : en deçà, le doigt n'a pas bougé
  const DOUBLE = 250; // ms : un toucher plus tôt et à moins de 3 × TOLERANCE clique au point du précédent
  const TOUCHER_MAX = 300; // ms : au-delà, ce n'est plus un toucher à 2 ou 3 doigts
  const PINCER = 0.12; // écart relatif des doigts qui fait un pincement
  const BOUTONS = { 0: 0, 2: 1, 1: 2 }; // bouton du navigateur → le nôtre

  const horlogeNavigateur = {
    maintenant: () => performance.now(),
    plusTard: (f, ms) => setTimeout(f, ms),
    annuler: (id) => clearTimeout(id),
  };

  const distance = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);

  class Gestes {
    constructor(emettre, horloge = horlogeNavigateur) {
      this.emettre = emettre;
      this.h = horloge;
      this.doigts = new Map(); // id → {x, y, x0, y0}
      this.mode = null;
      this.debut = null; // {x, y, t} du premier doigt
      this.premier = null; // son identifiant : lui seul mène le curseur
      this.clic = null; // {x, y} où le bouton s'enfonce (le point du toucher d'avant, s'il est tout proche)
      this.dernierToucher = null; // {x, y, t} du dernier toucher fini
      this.minuteur = null;
      this.inertie = null;
      this.boutonsTenus = new Map(); // pointeur souris/stylet → bouton enfoncé
    }

    // e : {id, type: 'touch'|'pen'|'mouse', x, y, bouton}
    bas(e) {
      if (e.type !== 'touch') {
        const b = BOUTONS[e.bouton] ?? 0;
        this.boutonsTenus.set(e.id, b);
        this.emettre({ t: 'souris', x: e.x, y: e.y });
        this.emettre({ t: 'bouton', b, bas: true });
        return;
      }
      this.arreterInertie();
      const t = this.h.maintenant();
      this.doigts.set(e.id, { x: e.x, y: e.y, x0: e.x, y0: e.y });
      const n = this.doigts.size;
      if (n === 1) {
        this.mode = 'un';
        this.debut = { x: e.x, y: e.y, t };
        this.premier = e.id;
        const d = this.dernierToucher;
        this.clic = d && t - d.t < DOUBLE && distance(d, e) < 3 * TOLERANCE ? { x: d.x, y: d.y } : { x: e.x, y: e.y };
        this.emettre({ t: 'anneau', x: e.x, y: e.y, actif: true });
        this.minuteur = this.h.plusTard(() => this.appuiLong(), APPUI_LONG);
      } else if (n === 2 && this.mode === 'un') {
        this.annulerAppuiLong();
        this.mode = 'deux';
        const c = this.centre();
        this.deux = { t: this.debut.t, c0: c, c, d0: this.ecart(), d: this.ecart(), v: { x: 0, y: 0 }, tv: t };
      } else if (n === 3 && (this.mode === 'deux' || this.mode === 'un')) {
        this.annulerAppuiLong();
        this.mode = 'trois';
      } else if (n > 3) {
        this.mode = 'ignore';
      }
    }

    bouge(e) {
      if (e.type !== 'touch') {
        this.emettre({ t: 'souris', x: e.x, y: e.y }); // survol du Pencil compris
        return;
      }
      const doigt = this.doigts.get(e.id);
      if (!doigt) return;
      doigt.x = e.x;
      doigt.y = e.y;
      switch (this.mode) {
        case 'un':
          if (distance(this.debut, e) <= TOLERANCE) return;
          this.annulerAppuiLong();
          this.mode = 'glisser';
          this.emettre({ t: 'souris', x: this.clic.x, y: this.clic.y });
          this.emettre({ t: 'bouton', b: 0, bas: true });
          this.emettre({ t: 'souris', x: e.x, y: e.y });
          return;
        case 'glisser':
          if (e.id === this.premier) this.emettre({ t: 'souris', x: e.x, y: e.y }); // un doigt posé en plus ne déplace rien
          return;
        case 'deux':
        case 'defiler':
        case 'pincer':
          return this.deuxDoigts();
        case 'trois':
          if (distance({ x: doigt.x0, y: doigt.y0 }, e) > 3 * TOLERANCE) this.mode = 'ignore';
          return;
      }
    }

    haut(e) {
      if (e.type !== 'touch') {
        const b = this.boutonsTenus.get(e.id);
        this.boutonsTenus.delete(e.id);
        if (b !== undefined) this.emettre({ t: 'bouton', b, bas: false });
        return;
      }
      if (!this.doigts.delete(e.id)) return;
      const t = this.h.maintenant();
      const reste = this.doigts.size;
      switch (this.mode) {
        case 'un': {
          this.annulerAppuiLong();
          // deux touchers du doigt tombent rarement au même point : le second clique où le premier a cliqué
          const { x, y } = this.clic;
          this.emettre({ t: 'souris', x, y });
          this.emettre({ t: 'bouton', b: 0, bas: true });
          this.emettre({ t: 'bouton', b: 0, bas: false });
          this.dernierToucher = { x, y, t };
          break;
        }
        case 'glisser':
          if (reste === 0) {
            this.emettre({ t: 'bouton', b: 0, bas: false });
            this.dernierToucher = null;
          }
          break;
        case 'deux':
          if (reste === 0 && t - this.deux.t < TOUCHER_MAX) {
            const c = this.deux.c0;
            this.emettre({ t: 'souris', x: c.x, y: c.y });
            this.emettre({ t: 'bouton', b: 1, bas: true });
            this.emettre({ t: 'bouton', b: 1, bas: false });
          }
          break;
        case 'defiler':
          if (reste === 0) this.lancerInertie();
          break;
        case 'trois':
          if (reste === 0 && t - this.debut.t < TOUCHER_MAX + 100) this.emettre({ t: 'clavier' });
          break;
      }
      // un geste à plusieurs doigts garde son mode jusqu'au dernier doigt levé
      if (reste === 0) this.mode = null;
    }

    annule(e) {
      // pointercancel : iPadOS a pris la main (geste système) ; on relâche sans cliquer
      if (e.type !== 'touch') return this.haut(e);
      this.doigts.delete(e.id);
      this.annulerAppuiLong();
      if (this.mode === 'glisser') this.emettre({ t: 'bouton', b: 0, bas: false });
      if (this.doigts.size === 0) this.mode = null;
      else this.mode = 'ignore';
    }

    // -- interne --

    appuiLong() {
      this.minuteur = null;
      if (this.mode !== 'un') return;
      this.mode = 'long';
      const { x, y } = this.clic;
      this.emettre({ t: 'anneau', x: this.debut.x, y: this.debut.y, actif: false });
      this.emettre({ t: 'souris', x, y });
      this.emettre({ t: 'bouton', b: 1, bas: true });
      this.emettre({ t: 'bouton', b: 1, bas: false });
    }

    annulerAppuiLong() {
      if (this.minuteur !== null) {
        this.h.annuler(this.minuteur);
        this.minuteur = null;
        this.emettre({ t: 'anneau', x: this.debut.x, y: this.debut.y, actif: false });
      }
    }

    centre() {
      let x = 0, y = 0;
      for (const d of this.doigts.values()) { x += d.x; y += d.y; }
      const n = this.doigts.size || 1;
      return { x: x / n, y: y / n };
    }

    ecart() {
      const [a, b] = [...this.doigts.values()];
      return a && b ? Math.max(1, distance(a, b)) : 1;
    }

    deuxDoigts() {
      if (this.doigts.size < 2) return;
      const s = this.deux;
      const c = this.centre();
      const d = this.ecart();
      if (this.mode === 'deux') {
        if (Math.abs(d / s.d0 - 1) > PINCER) this.mode = 'pincer';
        else if (distance(c, s.c0) > TOLERANCE) {
          this.mode = 'defiler';
          this.emettre({ t: 'souris', x: s.c0.x, y: s.c0.y }); // le Mac fait défiler ce qui est sous son curseur
        }
        else return;
      }
      const t = this.h.maintenant();
      const dx = c.x - s.c.x, dy = c.y - s.c.y;
      if (this.mode === 'defiler') {
        this.emettre({ t: 'molette', dx: -dx, dy: -dy });
        const dt = Math.max(1, t - s.tv);
        s.v = { x: 0.7 * s.v.x + 0.3 * (-dx / dt), y: 0.7 * s.v.y + 0.3 * (-dy / dt) };
      } else {
        this.emettre({ t: 'zoom', facteur: d / s.d, cx: c.x, cy: c.y, dx, dy });
      }
      s.c = c;
      s.d = d;
      s.tv = t;
    }

    lancerInertie() {
      let v = this.deux.v; // px/ms
      const pas = () => {
        v = { x: v.x * 0.92, y: v.y * 0.92 };
        if (Math.hypot(v.x, v.y) < 0.05) { this.inertie = null; return; }
        this.emettre({ t: 'molette', dx: v.x * 16, dy: v.y * 16 });
        this.inertie = this.h.plusTard(pas, 16);
      };
      if (Math.hypot(v.x, v.y) >= 0.3) this.inertie = this.h.plusTard(pas, 16);
    }

    arreterInertie() {
      if (this.inertie !== null) { this.h.annuler(this.inertie); this.inertie = null; }
    }
  }

  racine.Gestes = Gestes;
  if (typeof module !== 'undefined') module.exports = { Gestes, APPUI_LONG, DOUBLE };
})(typeof window !== 'undefined' ? window : globalThis);
