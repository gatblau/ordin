#!/usr/bin/env bash
# make journeys-windows: build one commit on the Windows build machine with the test-only `journeys`
# switch and run the journeys (apps/desktop/journeys) against it, from the computer that runs the tests.
#
#   COMMIT=<commit>        the commit to build and run; default HEAD. It must be in the repository the
#                          Windows machine fetches from (its git remote JOURNEYS_WINDOWS_REMOTE).
#   JOURNEYS_WINDOWS_SSH   how to reach the Windows machine; default "ssh -p 2222 ordin@127.0.0.1".
#   JOURNEYS_WINDOWS_REPO  the build checkout on that machine, in Git Bash's form; default /c/o.
#   JOURNEYS_WINDOWS_REMOTE  the git remote of that checkout to fetch from; default "server".
#   GREP=<text>            only the journeys whose title holds it (Playwright's --grep).
#   MINUTES=0              build without `minutes` (it is on by default, OrdinDesktop.md DSK-MIN-6) and run
#                          journey 13's other half: no Minutes row, Record button or Meeting minutes settings
#                          (PB-MIN-2). Records no note. By default the program has `minutes`, journey 13 runs
#                          its minutes half, and the fake OBS is built beside the program.
#
# The Windows machine needs, once: Git for Windows, Rust with the MSVC toolchain, Node.js 22.5 or later,
# the Tauri CLI (`cargo install tauri-cli`), Claude Code installed with npm, and a person signed in to its
# desktop (the program and its message boxes run there). Nothing the journeys do leaves that machine: the
# model and the online services are stand-ins on 127.0.0.1 (apps/desktop/journeys/support/stand-ins.ts).
#
# The report lands in target/journeys/<commit>/ here: results.json, the HTML report, and the output.
# When every journey passed (and GREP is unset), a note on the commit in refs/notes/ordin-journeys says so;
# the release build requires it (docs/spec/ordin-desktop/phase-5-playbook.md PB-REL-27, PB-REL-28).
# The exit status is the journeys' own.
set -euo pipefail
. "$(dirname "$0")/release/lib.sh"
load_conf

