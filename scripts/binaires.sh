#!/bin/sh
# Met l'exécutable de web_bureau dans bin/ (à inclure par « . ./scripts/binaires.sh » depuis la racine).
# Avec cargo : compilé ici. Sinon : téléchargé depuis la publication « binaires » (compilée et signée
# par la CI à chaque envoi sur main), vérifié par son empreinte SHA-256, une fois par commit.
DEPOT_BINAIRES="https://github.com/pilou33620/web_bureau/releases/download/binaires"

obtenir_binaires() {
  # copie de production (archive web_bureau-prod, sans les sources) : l'exécutable est déjà là
  if [ ! -f Cargo.toml ] && [ -x bin/web_bureau ]; then return 0; fi
  mkdir -p bin
  if command -v cargo >/dev/null 2>&1; then
    cargo build --release || return 1
    cp target/release/web_bureau bin/.neuf && mv -f bin/.neuf bin/web_bureau
    return 0
  fi
  ici=$(git rev-parse HEAD 2>/dev/null)
  if [ -x bin/web_bureau ] && [ -n "$ici" ] && [ "$(cat bin/commit 2>/dev/null)" = "$ici" ]; then
    return 0
  fi
  [ "$(uname -s)" = Darwin ] || { echo "[X] web_bureau ne fonctionne que sur macOS."; return 1; }
  archive="web_bureau-macos-aarch64.tar.gz"
  echo "  Téléchargement de web_bureau…"
  if curl -fsSL -o bin/.archive "$DEPOT_BINAIRES/$archive" \
     && curl -fsSL -o bin/.empreinte "$DEPOT_BINAIRES/$archive.sha256" \
     && [ "$(shasum -a 256 bin/.archive | cut -d' ' -f1)" = "$(cat bin/.empreinte)" ] \
     && tar -xzf bin/.archive -C bin; then
    rm -f bin/.archive bin/.empreinte
    return 0
  fi
  rm -f bin/.archive bin/.empreinte
  if [ -x bin/web_bureau ]; then
    echo "  [!] Téléchargement impossible : on garde l'exécutable déjà présent."
    return 0
  fi
  echo "[X] Pas d'exécutable et téléchargement impossible (ou empreinte fausse)."
  return 1
}
