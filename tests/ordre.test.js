// node --test tests/ : la remise en ordre des images (vidéo par UDP, un flux QUIC par image).
'use strict';
const test = require('node:test');
const assert = require('node:assert');
const { Ordre, ATTENTE_MAX } = require('../web/js/ordre.js');

// Une horloge factice et le journal de ce que l'ordre décide.
function banc() {
  let id = 0;
  const minuteurs = new Map();
  const journal = [];
  const o = new Ordre(
    { decoder: (i) => journal.push(`d${i.n}`), accuser: (n) => journal.push(`a${n}`), demanderCle: () => journal.push('cle') },
    { plusTard: (f) => { minuteurs.set(++id, f); return id; }, annuler: (i) => minuteurs.delete(i) },
  );
  const sonner = () => { const [[i, f]] = minuteurs; minuteurs.delete(i); f(); };
  const image = (n, precedente, cle = false) => ({ n, precedente, cle });
  return { o, journal, minuteurs, sonner, image, recue: (...a) => o.recue(image(...a), true) };
}

test('dans l\'ordre (WebSocket) : tout se décode à l\'arrivée, sans rien demander', () => {
  const { journal, minuteurs, recue } = banc();
  recue(1, 0, true); recue(2, 1); recue(5, 2); // 3 et 4 n'ont jamais été envoyées : 5 suit 2
  assert.deepStrictEqual(journal, ['d1', 'd2', 'd5']);
  assert.strictEqual(minuteurs.size, 0);
});

test('dans le désordre (UDP) : une image attend celle qui la précède', () => {
  const { journal, minuteurs, recue } = banc();
  recue(1, 0, true); recue(3, 2); recue(4, 3);
  assert.deepStrictEqual(journal, ['d1']);
  assert.strictEqual(minuteurs.size, 1);
  recue(2, 1);
  assert.deepStrictEqual(journal, ['d1', 'd2', 'd3', 'd4']);
  assert.strictEqual(minuteurs.size, 0, 'plus rien n\'attend : pas de demande d\'image clé');
});

test(`image en retard de plus de ${ATTENTE_MAX} ms : on libère le Mac et on demande une image clé, une fois`, () => {
  const { journal, minuteurs, sonner, recue } = banc();
  recue(1, 0, true); recue(3, 2); recue(4, 3);
  sonner();
  assert.deepStrictEqual(journal, ['d1', 'a4', 'cle']);
  assert.strictEqual(minuteurs.size, 0);
  // l'image clé arrive : elle se décode seule, et 3 et 4 sont oubliées
  recue(5, 4, true);
  recue(6, 5);
  recue(2, 1); // l'image en retard arrive enfin : trop tard
  assert.deepStrictEqual(journal, ['d1', 'a4', 'cle', 'd5', 'd6']);
  assert.strictEqual(minuteurs.size, 0);
});

test('l\'image en retard arrive avant l\'image clé : la suite reprend sans trou', () => {
  const { journal, sonner, recue } = banc();
  recue(1, 0, true); recue(3, 2);
  sonner();
  recue(2, 1);
  recue(4, 3, true);
  assert.deepStrictEqual(journal, ['d1', 'a3', 'cle', 'd2', 'd3', 'd4']);
});

test('de deux images clés en attente, la plus récente gagne', () => {
  const { journal, recue } = banc();
  recue(1, 0, true); recue(4, 3); recue(3, 2, true); recue(6, 5, true);
  assert.deepStrictEqual(journal, ['d1', 'd3', 'd4', 'd6']);
});

test('décodeur neuf : seule une image clé est acceptée', () => {
  const { o, journal, recue } = banc();
  recue(7, 6);
  assert.deepStrictEqual(journal, ['a7', 'cle']);
  recue(8, 7, true); recue(9, 8);
  o.recommencer(); // nouvelle session : les numéros repartent de 1
  recue(1, 0, true); recue(2, 1);
  assert.deepStrictEqual(journal, ['a7', 'cle', 'd8', 'd9', 'd1', 'd2']);
});

test('décodeur pas prêt : l\'image est accusée, pas décodée', () => {
  const { o, journal, image } = banc();
  o.recue(image(1, 0, true), false);
  assert.deepStrictEqual(journal, ['a1', 'cle']);
});
