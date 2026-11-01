#!/usr/bin/env bash
# Builds the online viewer and runs its browser test (needs Chromium; CHROME
# names one). SHOTS=<dir> keeps screenshots.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
"$HERE/build.sh" "$HERE/dist" >/dev/null
node "$HERE/test/viewer-test.mjs" "$HERE/dist" ${SHOTS:+--shots="$SHOTS"}
