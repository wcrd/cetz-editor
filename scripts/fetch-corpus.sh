#!/usr/bin/env bash
# Fetch the external diagram corpus into corpus/ (gitignored) at pinned
# commits, only its diagrams' sources (a sparse, blobless checkout), then compile each diagram once with the Typst CLI so every package
# it imports lands in the local package cache the corpus eval reads from.
#
# Usage: scripts/fetch-corpus.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
corpus="$root/corpus"

# name  repo  commit  files
sources=(
  "janosh https://github.com/janosh/diagrams 73e88e8846c03a0fbec4122678274ae6b6ed62cc /assets/*/*.typ"
)

for source in "${sources[@]}"; do
  read -r name repo rev files <<<"$source"
  dir="$corpus/$name"
  if [[ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" != "$rev" ]]; then
    rm -rf "$dir"
    git init -q "$dir"
    git -C "$dir" sparse-checkout set --no-cone "$files"
    git -C "$dir" fetch -q --depth 1 --filter=blob:none "$repo" "$rev"
    git -C "$dir" checkout -q FETCH_HEAD
  fi
  echo "$name  $rev"
done

# Warm the package cache. A diagram that fails here fails in the eval too,
# which reports it; that's not a reason to stop.
command -v typst >/dev/null || { echo "error: 'typst' not found on PATH" >&2; exit 1; }
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
find "$corpus" -name '*.typ' -not -path '*/.git/*' -print0 |
  xargs -0 -P 8 -I{} sh -c 'typst compile --root "$(dirname "$1")" "$1" "$2/$(basename "$1" .typ).pdf" 2>/dev/null || echo "typst failed: $1"' _ {} "$out"
echo "packages cached"
