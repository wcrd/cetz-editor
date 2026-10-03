#!/usr/bin/env bash
# Render fixtures/*.typ and fixtures/*.tikz to SVG and PNG in generated/.
#
# Usage: scripts/render-fixtures.sh [name ...]
#   With no names, renders every fixture. A name is a fixture basename
#   without extension (e.g. zone_diagram) and renders both its .typ and
#   .tikz versions if present.
#
# Env: PPI  PNG resolution (default 200)
set -euo pipefail
shopt -s nullglob

root="$(cd "$(dirname "$0")/.." && pwd)"
fixtures="$root/fixtures"
out="$root/generated"
ppi="${PPI:-200}"

# TikZ fixtures are bare tikzpicture fragments; wrap them in a standalone doc.
tikz_preamble='\documentclass[tikz,border=2pt]{standalone}
\usetikzlibrary{arrows.meta,calc,positioning,shapes}
\begin{document}
\input{fixture.tikz}
\end{document}'

need() {
  for cmd in "$@"; do
    command -v "$cmd" >/dev/null || { echo "error: '$cmd' not found on PATH" >&2; exit 1; }
  done
}

render_typ() {
  local src="$1" name
  name="$(basename "$src" .typ)"
  need typst
  # Explicit returns: set -e is ignored inside functions called with ||.
  typst compile "$src" "$out/$name.cetz.svg" || return 1
  typst compile --ppi "$ppi" "$src" "$out/$name.cetz.png" || return 1
  echo "cetz  $name"
}

render_tikz() {
  local src="$1" name tmp
  name="$(basename "$src" .tikz)"
  need latex pdflatex dvisvgm gs
  tmp="$(mktemp -d)"
  cp "$src" "$tmp/fixture.tikz"
  printf '%s\n' "$tikz_preamble" > "$tmp/doc.tex"
  for engine in latex pdflatex; do
    if ! (cd "$tmp" && "$engine" -interaction=nonstopmode -halt-on-error doc.tex >/dev/null); then
      echo "error: $engine failed on $src" >&2
      grep -A3 '^!' "$tmp/doc.log" >&2 || true
      rm -rf "$tmp"
      return 1
    fi
  done
  # DVI route: dvisvgm --pdf needs a Ghostscript library it often can't find.
  dvisvgm --no-fonts --verbosity=1 "$tmp/doc.dvi" -o "$out/$name.tikz.svg" \
    && gs -q -dSAFER -dBATCH -dNOPAUSE -sDEVICE=pngalpha -r"$ppi" \
      -o "$out/$name.tikz.png" "$tmp/doc.pdf" \
    || { rm -rf "$tmp"; return 1; }
  rm -rf "$tmp"
  echo "tikz  $name"
}

mkdir -p "$out"

if [ $# -eq 0 ]; then
  sources=("$fixtures"/*.typ "$fixtures"/*.tikz)
else
  sources=()
  for name in "$@"; do
    matches=()
    for f in "$fixtures/$name".typ "$fixtures/$name".tikz; do
      [ -e "$f" ] && matches+=("$f")
    done
    [ ${#matches[@]} -gt 0 ] || { echo "error: no fixture named '$name'" >&2; exit 1; }
    sources+=("${matches[@]}")
  done
fi

[ ${#sources[@]} -gt 0 ] || { echo "no fixtures found in $fixtures" >&2; exit 1; }

status=0
for src in "${sources[@]}"; do
  case "$src" in
    *.typ)  render_typ "$src"  || status=1 ;;
    *.tikz) render_tikz "$src" || status=1 ;;
  esac
done
exit $status
