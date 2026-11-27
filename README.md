# web_bureau

Prendre le contrôle du Mac (pensé pour un Mac mini sans écran) depuis l'iPad, dans Safari, sur le
réseau local. Le Mac capture son écran (ScreenCaptureKit), l'encode avec sa puce (VideoToolbox,
HEVC ou H.264, faible latence) et l'envoie par UDP (WebTransport) si Safari le permet, sinon par un
WebSocket chiffré ; Safari le décode (WebCodecs) et renvoie souris, clavier, Pencil et gestes.

Cahier des charges : `~/Documents/temp/web_bureau/cahier-des-charges.html`.

## Installation sur le Mac (une fois, devant le Mac)

Il faut un écran et un clavier branchés le temps de l'installation : macOS demande deux permissions
et affiche le code d'appairage.

```sh
cd ~ && curl -fsSL https://github.com/pilou33620/web_bureau/releases/download/binaires/web_bureau-prod-macos-aarch64.tar.gz | tar xz
sh ~/web_bureau/installer_mac.sh
```

L'installateur :

1. pose un LaunchAgent (`~/Library/LaunchAgents/fr.pilou.web_bureau.plist`) qui lance web_bureau à
   l'ouverture de session et le relance s'il s'arrête ;
2. règle l'énergie : ni veille ni extinction de l'écran, redémarrage après une coupure de courant
   (`pmset`, mot de passe administrateur) ;
3. ouvre les Réglages pour **Enregistrement de l'écran** et **Accessibilité** (à accorder à
   « web_bureau ») ;
4. affiche les adresses, l'empreinte de la CA et le **code d'appairage**.

À régler à la main pour un Mac sans écran :

- **Ouverture de session automatique** (Utilisateurs et groupes). macOS l'interdit tant que FileVault
  est actif.
