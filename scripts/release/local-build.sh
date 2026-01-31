#!/usr/bin/env bash
# make local-build: an unsigned installer of the working tree for this computer, to try ordin by hand.
# Not a release: no gate, no signing key, no update files, never published. A `.app` and a `.dmg` on
# macOS, an NSIS installer on Windows, in target/release/bundle/.
#
#   AGENTS=commit    (default) the bundled agents at RELEASE_BUNDLED_AGENTS_COMMIT, as a release copies them
#   AGENTS=worktree  the bundled agents' working folder as it is now, committed or not
#   AGENTS=none      no bundled agents
#   JOURNEYS=1       the test-only `journeys` switch (OrdinDesktop.md DSK-JRN-1): WebView2's debugging port and
#                    the dialogs' answer file, for Playwright on Windows. Never give it to anyone.
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf

case "$(uname -s)" in
  Darwin) BUNDLES="app,dmg" ;;
  MINGW*|MSYS*|CYGWIN*) BUNDLES="nsis" ;;
  *) die "ordin opens a window on macOS and Windows only; build on one of them" ;;
esac

CATALOGUE="$ROOT/apps/desktop/catalogue"
SOURCE="${RELEASE_BUNDLED_AGENTS_SOURCE:-}"
case "$SOURCE" in ""|/*) ;; *) SOURCE="$ROOT/$SOURCE" ;; esac
case "${AGENTS:-commit}" in
  commit)
    [ -n "$SOURCE" ] && [ -n "${RELEASE_BUNDLED_AGENTS_COMMIT:-}" ] \
      || die "release/release.conf names no bundled agents; use AGENTS=worktree or AGENTS=none"
    rm -rf "$CATALOGUE" && mkdir -p "$CATALOGUE"
    git -C "$SOURCE" archive "$RELEASE_BUNDLED_AGENTS_COMMIT" | tar -x -C "$CATALOGUE"
    note "bundled agents: commit $RELEASE_BUNDLED_AGENTS_COMMIT of $SOURCE"
    ;;
  worktree)
    [ -d "$SOURCE" ] || die "the bundled agents' folder $SOURCE is not there"
    rm -rf "$CATALOGUE" && mkdir -p "$CATALOGUE"
    (cd "$SOURCE" && git ls-files -co --exclude-standard | tar -c -T -) | tar -x -C "$CATALOGUE"
    note "bundled agents: the working folder of $SOURCE, as it is now"
    ;;
  none)
    rm -rf "$CATALOGUE"
    note "no bundled agents: Ordin's conversation will not start"
    ;;
  *) die "AGENTS is commit, worktree or none" ;;
esac

FEATURES=()
if [ "${JOURNEYS:-}" = 1 ]; then
  FEATURES=(--features journeys)
  note "WARNING: the journeys test switch is on; this build opens a debugging port. Keep it to the test machine."
fi
# MINUTES=1 names the `minutes` switch explicitly; it is on by default since DSK-MIN-6.
if [ "${MINUTES:-}" = 1 ]; then
  FEATURES+=(--features minutes)
  note "meeting minutes switch on"
fi

# The memory models, as a release passes them (PB-REL-13). Without them the build has no memory: Ordin
# can never remember or recall anything and the person is never offered the download (DSK-MD-2).
require_models_input
export ORDIN_MEMORY_MODELS_URL="$RELEASE_MEMORY_MODELS_URL"
export ORDIN_MEMORY_MODELS_SHA256="$RELEASE_MEMORY_MODELS_SHA256"
export ORDIN_MEMORY_MODELS_BYTES="$RELEASE_MEMORY_MODELS_BYTES"
export ORDIN_MEMORY_MODELS_MANIFEST="$MODELS_MANIFEST"
github_app_env

# The bundled agents go into the bundle's `catalogue` resource folder, where the program reads them
# (DSK-REC-39a), as the release build places them (PB-REL-6); tauri.conf.json names no resource itself.
# spec: ordin-desktop/phase-5-playbook#PB-REL-7
CONFIG=()
if [ -d "$CATALOGUE" ]; then
  CONFIG=(--config '{"bundle":{"resources":{"catalogue/":"catalogue/"}}}')
fi

make -C "$ROOT" desktop-build-inputs
# CI=true: Tauri's DMG step skips the Finder layout, which fails intermittently on macOS.
(cd "$ROOT/apps/desktop" && CI=true cargo tauri build --bundles "$BUNDLES" ${CONFIG[@]+"${CONFIG[@]}"} ${FEATURES[@]+"${FEATURES[@]}"})
note "unsigned build of $(git -C "$ROOT" rev-parse --short HEAD)$(git -C "$ROOT" diff --quiet HEAD || echo ' plus uncommitted changes') in target/release/bundle/"
