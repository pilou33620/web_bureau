// web_bureau, côté iPad : WebSocket chiffré, décodage WebCodecs, gestes, clavier,
// presse-papiers, fichiers. Tout texte reçu passe par textContent (jamais innerHTML).
'use strict';

const ENTETE_WEB = { 'X-WEB_BUREAU': '1' };
const $ = (id) => document.getElementById(id);
const ecran = $('ecran');
const scene = $('scene');
const ctx = ecran.getContext('2d', { alpha: false, desynchronized: true });

let ws = null;
let finVoulue = false; // la session s'est terminée exprès : pas de reconnexion automatique
let decodeur = null;
let config = null;
let attendreCle = true;
let touchesConnues = new Set();
let qualite = 'auto';

// -- voile (messages plein écran) ------------------------------------------------

function voile(texte, bouton) {
  $('voile').hidden = false;
  $('voile-texte').textContent = texte;
  const b = $('voile-bouton');
  b.hidden = !bouton;
  if (bouton) { b.textContent = bouton.texte; b.onclick = bouton.action; }
}
const sansVoile = () => { $('voile').hidden = true; };
const reprendre = { texte: 'Reprendre', action: () => { sansVoile(); connecter(); } };

// -- connexion -------------------------------------------------------------------

function tailleEcran() {
  const r = window.devicePixelRatio || 1;
  const pair = (n) => Math.min(8192, Math.max(320, Math.round(n * r))) & ~1;
  return [pair(window.innerWidth), pair(window.innerHeight)];
}

async function codecsDecodables([l, h]) {
  if (!('VideoDecoder' in window)) return [];
  const essais = [['hevc', 'hvc1.1.6.L153.B0'], ['avc', 'avc1.640034']];
  const oui = [];
  for (const [nom, codec] of essais) {
    try {
      const r = await VideoDecoder.isConfigSupported({ codec, codedWidth: l, codedHeight: h, hardwareAcceleration: 'prefer-hardware' });
      if (r.supported) oui.push(nom);
    } catch { /* codec inconnu */ }
  }
  return oui;
}

let tailleConnectee = null;

async function connecter() {
  if (!window.isSecureContext && location.hostname !== '127.0.0.1') {
    voile('Cette page doit être ouverte en https:// (installez la CA du Mac : voir le README).');
    return;
  }
  const taille = tailleEcran();
  const codecs = await codecsDecodables(taille);
  if (!codecs.length) {
    voile('Safari ne peut pas décoder la vidéo ici (WebCodecs absent : iPadOS 16.4 ou plus récent, en https).');
    return;
  }
  finVoulue = false;
  voile('Connexion au Mac…');
  ws = new WebSocket(`${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws`);
  ws.binaryType = 'arraybuffer';
  ws.onopen = () => { tailleConnectee = taille; envoyer({ t: 'bonjour', ecran: taille, codecs }); envoyer({ t: 'qualite', q: qualite }); };
  ws.onmessage = (ev) => (typeof ev.data === 'string' ? recuTexte(JSON.parse(ev.data)) : recuImage(ev.data));
  ws.onclose = () => {
    fermerDecodeur();
    ws = null;
    if (!finVoulue) {
      voile('Connexion perdue, nouvel essai…');
      setTimeout(connecter, 1500);
    }
  };
}

function envoyer(m) {
  if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(m));
}

// -- vidéo -----------------------------------------------------------------------

function base64(texte) {
  return Uint8Array.from(atob(texte), (c) => c.charCodeAt(0));
}

function fermerDecodeur() {
  if (decodeur && decodeur.state !== 'closed') decodeur.close();
  decodeur = null;
}

function configurer() {
  fermerDecodeur();
  decodeur = new VideoDecoder({ output: dessiner, error: () => { envoyer({ t: 'image_cle' }); setTimeout(configurer, 0); } });
  decodeur.configure({
    codec: config.codec,
    codedWidth: config.largeur,
    codedHeight: config.hauteur,
    description: base64(config.description),
    optimizeForLatency: true,
    hardwareAcceleration: 'prefer-hardware',
  });
  attendreCle = true;
}