- **Écran verrouillé** : « Exiger le mot de passe après l'économiseur d'écran ou l'extinction de
  l'écran » sur *Jamais*, et pas d'économiseur d'écran. Sinon macOS verrouille le Mac tout seul, que
  web_bureau le veuille ou non. À remettre seulement quand le test de déverrouillage (Infos, sur
  l'iPad) a réussi.
- **Session à distance (SSH)**, avec une clé seulement : c'est l'accès de secours si web_bureau ne
  répond plus.

Ensuite, débranchez l'écran : web_bureau crée un **écran virtuel** aux proportions de l'iPad.

## Sur l'iPad

1. Ouvrez `https://<IP du Mac>:8803/appairage/ca.pem` et installez le profil (Réglages › Profil
   téléchargé), puis activez-le dans Réglages › Général › Informations › Réglages des certificats.
   Comparez l'empreinte avec celle de l'installateur.
2. Ouvrez `https://<IP du Mac>:8803/` et saisissez le code à 6 chiffres. Le cookie dure un an.
3. Partager › **Sur l'écran d'accueil** : en plein écran, sans barre d'adresse, et Safari ne prend
   plus ⌘T, ⌘W, ⌘L ni ⌘R. Coupez aussi les gestes de multitâche à 4-5 doigts
   (Réglages › Multitâche et gestes).

## Utilisation

| Geste | Effet sur le Mac |
|---|---|
| Toucher | clic gauche à l'endroit touché |
| Double toucher | double clic |
| Un doigt qui glisse | glisser, bouton gauche tenu (déplacer une fenêtre, sélectionner du texte) |
| Appui long immobile (0,65 s) | clic droit |
| Toucher à 2 doigts | clic droit |
| Glisser à 2 doigts | défilement (avec inertie) |
| Pincer | zoom de la vue sur l'iPad (« 1:1 » pour revenir) |
| Toucher à 3 doigts | ouvre ou ferme le clavier à l'écran |

- **Magic Keyboard et trackpad** : touches, clic, clic droit à deux doigts, défilement. ⌘⇥, ⌘Espace
  et ⌘H sont pris par iPadOS : utilisez les boutons de la barre (⌘⇥, ▦ Mission Control, ⌕ Spotlight).
- **Apple Pencil** : pointage direct. Le survol (iPad M4, Pencil Pro/USB-C) déplace le curseur.
- **Presse-papiers** : ⌘C sur le clavier de l'iPad copie aussi dans l'iPad ; ⌘V colle le texte de
  l'iPad sur le Mac. Les boutons Copier et Coller font de même au tactile.
- **Fichiers** (📁) : parcourir le dossier personnel du Mac et télécharger, ou envoyer des fichiers
  dans `~/Downloads/web_bureau` (2 Go au plus par fichier).
- **Réglages** (⚙︎), ceux de Moonlight, gardés sur l'iPad :
  - résolution : l'écran de l'iPad (par défaut), 720p, 1080p, 1440p ou 4K. 720p et 1440p donnent un
    bureau de 1280 × 720 points, 1080p et 4K un bureau de 1920 × 1080 points ; 1440p et 4K sont en
    Retina ;
  - images par seconde : 30, 60 ou 120. 120 ne se voit que sur un iPad à écran 120 Hz (iPad Pro), et
    seulement après avoir coupé « Prefer Page Rendering Updates near 60fps » dans Réglages › Apps ›
    Safari › Avancé › Feature Flags ; l'écran virtuel du Mac passe alors à 120 Hz ;
  - débit : automatique (celui que Moonlight conseille : 10 Mb/s en 720p, 20 en 1080p, 40 en 1440p,
    80 en 4K, environ 43 pour l'écran d'un iPad Pro 11 pouces et 57 pour un 13 pouces, à 60 images
    par seconde ; la moitié à 30, 1,4 fois plus à 120), ou de 0,5 à 150 Mb/s. C'est un plafond : le débit baisse seul si le Wi-Fi ne suit pas.
- **Mesures** (📈) : codec, UDP ou TCP, taille, images/s, débit, latence de la capture à l'écran.
- **Vidéo par UDP** : rien à régler. Si Safari a WebTransport (iPadOS 26.4 ou plus récent), la vidéo
  passe par UDP (QUIC), une image par flux : un paquet perdu ne retient que son image, et si elle
  tarde plus de 50 ms la page demande une image qui se décode seule. Sinon, ou si l'UDP est filtré,
  la vidéo passe par le WebSocket comme avant. Souris, clavier et le reste gardent le WebSocket.
- **Infos** (ⓘ) : permissions, écran capturé, temps d'encodage, test du déverrouillage.

## Sécurité

Un appareil appairé contrôle **tout** le Mac. Ce qui protège l'accès :

- un **jeton propre** à web_bureau (`~/.web_tools/web_bureau/jeton.txt`, 0600) et un **cookie propre**
  (`web_bureau_jeton`) : un appareil appairé à un autre web tool n'entre pas ;
- le **code d'appairage** à 6 chiffres, affiché seulement sur le Mac : usage unique, 10 min, 5
  erreurs puis pause croissante ;
- **aucune exemption pour le Mac lui-même** (écart voulu avec les autres web tools) : un autre compte
  ou un programme du Mac qui passe par `127.0.0.1` doit lui aussi présenter le jeton ;
- **HTTPS obligatoire**, avec la CA propre à web_bureau (`~/.web_tools/web_bureau/tls/`) : sans
  openssl, web_bureau refuse le mode réseau au lieu de passer en clair ;
- les adresses publiques sont refusées (réseau local et Tailscale seulement), et les en-têtes `Host`
  et `Origin` sont vérifiés (DNS rebinding, WebSocket d'un autre site) ;
- les écritures exigent l'en-tête `X-WEB_BUREAU` (protection CSRF) ;
- le **port UDP** (même numéro) ne porte que la vidéo, du Mac vers l'iPad, sous le même certificat.
  WebTransport n'envoie pas le cookie : la session du WebSocket, déjà authentifiée, donne à la page
  un billet aléatoire à usage unique, périmé avec elle. Sans billet valide, ou depuis une adresse
  publique, la liaison est refusée ;
- chaque message de l'iPad est validé : position dans l'image, touche connue, tailles bornées. Un
  message invalide ferme la session ;
- **icône dans la barre des menus** pendant une session, avec un menu « Couper la session » ;
- **fin de session après 15 min sans geste**. Le Mac se verrouille alors aussi, si le
  déverrouillage depuis l'iPad a été vérifié (Infos › Tester, devant le Mac). Sinon, la session se
  termine sans verrouiller, pour ne jamais enfermer un Mac sans écran ;
- jamais de jeton, de mot de passe, de texte copié ni de frappe dans le journal.

Retirer l'accès à tous les appareils : `~/web_bureau/bin/web_bureau --nouveau-jeton` (la commande
retire le jeton et sort), puis relancer le service :
`launchctl kickstart -k gui/$(id -u)/fr.pilou.web_bureau`. Le journal donne alors un nouveau code.

## Mises à jour

Au démarrage, puis une fois par jour quand aucune session n'est ouverte, web_bureau compare l'empreinte de la publication « binaires » à la sienne. Si une
nouvelle version existe, il la télécharge, vérifie son SHA-256, la pose, puis sort avec le code 75 ;
launchd le relance. Une nouvelle version qui échoue 3 fois au démarrage est remplacée par la
précédente (`bin/web_bureau.precedent`). La CI signe chaque version avec le même certificat : macOS
garde donc les permissions.

## Dépannage

- Journal : `~/Library/Logs/web_bureau.log` (par SSH : `tail -n 30 ~/Library/Logs/web_bureau.log`,
  qui montre aussi un nouveau code d'appairage).
- Relancer : `launchctl kickstart -k gui/$(id -u)/fr.pilou.web_bureau`.
- Désinstaller : `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/fr.pilou.web_bureau.plist`,
  puis supprimer ce fichier et `~/web_bureau`.
- Écran noir ou « écran introuvable » : regardez Infos. Si l'écran virtuel est impossible (une version
  de macOS a retiré l'API privée `CGVirtualDisplay`), branchez un bouchon HDMI factice et relancez
  avec `--sans-ecran-virtuel`.

## Options

```
web_bureau [--port N] [--local] [--sans-maj] [--sans-ecran-virtuel] [--nouveau-jeton]
           [--sans-navigateur] [--sans-pause]
```

Port 8803 par défaut (le suivant s'il est pris), en TCP et en UDP : si le pare-feu de macOS est
actif, il doit laisser entrer `web_bureau`. `--local` : 127.0.0.1 seulement et **sans jeton**, pour
les essais ; jamais en service (et sans vidéo par UDP).

## Limites connues

- **Écran virtuel** : `CGVirtualDisplay` est une API privée d'Apple (BetterDisplay et DeskPad
  l'emploient), qu'une mise à jour de macOS peut retirer. Repli : un bouchon HDMI factice.
- **Déverrouillage à distance** : macOS peut refuser la saisie simulée sur l'écran verrouillé. Le
  test de la page Infos le dit ; tant qu'il n'a pas réussi, web_bureau ne verrouille jamais le Mac.
- **Rappel de macOS 15 et suivants** : macOS redemande de temps en temps de confirmer
  l'enregistrement de l'écran. La fenêtre s'affiche sur l'écran du Mac, donc dans le flux, et
  s'accepte depuis l'iPad.
- **Raccourcis système de l'iPad** (⌘⇥, ⌘Espace…) : iPadOS les prend avant la page.
- **Claviers ISO** : si `@` et `<` sont inversés sur le Mac, voir le commentaire `shortcut:` de
  `rust/bureau/src/clavier.rs`.
- Pas de son, une seule session à la fois, réseau local seulement (pour l'instant).

## Développement et tests

- Le code s'écrit sur le Pi, qui ne compile pas. La CI GitHub (macOS, Apple Silicon) lance les tests
  (`cargo test`, `node --test tests/*.test.js`) et publie l'exécutable signé.
- `Cargo.lock` est suivi par git et la CI compile avec `--locked` : les bibliothèques ne changent que
  quand on le décide. Pour tout mettre à jour : `gh workflow run verrou.yml`, puis `gh run download`
  de l'artefact `Cargo.lock`, à relire et committer.
- Tests : protocole, trames WebSocket, régulation du débit, vidéo par UDP (un vrai client
  WebTransport : billet, une image par flux, fermeture), ordre des images, fichiers (confinement, liens
  symboliques, envoi coupé), retour arrière des mises à jour, réglages abîmés, refus du serveur
  (CSRF, tailles, Origin), sécurité de l'appairage (tests recopiés de la caisse commune, HTTPS
  compris), et un test par geste.
- Ce qui exige les permissions macOS (capture, encodage réel, souris) se vérifie sur le Mac par
  Infos › diagnostic.
- `unsafe` n'existe que dans `rust/bureau/src/macos/` (`#![deny(unsafe_code)]` ailleurs).
- `rust/bureau/src/commun/` est une **copie** de la caisse `web_tools_commun` (dépôt web_suite),
  rendue indépendante (jeton, cookie et CA propres, `TCP_NODELAY`). Une correction de sécurité de
  la caisse est à reporter ici à la main.
- Certificat de signature : les secrets `MAC_CERT_P12` (PKCS#12 en base64) et `MAC_CERT_MDP` du dépôt.
  Sans eux, la CI signe en ad hoc et macOS redemande les permissions à chaque version.