COMMIT=$(git -C "$ROOT" rev-parse --verify "${COMMIT:-HEAD}^{commit}")
read -r -a WIN <<< "${JOURNEYS_WINDOWS_SSH:-ssh -p 2222 ordin@127.0.0.1}"
REPO="${JOURNEYS_WINDOWS_REPO:-/c/o}"
REMOTE="${JOURNEYS_WINDOWS_REMOTE:-server}"
# The release's own switches, as the default build has them, plus `journeys`.
BUILD_FLAGS="--features journeys"
MINUTES_FLAG=1
if [ "${MINUTES:-1}" = 0 ]; then BUILD_FLAGS="--no-default-features --features journeys,team-knowledge"; MINUTES_FLAG=0; fi
OUT="$ROOT/target/journeys/$COMMIT"
SOURCE="${RELEASE_BUNDLED_AGENTS_SOURCE:-}"
case "$SOURCE" in ""|/*) ;; *) SOURCE="$ROOT/$SOURCE" ;; esac
[ -n "$SOURCE" ] && [ -n "${RELEASE_BUNDLED_AGENTS_COMMIT:-}" ] || die "release/release.conf names no bundled agents"
require_models_input

# Runs a Git Bash script on the Windows machine (its SSH shell is PowerShell).
# PowerShell splits a double-quoted run of words, and an apostrophe ends the quoting, so the scripts
# below quote nothing that holds a space and contain no apostrophe.
win_bash() {
  "${WIN[@]}" "& 'C:\\Program Files\\Git\\bin\\bash.exe' -lc '$(printf %s "$1" | sed "s/'/''/g")'"
}

# The bundled agents at the commit of the release, as release-build copies them (PB-REL-7). The Windows
# machine may read only this repository from this computer, so the branches of RELEASE_BUNDLED_AGENTS_SOURCE
# are copied here under refs/journeys/agents/, and the machine takes the commit from there.
git -C "$ROOT" fetch -q "$SOURCE" '+HEAD:refs/journeys/agents/HEAD' '+refs/heads/*:refs/journeys/agents/heads/*' \
  || die "could not read the bundled agents from $SOURCE"
git -C "$ROOT" cat-file -e "$RELEASE_BUNDLED_AGENTS_COMMIT^{commit}" 2>/dev/null \
  || die "the bundled agents commit $RELEASE_BUNDLED_AGENTS_COMMIT is on no branch of $SOURCE; give it one there"

note "journeys: building $COMMIT on the Windows machine"
win_bash "set -euo pipefail
cd '$REPO'
git fetch -q '$REMOTE' '+refs/heads/*:refs/remotes/$REMOTE/*' '+refs/journeys/*:refs/journeys/*'
git checkout -q -f '$COMMIT'
git clean -fdq -e node_modules -e target
rm -rf apps/desktop/catalogue && mkdir -p apps/desktop/catalogue
git archive '$RELEASE_BUNDLED_AGENTS_COMMIT' | tar -x -C apps/desktop/catalogue
(cd apps/desktop/ui && npm ci --no-audit --no-fund >/dev/null)
export ORDIN_MEMORY_MODELS_URL='$RELEASE_MEMORY_MODELS_URL' ORDIN_MEMORY_MODELS_SHA256='$RELEASE_MEMORY_MODELS_SHA256'
export ORDIN_MEMORY_MODELS_BYTES='$RELEASE_MEMORY_MODELS_BYTES' ORDIN_MEMORY_MODELS_MANIFEST='$MODELS_MANIFEST'
make desktop-build-inputs >/dev/null
# whisper.cpp built with optimisation, as the Makefile does on Windows (plan 07 FU-020).
export CMAKE_C_FLAGS_RELEASE=-MD\\ -O2\\ -Ob2\\ -DNDEBUG CMAKE_CXX_FLAGS_RELEASE=-MD\\ -O2\\ -Ob2\\ -DNDEBUG
(cd apps/desktop && cargo tauri build --no-bundle $BUILD_FLAGS >/dev/null 2>&1) || (cd apps/desktop && cargo tauri build --no-bundle $BUILD_FLAGS 2>&1 | tail -40; exit 1)
if [ $MINUTES_FLAG = 1 ]; then cargo build -q --release -p ordin-obs --features fake --example fake-obs; fi
# An unbundled program finds its resources beside it: the bundled agents go there.
rm -rf target/release/catalogue && cp -r apps/desktop/catalogue target/release/catalogue
echo built: \$(git rev-parse HEAD)"

note "journeys: running"
set +e
win_bash "cd '$REPO/apps/desktop/journeys' && npm ci --no-audit --no-fund >/dev/null && ORDIN_JOURNEYS_MINUTES=$MINUTES_FLAG ORDIN_JOURNEYS_EXE=\$(cygpath -w '$REPO/target/release/ordin.exe') npx playwright test${GREP:+ --grep '$GREP'}"
STATUS=$?
# Whatever the result (and even when Playwright was cut short before its own teardown), ordin does not
# stay running on the Windows machine: a running ordin.exe locks the file the next release build writes.
# taskkill answers non-zero when nothing runs; that is the wanted state, so its status is not the run's.
win_bash "taskkill //F //IM ordin.exe >/dev/null 2>&1; exit 0" || note "journeys: could not reach the Windows machine to stop ordin.exe"
set -e

rm -rf "$OUT" && mkdir -p "$OUT"
win_bash "cd '$REPO/apps/desktop/journeys/run' && tar -c results.json report output 2>/dev/null" | tar -x -C "$OUT" || true
if [ "$STATUS" -eq 0 ] && [ "$MINUTES_FLAG" = 0 ]; then
  note "journeys: every journey passed on $COMMIT without minutes (report in $OUT); a build without minutes records nothing"
elif [ "$STATUS" -eq 0 ] && [ -n "${GREP:-}" ]; then
  note "journeys: the chosen journeys passed on $COMMIT (report in $OUT); with GREP set, nothing was recorded"
elif [ "$STATUS" -eq 0 ]; then
  # The release build reads this note (PB-REL-27); a run of some journeys only (GREP) records nothing.
  # spec: ordin-desktop/phase-5-playbook#PB-REL-28
  COUNT=$(jq -r '.stats.expected // empty' "$OUT/results.json" 2>/dev/null || true)
  RECORD="${JOURNEYS_PASSED_PREFIX}$(date -u +%Y-%m-%dT%H:%M:%SZ) windows, ${COUNT:-?} journeys"
  IDENTITY=()
  if [ -z "$(git -C "$ROOT" config user.email 2>/dev/null || true)" ]; then
    IDENTITY=(-c user.name=ordin-gate -c user.email=ordin-gate@localhost)
  fi
  git -C "$ROOT" "${IDENTITY[@]+"${IDENTITY[@]}"}" notes --ref="$JOURNEYS_NOTES_REF" add -f -m "$RECORD" "$COMMIT" \
    || die "every journey passed on $COMMIT, but the note could not be written."
  note "journeys: every journey passed on $COMMIT (report in $OUT); recorded: $RECORD"
else
  note "journeys: FAILED on $COMMIT (report in $OUT)"
fi
exit "$STATUS"