function recuImage(tampon) {
  const v = new DataView(tampon);
  const cle = (v.getUint8(1) & 1) === 1;
  const n = v.getUint32(2);
  const captureUs = Number(v.getBigUint64(6));
  mesures.octets += tampon.byteLength;
  if (!decodeur || decodeur.state !== 'configured' || (attendreCle && !cle)) {
    envoyer({ t: 'ack', n }); // libère le Mac, qui n'attend pas une image que l'on ne montrera pas
    if (attendreCle) envoyer({ t: 'image_cle' });
    return;
  }
  attendreCle = false;
  captures.set(n, captureUs);
  decodeur.decode(new EncodedVideoChunk({ type: cle ? 'key' : 'delta', timestamp: n, data: new Uint8Array(tampon, 14) }));
}

const captures = new Map();

function dessiner(image) {
  ctx.drawImage(image, 0, 0, ecran.width, ecran.height);
  const n = image.timestamp;
  image.close();
  envoyer({ t: 'ack', n });
  mesures.images++;
  const c = captures.get(n);
  captures.delete(n);
  if (c !== undefined && horloge.ecart !== null) mesures.latences.push(performance.now() * 1000 + horloge.ecart - c);
  if (captures.size > 120) captures.clear();
}

// -- messages du Mac -----------------------------------------------------------------

const horloge = { ecart: null }; // horloge du Mac (µs) − horloge de la page (µs)
let attenteCopie = null;

function recuTexte(m) {
  switch (m.t) {
    case 'config':
      config = m;
      ecran.width = m.largeur;
      ecran.height = m.hauteur;
      disposer();
      configurer();
      sansVoile();
      break;
    case 'ping':
      envoyer({ t: 'pong', n: m.n });
      horloge.ecart = m.us + (m.rtt * 1000) / 2 - performance.now() * 1000;
      mesures.debit = m.debit;
      mesures.codec = m.codec;
      break;
    case 'presse_papiers':
      if (attenteCopie) { attenteCopie(m.texte); attenteCopie = null; }
      break;
    case 'etat':
      $('verrou').hidden = !m.verrouille;
      break;
    case 'fin':
      finVoulue = true;
      voile({
        inactivite: 'Session terminée après 15 minutes sans geste.',
        reprise: 'Session reprise par un autre appareil.',
        coupee_sur_mac: 'Session coupée depuis le Mac.',
        message_refuse: 'Session terminée : message refusé par le Mac.',
      }[m.raison] || 'Session terminée.', reprendre);
      break;
    case 'erreur':
      finVoulue = true;
      voile(m.texte, reprendre);
      break;
  }
}

// -- disposition et zoom de la vue ------------------------------------------------------

const vue = { echelle: 1, tx: 0, ty: 0 };

function disposer() {
  const W = window.innerWidth, H = window.innerHeight;
  const k = Math.min(W / ecran.width, H / ecran.height);
  const l = ecran.width * k, h = ecran.height * k;
  ecran.style.width = `${l}px`;
  ecran.style.height = `${h}px`;
  if (vue.echelle <= 1) { vue.echelle = 1; vue.tx = 0; vue.ty = 0; }
  const bx = (W - l) / 2, by = (H - h) / 2;
  ecran.style.transform = `translate(${bx + vue.tx}px, ${by + vue.ty}px) scale(${vue.echelle})`;
}

function zoomer(a) {
  const W = window.innerWidth, H = window.innerHeight;
  const avant = vue.echelle;
  vue.echelle = Math.min(5, Math.max(1, vue.echelle * a.facteur));
  const f = vue.echelle / avant;
  // le point sous les doigts reste sous les doigts ; les doigts déplacent aussi la vue
  const r = ecran.getBoundingClientRect();
  vue.tx += (a.cx - r.left) * (1 - f) + a.dx;
  vue.ty += (a.cy - r.top) * (1 - f) + a.dy;
  const l = parseFloat(ecran.style.width) * vue.echelle, h = parseFloat(ecran.style.height) * vue.echelle;
  const bx = (W - parseFloat(ecran.style.width)) / 2, by = (H - parseFloat(ecran.style.height)) / 2;
  vue.tx = Math.min(-bx, Math.max(W - l - bx, vue.tx));
  vue.ty = Math.min(-by, Math.max(H - h - by, vue.ty));
  disposer();
}

