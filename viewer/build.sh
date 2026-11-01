#!/usr/bin/env bash
# Builds the online viewer into viewer/dist (or $1): the reader as
# WebAssembly (wasm.rs -> wasm32-unknown-unknown, no imports), the page
# (web/), the substitute fonts (pdf.js' standard fonts, deps.json) and a
# sample PDF. Serve the folder as static files (node viewer/serve.mjs).
#
# Needs cargo with the wasm32-unknown-unknown target
# (rustup target add wasm32-unknown-unknown).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT="${1:-$HERE/dist}"
# shellcheck source=../scripts/deps.sh
source "$ROOT/scripts/deps.sh"

echo "==> reader -> WebAssembly"
(cd "$HERE" && RUSTFLAGS="${RUSTFLAGS:--Awarnings}" cargo build -q --release --target wasm32-unknown-unknown)
mkdir -p "$OUT/fonts"
cp "$HERE/target/wasm32-unknown-unknown/release/rangerpdf_viewer.wasm" "$OUT/rangerpdf.wasm"
cp "$HERE/web/"*.{html,css,js} "$OUT/"
if ensure_fonts; then
  cp "$PDF_FONTS"/*.{ttf,pfb} "$PDF_FONTS"/LICENSE_* "$OUT/fonts/"
else
  echo "warning: no standard fonts; text in fonts a PDF does not embed is not drawn" >&2
fi
node "$HERE/test/sample-pdf.mjs" "$OUT/sample.pdf"
echo "==> $OUT ($(wc -c <"$OUT/rangerpdf.wasm") bytes of WebAssembly)"
