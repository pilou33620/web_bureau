// Réglages de web_bureau, dans le navigateur du Mac : connecter un iPad, état, réglages, journal.
// Tout texte reçu passe par textContent (jamais innerHTML).
'use strict';

const $ = (id) => document.getElementById(id);
const LECTURE = { credentials: 'same-origin', cache: 'no-store' };
let dernier = null; // le dernier état reçu
let qrVus = {}; // l'adresse de chaque QR code affiché : ne la recharger que si elle change

async function poster(chemin, corps = {}) {
  const r = await fetch(chemin, {
    method: 'POST', ...LECTURE,
    headers: { 'X-WEB_BUREAU': '1', 'Content-Type': 'application/json' },
    body: JSON.stringify(corps),
  });
  const d = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(d.detail || `refusé (${r.status})`);
  return d;
}

function alerte(texte) {
  $('alerte').textContent = texte || '';
  $('alerte').hidden = !texte;
}

function oui(element, vrai, texteOui, texteNon) {
  element.textContent = vrai ? texteOui : texteNon;
  element.className = vrai ? 'oui' : 'non';
}

function qr(img, adresse) {
  if (qrVus[img.id] === adresse) return;
  qrVus[img.id] = adresse;
  img.src = adresse;
}

function afficher(d) {
  dernier = d;
  $('version').textContent = `version ${d.version} · port ${d.port}`;

  // connecter l'iPad
  const choix = $('adresse');
  const avant = choix.value;
  if ([...choix.options].map((o) => o.value).join() !== d.adresses.join()) {
    choix.replaceChildren(...d.adresses.map((a) => new Option(a, a)));
    if (d.adresses.includes(avant)) choix.value = avant;
  }
  const ip = choix.value;
  const hote = ip.includes(':') ? `[${ip}]` : ip;
  if (!ip) {
    alerte('Le Mac n\'a pas d\'adresse sur un réseau local : branchez-le (Ethernet) ou connectez-le au Wi-Fi.');
  }
  $('empreinte').textContent = d.empreinte || 'certificat absent';
  $('url-ca').textContent = `http://${hote}:${d.port}/appairage/ca.pem`;
  $('url-page').textContent = `https://${hote}:${d.port}/`;
  qr($('qr-ca'), `/mac/qr.svg?quoi=ca&ip=${encodeURIComponent(ip)}`);
  // le code change : le QR code d'appairage aussi (le serveur y met le code valable)
  qr($('qr-appairage'), `/mac/qr.svg?quoi=appairage&ip=${encodeURIComponent(ip)}&c=${d.code ? d.code.code.replace(' ', '') : 0}`);
  if (d.code) {
    $('code').textContent = d.code.code;
    const min = Math.ceil(d.code.reste / 60);
    $('code-reste').textContent = `Valable encore ${min} min, une seule fois. Un nouveau code le remplace ensuite.`;
  } else {
    $('code').textContent = '––– –––';
    $('code-reste').textContent = d.attente
      ? `Trop d'essais faux : nouveau code dans ${d.attente} s.`
      : (d.https ? '' : 'Pas de code : web_bureau tourne sans appairage (essais en local).');
  }

  // état
  oui($('perm-ecran'), d.permissions.ecran, 'accordée', 'à accorder');
  oui($('perm-accessibilite'), d.permissions.accessibilite, 'accordée', 'à accorder');
  document.querySelector('[data-permission="ecran"]').hidden = d.permissions.ecran;
  document.querySelector('[data-permission="accessibilite"]').hidden = d.permissions.accessibilite;
  $('aide-permissions').hidden = d.permissions.ecran && d.permissions.accessibilite;
  $('ecran').textContent = `${d.ecran.virtuel ? 'virtuel' : 'principal'}, ${d.ecran.pixels[0]} × ${d.ecran.pixels[1]} pixels`;
  $('udp').textContent = d.udp ? 'prête (l\'iPad s\'y branche si Safari le permet)' : 'indisponible : la vidéo passe par le WebSocket';
  $('session').textContent = d.session ? `iPad ${d.session} connecté` : 'aucune';
  $('couper').hidden = !d.session;

  // réglages (pas pendant que l'on s'en sert)
  const inactivite = $('inactivite');
  if (!inactivite.options.length) {
    inactivite.replaceChildren(...d.reglages.inactivites.map((m) => new Option(m < 60 ? `${m} min` : `${m / 60} h`, m)));
  }
  if (document.activeElement !== inactivite) inactivite.value = d.reglages.inactivite_min;
  if (document.activeElement !== $('maj')) $('maj').checked = d.reglages.maj_auto;
}

async function rafraichir() {
  try {
    const r = await fetch('/mac/etat', LECTURE);
    if (r.status === 403) {
      alerte('Accès expiré (web_bureau a redémarré, ou lien périmé) : rouvrez les réglages par l\'icône web_bureau de la barre des menus › Réglages…');
      return;
    }
    afficher(await r.json());
    if ($('alerte').textContent.startsWith('web_bureau') || $('alerte').textContent.startsWith('Accès')) alerte('');
  } catch {
    alerte('web_bureau ne répond pas (il redémarre peut-être)…');
  }
}

async function lireJournal() {
  const zone = $('journal-texte');
  try {
    const r = await fetch('/mac/journal', LECTURE);
    if (!r.ok) return;
    const texte = await r.text();
    if (texte === zone.textContent) return;
    const enBas = zone.scrollHeight - zone.scrollTop - zone.clientHeight < 40;
    zone.textContent = texte;
    if (enBas) zone.scrollTop = zone.scrollHeight;
  } catch { /* le serveur redémarre : l'alerte le dit déjà */ }
}

function dire(texte) { $('reglages-etat').textContent = texte; }

async function agir(chemin, corps, apres) {
  try {
    await poster(chemin, corps);
    if (apres) dire(apres);
    rafraichir();
  } catch (e) {
    dire(`Refusé : ${e.message}`);
  }
}

$('adresse').addEventListener('change', () => dernier && afficher(dernier));
$('inactivite').addEventListener('change', (e) => agir('/mac/reglages', { inactivite_min: +e.target.value }, 'Enregistré.'));
$('maj').addEventListener('change', (e) => agir('/mac/reglages', { maj_auto: e.target.checked }, 'Enregistré.'));
$('couper').addEventListener('click', () => agir('/mac/couper'));
document.querySelectorAll('[data-permission]').forEach((b) => b.addEventListener('click', () => agir('/mac/permission', { quoi: b.dataset.permission })));
const relancer = () => agir('/mac/relancer', {}, 'web_bureau redémarre ; ces réglages se rouvrent dans un nouvel onglet.');
$('relancer').addEventListener('click', relancer);
$('relancer-perm').addEventListener('click', relancer);
$('revoquer').addEventListener('click', () => {
  if (!confirm('Retirer l\'accès à tous les appareils ? Chaque iPad devra être appairé à nouveau.')) return;
  agir('/mac/revoquer', {}, 'Accès retirés. web_bureau redémarre ; ces réglages se rouvrent dans un nouvel onglet.');
});

rafraichir();
lireJournal();
setInterval(rafraichir, 2000);
setInterval(() => { if (!document.hidden) lireJournal(); }, 2000);