window.addEventListener('resize', () => {
  if (!config) return;
  disposer();
  const t = tailleEcran();
  if (ws && tailleConnectee && (t[0] !== tailleConnectee[0] || t[1] !== tailleConnectee[1])) {
    clearTimeout(window.reconnexion);
    window.reconnexion = setTimeout(() => ws && ws.close(), 600); // l'écran virtuel suit l'orientation
  }
});

// -- gestes ------------------------------------------------------------------------------

let derniereSouris = 0, sourisEnAttente = null, minuteurSouris = null;

function normalise(x, y) {
  const r = ecran.getBoundingClientRect();
  const borne = (v) => Math.min(1, Math.max(0, v));
  return { x: borne((x - r.left) / r.width), y: borne((y - r.top) / r.height) };
}

function viderSouris() {
  clearTimeout(minuteurSouris);
  minuteurSouris = null;
  if (sourisEnAttente) { envoyer(sourisEnAttente); sourisEnAttente = null; derniereSouris = performance.now(); }
}

function action(a) {
  switch (a.t) {
    case 'souris': {
      sourisEnAttente = { t: 'souris', ...normalise(a.x, a.y) };
      const attente = 8 - (performance.now() - derniereSouris); // 120 envois/s au plus
      if (attente <= 0) viderSouris();
      else if (!minuteurSouris) minuteurSouris = setTimeout(viderSouris, attente);
      break;
    }
    case 'bouton': viderSouris(); envoyer(a); break;
    case 'molette': envoyer({ t: 'molette', dx: Math.max(-5000, Math.min(5000, a.dx * 2)), dy: Math.max(-5000, Math.min(5000, a.dy * 2)) }); break;
    case 'clavier': basculerClavier(); break;
    case 'zoom': zoomer(a); break;
    case 'anneau': {
      const an = $('anneau');
      an.classList.toggle('actif', a.actif);
      if (a.actif) { an.style.left = `${a.x}px`; an.style.top = `${a.y}px`; }
      break;
    }
  }
}

const gestes = new Gestes(action);
const evenement = (e) => ({ id: e.pointerId, type: e.pointerType || 'mouse', x: e.clientX, y: e.clientY, bouton: e.button });

scene.addEventListener('pointerdown', (e) => { e.preventDefault(); scene.setPointerCapture(e.pointerId); gestes.bas(evenement(e)); });
scene.addEventListener('pointermove', (e) => { e.preventDefault(); gestes.bouge(evenement(e)); });
scene.addEventListener('pointerup', (e) => { e.preventDefault(); gestes.haut(evenement(e)); });
scene.addEventListener('pointercancel', (e) => gestes.annule(evenement(e)));
scene.addEventListener('contextmenu', (e) => e.preventDefault());
scene.addEventListener('wheel', (e) => {
  e.preventDefault();
  const k = e.deltaMode === 1 ? 16 : 1; // lignes → pixels
  envoyer({ t: 'molette', dx: Math.max(-5000, Math.min(5000, e.deltaX * k)), dy: Math.max(-5000, Math.min(5000, e.deltaY * k)) });
}, { passive: false });
document.addEventListener('gesturestart', (e) => e.preventDefault()); // pas de zoom de la page par Safari

// -- clavier -----------------------------------------------------------------------------

const saisie = $('saisie');
const SENTINELLE = ' '; // un caractère à effacer : sans lui, « effacer » n'arrive pas d'un champ vide

function remettreSaisie() { saisie.value = SENTINELLE; saisie.setSelectionRange(1, 1); }

function basculerClavier() {
  if (document.activeElement === saisie) saisie.blur();
  else { remettreSaisie(); saisie.focus({ preventScroll: true }); }
}

const dansUnChamp = (cible) => cible !== saisie && cible instanceof HTMLElement && cible.closest('input, textarea, .panneau');

