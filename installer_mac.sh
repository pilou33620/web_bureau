#!/bin/sh
# Installe web_bureau sur ce Mac, une fois, DEVANT le Mac (écran et clavier branchés) :
# lancement automatique à l'ouverture de session, jamais de veille, permissions, code d'appairage.
# À lancer depuis le dossier décompressé : sh installer_mac.sh
set -e
ICI=$(cd "$(dirname "$0")" && pwd)
EXE="$ICI/bin/web_bureau"
NOM=fr.pilou.web_bureau
PLIST="$HOME/Library/LaunchAgents/$NOM.plist"
JOURNAL="$HOME/Library/Logs/web_bureau.log"

[ "$(uname -s)" = Darwin ] || { echo "[X] web_bureau s'installe sur macOS."; exit 2; }
[ -x "$EXE" ] || { echo "[X] $EXE introuvable : décompressez l'archive web_bureau-prod-macos-aarch64 complète."; exit 2; }
# une archive passée par un navigateur porte l'attribut de quarantaine : Gatekeeper bloquerait le lancement
xattr -dr com.apple.quarantine "$ICI" 2>/dev/null || true

titre() { printf '\n\033[1m%s\033[0m\n' "$1"; }

printf '\n\033[1mInstallation de web_bureau\033[0m (4 étapes, environ 2 minutes)\n'

titre "Étape 1/4 · Démarrage automatique"
mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
cat > "$PLIST" <<FIN
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$NOM</string>
  <key>ProgramArguments</key>
  <array><string>$EXE</string><string>--sans-navigateur</string><string>--sans-pause</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>$JOURNAL</string>
  <key>StandardErrorPath</key><string>$JOURNAL</string>
</dict>
</plist>
FIN
: > "$JOURNAL"
chmod 600 "$JOURNAL" # il contient le code d'appairage : pas lisible par les autres comptes du Mac
launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "   ✓ web_bureau démarrera avec la session et se relancera s'il s'arrête."

titre "Étape 2/4 · Énergie"
echo "   Le Mac ne doit jamais se mettre en veille. Tapez le mot de passe du Mac (rien ne s'affiche pendant la frappe) :"
if sudo pmset -a sleep 0 disksleep 0 displaysleep 0 autorestart 1; then
  echo "   ✓ Ni veille ni extinction de l'écran ; redémarrage après une coupure de courant."
else
  echo "   [!] Refusé : réglez-le dans Réglages Système › Économie d'énergie."
fi

titre "Étape 3/4 · Deux permissions"
echo "   Les Réglages Système s'ouvrent. Activez « web_bureau » dans les deux listes :"
echo "     • Confidentialité et sécurité › Enregistrement de l'écran"
echo "     • Confidentialité et sécurité › Accessibilité"
echo "   (web_bureau absent de la liste ? bouton +, puis $EXE)"
open "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture" 2>/dev/null || true
sleep 2
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility" 2>/dev/null || true
printf "   Appuyez sur Entrée quand les deux sont activées… "
read -r _
: > "$JOURNAL" # le code du premier lancement ne vaut plus rien : seul celui de la relance doit s'afficher
# à la relance, web_bureau ouvre sa page de réglages dans le navigateur du Mac
DONNEES="$HOME/.web_tools/web_bureau"
mkdir -p "$DONNEES" && : > "$DONNEES/ouvrir-reglages"
launchctl kickstart -k "gui/$(id -u)/$NOM" # macOS n'applique les permissions qu'au redémarrage du programme
echo "   ✓ web_bureau relancé avec ses permissions."

titre "Étape 4/4 · Connecter l'iPad"
for _ in $(seq 1 30); do
  grep -q "Code d'appairage" "$JOURNAL" 2>/dev/null && break
  sleep 1
done
if grep -q "Code d'appairage" "$JOURNAL" 2>/dev/null; then
  echo "   La page « Réglages de web_bureau » vient de s'ouvrir dans le navigateur du Mac."
  echo "   Suivez « Connecter l'iPad » : trois étapes, avec des QR codes à viser avec l'appareil photo de l'iPad."
  echo
  echo "   Vous pourrez la rouvrir à tout moment : icône « web_bureau » dans la barre des menus › Réglages…"
  ADRESSE=$(grep -Eo "https://[0-9][0-9.]*:[0-9]+/" "$JOURNAL" | grep -v "^https://127\." | head -1)
  CODE=$(grep "Code d'appairage" "$JOURNAL" | tail -1 | grep -Eo "[0-9]{3} [0-9]{3}")
  echo
  echo "   Sans la page : sur l'iPad, ouvrez ${ADRESSE:-https://<adresse du Mac>:8803/} dans Safari"
  echo "   et saisissez le code ${CODE:-affiché dans le journal} (valable 10 min)."
else
  echo "   [!] web_bureau ne répond pas. Ce qu'il a écrit :"
  sed 's/^/       /' "$JOURNAL" | tail -20
  echo "       Voir « Dépannage » dans $ICI/README.md."
fi

titre "Pour laisser le Mac sans écran (à régler une fois dans Réglages Système)"
echo "   • Utilisateurs et groupes › Ouverture de session automatique (FileVault doit être désactivé)."
echo "   • Écran verrouillé : « Exiger le mot de passe après l'économiseur d'écran ou l'extinction de"
echo "     l'écran » sur Jamais, et « Lancer l'économiseur d'écran » sur Jamais. Sinon macOS verrouille"
echo "     le Mac tout seul. À remettre seulement quand le test de déverrouillage (Infos, sur l'iPad) a réussi."
echo "   • Général › Partage › Session à distance (SSH) : l'accès de secours, avec votre clé dans ~/.ssh/authorized_keys."
echo
echo "Journal : $JOURNAL — désinstaller : launchctl bootout gui/\$(id -u) $PLIST && rm $PLIST"
