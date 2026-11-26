#!/bin/sh
# Lance web_bureau à la main, dans ce terminal (essais). En service, c'est le LaunchAgent posé par
# installer_mac.sh qui le lance : les permissions macOS vont alors à web_bureau, pas au Terminal.
cd "$(dirname "$0")" || exit 2
. ./scripts/binaires.sh
obtenir_binaires || exit 3
while :; do
  ./bin/web_bureau "$@"
  code=$?
  [ "$code" -eq 75 ] || exit "$code" # 75 : mis à jour, on relance la nouvelle version
  obtenir_binaires || exit 3
done