document.addEventListener('keydown', (e) => {
  if (!ws || dansUnChamp(e.target) || e.isComposing || !touchesConnues.has(e.code)) return;
  e.preventDefault();
  if (e.metaKey && e.code === 'KeyV') return collerVersMac(true); // ⌘ est déjà enfoncé sur le Mac
  if (e.metaKey && (e.code === 'KeyC' || e.code === 'KeyX')) copierVersIPad();
  envoyer({ t: 'touche', code: e.code, bas: true }); // les répétitions aussi : le Mac ne répète pas une touche simulée
});

document.addEventListener('keyup', (e) => {
  if (!ws || dansUnChamp(e.target) || !touchesConnues.has(e.code)) return;
  e.preventDefault();
  envoyer({ t: 'touche', code: e.code, bas: false });
});

// clavier à l'écran : du texte, pas des touches
saisie.addEventListener('beforeinput', (e) => {
  if (e.inputType === 'insertCompositionText') return; // fini par compositionend
  e.preventDefault();
  if ((e.inputType === 'insertText' || e.inputType === 'insertReplacementText') && e.data) envoyerTexte(e.data);
  else if (e.inputType === 'insertLineBreak' || e.inputType === 'insertParagraph') tapoter('Enter');
  else if (e.inputType === 'deleteContentBackward' || e.inputType === 'deleteWordBackward') tapoter('Backspace');
  else if (e.inputType === 'deleteContentForward') tapoter('Delete');
  remettreSaisie();
});
saisie.addEventListener('compositionend', (e) => { if (e.data) envoyerTexte(e.data); remettreSaisie(); });
saisie.addEventListener('input', remettreSaisie);
saisie.addEventListener('focus', () => document.body.classList.add('clavier-ouvert'));
saisie.addEventListener('blur', () => document.body.classList.remove('clavier-ouvert'));

function envoyerTexte(s) {
  for (let i = 0; i < s.length; i += 500) envoyer({ t: 'texte', s: s.slice(i, i + 500) });
}

function tapoter(code) {
  envoyer({ t: 'touche', code, bas: true });
  envoyer({ t: 'touche', code, bas: false });
}

window.addEventListener('blur', () => envoyer({ t: 'relacher' }));
document.addEventListener('visibilitychange', () => {
  if (document.hidden) envoyer({ t: 'relacher' });
  else envoyer({ t: 'image_cle' });
});

// -- presse-papiers ---------------------------------------------------------------------------

function attendreTexteDuMac() {
  return new Promise((ok, ko) => {
    attenteCopie = ok;
    setTimeout(() => ko(new Error('le Mac n\'a rien copié')), 2500);
  });
}

// Safari n'écrit dans le presse-papiers que pendant un geste : on lui donne tout de suite une
// promesse, tenue quand le texte copié arrive du Mac.
function copierVersIPad() {
  if (!navigator.clipboard || !window.ClipboardItem) return;
  const texte = attendreTexteDuMac().then((t) => new Blob([t], { type: 'text/plain' }));
  navigator.clipboard.write([new ClipboardItem({ 'text/plain': texte })]).catch(() => {});
}

async function collerVersMac(depuisLeClavier) {
  try {
    const t = await Promise.race([navigator.clipboard.readText(), new Promise((_, ko) => setTimeout(ko, 4000))]);
    if (t && t.length <= 1000000) envoyer({ t: 'presse_papiers', texte: t });
  } catch { /* refusé ou vide : on colle ce que le Mac a déjà */ }
  if (depuisLeClavier) tapoter('KeyV');
  else envoyer({ t: 'raccourci', mods: ['cmd'], c: 'v' });
}

// -- barre d'outils ---------------------------------------------------------------------------

const QUALITES = ['auto', 'econome', 'maximum'];
const NOMS_QUALITE = { auto: 'Auto', econome: 'Économe', maximum: 'Max' };

