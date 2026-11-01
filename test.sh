#!/usr/bin/env bash
# Builds the test runner (src/main.rs, Rust-syntax Ranger) for each target and
# runs it. Its last line is `ALL PASS <n>` when every test passed.
#
#   ./test.sh                  rust es6 kotlin dart swift (those installed)
#   ./test.sh rust es6         some targets
#   ./test.sh viewer           the online viewer (viewer/test.sh)
#
# rust is plain rustc on src/main.rs; the others go through Ranger
# (deps.json pins it; RANGER_DIR names a checkout of your own). Swift: set
# SWIFT_RUN to a command that runs a .swift file when swiftc is not on PATH.
# Each target's output is kept in parity/results/<target>.txt.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/deps.sh
source "$ROOT/scripts/deps.sh"

TARGETS=("$@")
[[ ${#TARGETS[@]} -eq 0 ]] && TARGETS=(rust es6 kotlin dart swift)
have() { command -v "$1" >/dev/null 2>&1; }
SRC="$ROOT/src"
OUT="$ROOT/tmp"
RES="$ROOT/parity/results"
mkdir -p "$OUT" "$RES"
rgrc() { (cd "$SRC" && NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=8192}" node "$RANGER/dist/rgrc.js" -l="$1" main.rs -d="$OUT/$1" -o="$2" >"$OUT/$1.log" 2>&1) ||
  { grep -E '^\[FAIL|rror' "$OUT/$1.log" | head -20; return 1; }; }
need_ranger() { [[ -n "${RANGER:-}" ]] || ensure_ranger; }

failed=0
for t in "${TARGETS[@]}"; do
  log="$OUT/$t.out"
  case "$t" in
    rust)
      have rustc || { echo "skip rust: no rustc"; continue; }
      rustc --edition 2021 -O -o "$OUT/core" "$SRC/main.rs" 2>"$OUT/rust.log" || { cat "$OUT/rust.log"; failed=1; continue; }
      "$OUT/core" >"$log" ;;
    es6)
      need_ranger && rgrc es6 core.js || { failed=1; continue; }
      node "$OUT/es6/core.js" >"$log" ;;
    kotlin)
      { have kotlinc && have java; } || { echo "skip kotlin: no kotlinc"; continue; }
      need_ranger && rgrc kotlin core.kt || { failed=1; continue; }
      JAVA_OPTS="${JAVA_OPTS:--Xmx8g -Xss8m}" kotlinc "$OUT/kotlin/core.kt" -include-runtime -d "$OUT/kotlin/core.jar" 2>"$OUT/kotlinc.log" || { grep error "$OUT/kotlinc.log"; failed=1; continue; }
      java -Dfile.encoding=UTF-8 -jar "$OUT/kotlin/core.jar" 2>/dev/null >"$log" ;;
    dart)
      have dart || { echo "skip dart: no dart"; continue; }
      need_ranger && rgrc dart core.dart || { failed=1; continue; }
      dart run "$OUT/dart/core.dart" >"$log" ;;
    swift)
      { [[ -n "${SWIFT_RUN:-}" ]] || have swiftc; } || { echo "skip swift: no swiftc (or SWIFT_RUN)"; continue; }
      need_ranger && rgrc swift6 core.swift || { failed=1; continue; }
      if [[ -n "${SWIFT_RUN:-}" ]]; then
        $SWIFT_RUN "$OUT/swift6/core.swift" >"$log"
      else
        swiftc -O -o "$OUT/swift6/core" "$OUT/swift6/core.swift" && "$OUT/swift6/core" >"$log"
      fi ;;
    viewer)
      "$ROOT/viewer/test.sh" || failed=1
      continue ;;
    *) echo "unknown target $t"; failed=1; continue ;;
  esac
  cp "$log" "$RES/$t.txt"
  last="$(tail -1 "$log")"
  echo "pdf $t: $last"
  [[ "$last" == ALL\ PASS* ]] || failed=1
done
exit $failed
