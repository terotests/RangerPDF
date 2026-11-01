# Sourced by test.sh and viewer/build.sh: puts what the builds need under
# .deps/ at the commits deps.json pins.
#
#   .deps/Ranger         Ranger, for the es6, kotlin, dart and swift test
#                        builds (RANGER_DIR names a checkout of your own)
#   .deps/pdfjs-fonts    pdf.js' standard fonts (Liberation Sans, Foxit): the
#                        faces non-embedded fonts are drawn in. Data only; no
#                        pdf.js code is used.

_DEPS_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

dep_pin() { # <dep> <field>
  node -e 'const m=require(process.argv[1]);process.stdout.write(m[process.argv[2]][process.argv[3]]||"")' \
    "$_DEPS_ROOT/deps.json" "$1" "$2"
}

# <name> <git url> <commit> <dir> [sparse paths]
dep_sync() {
  local name="$1" url="$2" rev="$3" dir="$4" sparse="${5:-}"
  [[ "$(git -C "$dir" rev-parse -q --verify HEAD 2>/dev/null || true)" == "$rev" ]] && return 0
  echo "==> $name -> ${rev:0:7}   ($dir)"
  rm -rf "$dir"
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" remote add origin "$url"
  if [[ -n "$sparse" ]]; then
    # shellcheck disable=SC2086
    git -C "$dir" sparse-checkout set $sparse
    git -C "$dir" fetch -q --depth 1 --filter=blob:none origin "$rev" || return 1
  else
    git -C "$dir" fetch -q --depth 1 origin "$rev" || return 1
  fi
  git -C "$dir" checkout -q --detach FETCH_HEAD
}

ensure_ranger() {
  if [[ -n "${RANGER_DIR:-}" ]]; then
    RANGER="$(cd "$RANGER_DIR" && pwd)"
  else
    RANGER="$_DEPS_ROOT/.deps/Ranger"
    dep_sync Ranger "$(dep_pin ranger git)" "$(dep_pin ranger rev)" "$RANGER" || return 1
  fi
  [[ -f "$RANGER/dist/rgrc.js" ]] || { echo "error: $RANGER has no dist/rgrc.js" >&2; return 1; }
}

# Sets PDF_FONTS to the directory with the standard fonts.
ensure_fonts() {
  local dir="$_DEPS_ROOT/.deps/pdfjs-fonts"
  dep_sync "pdf.js standard fonts" "$(dep_pin pdfjs git)" "$(dep_pin pdfjs rev)" "$dir" external/standard_fonts || return 1
  PDF_FONTS="$dir/external/standard_fonts"
}
