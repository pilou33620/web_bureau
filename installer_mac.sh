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

echo "1. Lancement automatique (LaunchAgent $NOM)"
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
launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"

echo
echo "2. Énergie : jamais de veille, redémarrage après une coupure de courant (mot de passe administrateur)"
sudo pmset -a sleep 0 disksleep 0 autorestart 1 || echo "   [!] pmset refusé : réglez-le dans Réglages › Économie d'énergie."

echo
echo "3. Permissions : macOS affiche deux demandes pour « web_bureau »."
echo "   Accordez « Enregistrement de l'écran » et « Accessibilité » dans les Réglages qui s'ouvrent."
open "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture" 2>/dev/null || true
sleep 2
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility" 2>/dev/null || true
printf "   Appuyez sur Entrée une fois les deux permissions accordées… "
read -r _
launchctl kickstart -k "gui/$(id -u)/$NOM" # macOS ne les applique qu'au redémarrage du programme

echo
echo "4. Pour un Mac sans écran (à régler une fois dans Réglages Système) :"
echo "   - Utilisateurs et groupes › Ouverture de session automatique (FileVault doit être désactivé) ;"
echo "   - Général › Partage › Session à distance (SSH, accès de secours ; ajoutez votre clé dans ~/.ssh/authorized_keys)."

echo
echo "5. Sur l'iPad : installez la CA du Mac puis entrez le code (valable 10 min)"
for _ in $(seq 1 30); do
  grep -q "Code d'appairage" "$JOURNAL" 2>/dev/null && break
  sleep 1
done
grep -E "https://|Code d'appairage|empreinte|\[X\]|\[!\]" "$JOURNAL" || echo "   [!] Rien dans $JOURNAL : voir « Dépannage » dans README.md."
echo
echo "Journal : $JOURNAL — désinstaller : launchctl bootout gui/\$(id -u) $PLIST && rm $PLIST"
