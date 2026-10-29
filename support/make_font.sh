#!/usr/bin/env bash
# Erzeugt die eingebetteten WOFF2-Subsets von JetBrainsMono Nerd Font.
# Voraussetzung: fonttools + brotli (z. B. in einem venv). Manuell ausführen, nicht Teil des Builds.
set -euo pipefail
SRC="${NF_DIR:-$HOME/.local/share/fonts/JetBrainsMonoNerdFont}"
OUT="$(dirname "$0")/../frontend/fonts"
UNI="U+0020-007E,U+00A0-017F,U+2010-2027,U+2030-203A,U+20AC,U+2190-21FF,U+2212,U+2500-259F,U+25A0-25FF,U+2713-2717,U+2605,U+2606,U+2022,U+2026"
# Nerd-Font md-* Icons (zusätzlich zu den Zeichen oben), Liste in support/icons.txt
EXTRA=""
[ -f "$(dirname "$0")/icons.txt" ] && EXTRA=",$(grep -v '^#' "$(dirname "$0")/icons.txt" | tr -s '\n ' ',' | sed 's/,$//')"
for w in Regular Bold; do
  pyftsubset "$SRC/JetBrainsMonoNerdFont-$w.ttf" --unicodes="$UNI$EXTRA" --flavor=woff2 \
    --layout-features='' --output-file="$OUT/jbm-$w.woff2"
done
# Android: dasselbe Subset als TTF (res/font verlangt Kleinbuchstaben-Namen)
AND="$(dirname "$0")/../android/app/src/main/res/font"
mkdir -p "$AND"
for w in Regular Bold; do
  pyftsubset "$SRC/JetBrainsMonoNerdFont-$w.ttf" --unicodes="$UNI$EXTRA,U+2318,U+25B6,U+25C0,U+2715,U+2630" \
    --layout-features='' --output-file="$AND/jbm_$(echo "$w" | tr A-Z a-z).ttf"
done
ls -l "$OUT"