const actions = {
  clavier: basculerClavier,
  cmdtab: () => envoyer({ t: 'raccourci', mods: ['cmd'], code: 'Tab' }),
  mission: () => envoyer({ t: 'raccourci', mods: ['ctrl'], code: 'ArrowUp' }),
  spotlight: () => envoyer({ t: 'raccourci', mods: ['cmd'], code: 'Space' }),
  echap: () => tapoter('Escape'),
  tab: () => tapoter('Tab'),
  gauche: () => tapoter('ArrowLeft'),
  droite: () => tapoter('ArrowRight'),
  haut: () => tapoter('ArrowUp'),
  bas: () => tapoter('ArrowDown'),
  copier: () => { copierVersIPad(); envoyer({ t: 'raccourci', mods: ['cmd'], c: 'c' }); },
  coller: () => collerVersMac(false),
  zoom: () => { vue.echelle = 1; disposer(); },
  qualite: (b) => {
    qualite = QUALITES[(QUALITES.indexOf(qualite) + 1) % QUALITES.length];
    b.textContent = NOMS_QUALITE[qualite];
    envoyer({ t: 'qualite', q: qualite });
  },
  mesures: () => { $('mesures').hidden = !$('mesures').hidden; },
  fichiers: () => basculerPanneau('fichiers', () => ouvrirDossier('')),
  infos: () => basculerPanneau('infos', diagnostic),
  replier: () => document.body.classList.toggle('barre-repliee'),
};

document.querySelectorAll('[data-action]').forEach((b) => {
  // pointerdown + preventDefault : le bouton ne prend pas le focus (le clavier à l'écran reste ouvert)
  b.addEventListener('pointerdown', (e) => e.preventDefault());
  b.addEventListener('click', () => actions[b.dataset.action](b));
});

function basculerPanneau(nom, ouvrir) {
  const p = $(`panneau-${nom}`);
  const ouvert = p.hidden;
  document.querySelectorAll('.panneau').forEach((x) => { x.hidden = true; });
  p.hidden = !ouvert;
  if (ouvert) ouvrir();
}
document.querySelectorAll('.fermer').forEach((b) => b.addEventListener('click', () => { b.closest('.panneau').hidden = true; }));

// -- mesures ----------------------------------------------------------------------------------

const mesures = { images: 0, octets: 0, latences: [], debit: 0, codec: '' };
setInterval(() => {
  const l = mesures.latences.sort((a, b) => a - b);
  const mediane = l.length ? Math.round(l[l.length >> 1] / 1000) : '–';
  $('mesures').textContent = `${mesures.codec || '–'} · ${mesures.images} i/s · ${(mesures.octets * 8 / 1e6).toFixed(1)} Mb/s`
    + ` · capture→écran ${mediane} ms · cible ${(mesures.debit / 1e6).toFixed(0)} Mb/s`;
  mesures.images = 0;
  mesures.octets = 0;
  mesures.latences = [];
}, 1000);

// -- écran verrouillé ---------------------------------------------------------------------------

$('verrou-form').addEventListener('submit', (e) => {
  e.preventDefault();
  const mdp = $('verrou-mdp').value;
  $('verrou-mdp').value = '';
  if (mdp) envoyer({ t: 'deverrouiller', mdp });
  $('verrou-aide').textContent = 'Mot de passe envoyé. Si le Mac reste verrouillé, macOS refuse la saisie à distance : voir Infos.';
});

// -- fichiers --------------------------------------------------------------------------------------

const entete = { credentials: 'same-origin', cache: 'no-store' };
let dossierCourant = '';

