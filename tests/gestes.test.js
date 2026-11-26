// node --test tests/ : un test par geste du cahier des charges (§7.3).
'use strict';
const test = require('node:test');
const assert = require('node:assert');
const { Gestes, APPUI_LONG } = require('../web/js/gestes.js');

// Une horloge factice : le temps n'avance que par avancer(ms).
function banc() {
  let maintenant = 0, id = 0;
  const minuteurs = new Map();
  const horloge = {
    maintenant: () => maintenant,
    plusTard: (f, ms) => { minuteurs.set(++id, { f, quand: maintenant + ms }); return id; },
    annuler: (i) => minuteurs.delete(i),
  };
  const actions = [];
  const g = new Gestes((a) => { if (a.t !== 'anneau') actions.push(a); }, horloge);
  const avancer = (ms) => {
    const fin = maintenant + ms;
    for (;;) {
      const prochain = [...minuteurs.entries()].filter(([, m]) => m.quand <= fin).sort((a, b) => a[1].quand - b[1].quand)[0];
      if (!prochain) break;
      maintenant = prochain[1].quand;
      minuteurs.delete(prochain[0]);
      prochain[1].f();
    }
    maintenant = fin;
  };
  const doigt = (id, x, y) => ({ id, type: 'touch', x, y });
  const boutons = () => actions.filter((a) => a.t === 'bouton').map((a) => `${a.b}${a.bas ? 'v' : '^'}`).join(' ');
  return { g, actions, avancer, doigt, boutons };
}

test('toucher court = clic gauche à l\'endroit touché', () => {
  const { g, actions, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 50));
  avancer(80);
  g.haut(doigt(1, 101, 51));
  assert.strictEqual(boutons(), '0v 0^');
  assert.deepStrictEqual(actions[0], { t: 'souris', x: 100, y: 50 });
});

test('double toucher = deux clics (le Mac compte le double clic)', () => {
  const { g, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 50)); avancer(60); g.haut(doigt(1, 100, 50));
  avancer(120);
  g.bas(doigt(2, 102, 52)); avancer(60); g.haut(doigt(2, 102, 52));
  assert.strictEqual(boutons(), '0v 0^ 0v 0^');
});

test('appui long immobile = clic droit, sans clic gauche au relâchement', () => {
  const { g, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 10, 10));
  avancer(APPUI_LONG + 10);
  g.haut(doigt(1, 10, 10));
  assert.strictEqual(boutons(), '1v 1^');
});

test('toucher puis appui maintenu et glisser = glisser-déposer', () => {
  const { g, actions, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 100)); avancer(50); g.haut(doigt(1, 100, 100));
  avancer(150);
  g.bas(doigt(2, 100, 100));
  g.bouge(doigt(2, 140, 100));
  g.bouge(doigt(2, 200, 120));
  g.haut(doigt(2, 200, 120));
  assert.strictEqual(boutons(), '0v 0^ 0v 0^');
  const derniere = actions.filter((a) => a.t === 'souris').pop();
  assert.deepStrictEqual(derniere, { t: 'souris', x: 200, y: 120 });
});

test('un doigt qui glisse = curseur, sans clic', () => {
  const { g, actions, doigt, boutons } = banc();
  g.bas(doigt(1, 0, 0));
  g.bouge(doigt(1, 30, 0));
  g.bouge(doigt(1, 60, 0));
  g.haut(doigt(1, 60, 0));
  assert.strictEqual(boutons(), '');
  assert.strictEqual(actions.filter((a) => a.t === 'souris').length, 2);
});

test('toucher à 2 doigts = clic droit', () => {
  const { g, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 100)); g.bas(doigt(2, 160, 100));
  avancer(100);
  g.haut(doigt(1, 100, 100)); g.haut(doigt(2, 160, 100));
  assert.strictEqual(boutons(), '1v 1^');
});

test('glisser à 2 doigts = défilement, puis inertie', () => {
  const { g, actions, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 300)); g.bas(doigt(2, 160, 300));
  for (let i = 1; i <= 10; i++) {
    avancer(16);
    g.bouge(doigt(1, 100, 300 - 20 * i)); g.bouge(doigt(2, 160, 300 - 20 * i));
  }
  g.haut(doigt(1, 100, 100)); g.haut(doigt(2, 160, 100));
  const pendant = actions.filter((a) => a.t === 'molette').length;
  assert.ok(pendant > 0);
  assert.ok(actions.filter((a) => a.t === 'molette').every((a) => a.dy > 0), 'doigts vers le haut : le contenu monte (dy > 0)');
  avancer(2000);
  assert.ok(actions.filter((a) => a.t === 'molette').length > pendant, 'inertie');
  assert.strictEqual(boutons(), '');
});

test('pincer = zoom de la vue, rien vers le Mac', () => {
  const { g, actions, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 100)); g.bas(doigt(2, 200, 100));
  g.bouge(doigt(2, 260, 100));
  g.bouge(doigt(2, 300, 100));
  g.haut(doigt(1, 100, 100)); g.haut(doigt(2, 300, 100));
  assert.ok(actions.some((a) => a.t === 'zoom' && a.facteur > 1));
  assert.ok(!actions.some((a) => a.t === 'molette'));
  assert.strictEqual(boutons(), '');
});

test('toucher à 3 doigts = clavier', () => {
  const { g, actions, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 100)); g.bas(doigt(2, 150, 100)); g.bas(doigt(3, 200, 100));
  avancer(120);
  g.haut(doigt(1, 100, 100)); g.haut(doigt(2, 150, 100)); g.haut(doigt(3, 200, 100));
  assert.deepStrictEqual(actions.filter((a) => a.t === 'clavier'), [{ t: 'clavier' }]);
  assert.strictEqual(boutons(), '');
});

test('Pencil et trackpad : pointage direct, survol, clic droit', () => {
  const { g, actions, boutons } = banc();
  g.bouge({ id: 9, type: 'pen', x: 5, y: 5 }); // survol
  g.bas({ id: 9, type: 'pen', x: 5, y: 5, bouton: 0 });
  g.bouge({ id: 9, type: 'pen', x: 9, y: 5 });
  g.haut({ id: 9, type: 'pen', x: 9, y: 5 });
  g.bas({ id: 1, type: 'mouse', x: 0, y: 0, bouton: 2 });
  g.haut({ id: 1, type: 'mouse', x: 0, y: 0 });
  assert.strictEqual(boutons(), '0v 0^ 1v 1^');
  assert.deepStrictEqual(actions[0], { t: 'souris', x: 5, y: 5 });
});

test('geste annulé par iPadOS : rien ne reste enfoncé', () => {
  const { g, avancer, doigt, boutons } = banc();
  g.bas(doigt(1, 100, 100)); avancer(50); g.haut(doigt(1, 100, 100));
  avancer(100);
  g.bas(doigt(2, 100, 100)); g.bouge(doigt(2, 150, 100));
  g.annule(doigt(2, 150, 100));
  assert.strictEqual(boutons(), '0v 0^ 0v 0^');
});