function taille(n) {
  if (n < 1024) return `${n} o`;
  if (n < 1048576) return `${(n / 1024).toFixed(0)} Ko`;
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} Mo`;
  return `${(n / 1073741824).toFixed(2)} Go`;
}

async function ouvrirDossier(chemin) {
  const liste = $('fichiers-liste');
  const r = await fetch(`/api/fichiers?chemin=${encodeURIComponent(chemin)}`, entete);
  const v = await r.json();
  if (!r.ok) { $('fichiers-etat').textContent = v.detail || 'Dossier illisible'; return; }
  dossierCourant = v.chemin;
  $('fichiers-chemin').textContent = `~/${v.chemin}`;
  liste.replaceChildren();
  if (v.chemin) {
    const li = document.createElement('li');
    li.textContent = '⬑ Dossier parent';
    li.className = 'dossier';
    li.onclick = () => ouvrirDossier(v.chemin.split('/').slice(0, -1).join('/'));
    liste.append(li);
  }
  for (const f of v.entrees) {
    const li = document.createElement('li');
    const chemin = v.chemin ? `${v.chemin}/${f.nom}` : f.nom;
    if (f.dossier) {
      li.className = 'dossier';
      li.textContent = `📁 ${f.nom}`;
      li.onclick = () => ouvrirDossier(chemin);
    } else {
      const a = document.createElement('a');
      a.href = `/api/fichiers/telecharger?chemin=${encodeURIComponent(chemin)}`;
      a.download = f.nom;
      a.textContent = f.nom;
      const t = document.createElement('span');
      t.textContent = taille(f.taille);
      li.append(a, t);
    }
    liste.append(li);
  }
}

$('fichiers-envoi').addEventListener('change', (e) => {
  const fichiers = [...e.target.files];
  e.target.value = '';
  const suivant = () => {
    const f = fichiers.shift();
    if (!f) { ouvrirDossier(dossierCourant); return; }
    const x = new XMLHttpRequest();
    x.open('POST', `/api/fichiers/envoyer?nom=${encodeURIComponent(f.name)}`);
    x.setRequestHeader('X-WEB_BUREAU', '1');
    x.upload.onprogress = (p) => { $('fichiers-etat').textContent = `${f.name} : ${Math.round((100 * p.loaded) / p.total)} %`; };
    x.onload = () => {
      let v = {};
      try { v = JSON.parse(x.responseText); } catch { /* réponse vide */ }
      $('fichiers-etat').textContent = x.status === 200 ? `${v.nom} déposé dans ~/Downloads/web_bureau` : `${f.name} : ${v.detail || 'refusé'}`;
      suivant();
    };
    x.onerror = () => { $('fichiers-etat').textContent = `${f.name} : envoi interrompu`; suivant(); };
    x.send(f);
  };
  suivant();
});

// -- infos et diagnostic --------------------------------------------------------------------------

async function diagnostic() {
  const zone = $('infos-diagnostic');
  zone.textContent = 'Diagnostic en cours (essai des encodeurs)…';
  try {
    const r = await fetch('/api/diagnostic', entete);
    const d = await r.json();
    const enc = (e) => (e.ok ? `oui, ${e.ms_par_image} ms par image` : `non (${e.erreur})`);
    zone.textContent = [
      `web_bureau ${d.version}`,
      `Enregistrement de l'écran : ${d.permissions.ecran ? 'accordé' : 'À ACCORDER sur le Mac'}`,
      `Accessibilité : ${d.permissions.accessibilite ? 'accordée' : 'À ACCORDER sur le Mac'}`,
      `Écran ${d.ecran.virtuel ? 'virtuel' : 'principal'} : ${d.ecran.pixels[0]} × ${d.ecran.pixels[1]} pixels`,
      `Encodeur H.264 : ${enc(d.encodeurs.h264)}`,
      `Encodeur HEVC : ${enc(d.encodeurs.hevc)}`,
      `Déverrouillage depuis l'iPad : ${d.verrouillage.deverrouillage_verifie ? 'vérifié' : 'non vérifié'}`,
    ].join('\n');
  } catch {
    zone.textContent = 'Diagnostic impossible.';
  }
}

$('essai-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const mdp = $('essai-mdp').value;
  $('essai-mdp').value = '';
  if (!$('essai-devant').checked) { $('essai-etat').textContent = 'Cochez la case : à faire devant le Mac.'; return; }
  $('essai-etat').textContent = 'Verrouillage du Mac, puis saisie du mot de passe…';
  try {
    const r = await fetch('/api/diagnostic/deverrouillage', {
      ...entete, method: 'POST', headers: { ...ENTETE_WEB, 'Content-Type': 'application/json' }, body: JSON.stringify({ mdp }),
    });
    const v = await r.json();
    $('essai-etat').textContent = v.message || v.detail || 'Réponse inattendue';
  } catch {
    $('essai-etat').textContent = 'Essai impossible.';
  }
});

// -- départ -------------------------------------------------------------------------------------------

(async () => {
  try {
    touchesConnues = new Set(await (await fetch('/api/touches', entete)).json());
  } catch { /* sans la liste, le clavier physique reste muet */ }
  connecter();
})();
