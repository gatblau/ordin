#!/usr/bin/env bash
# make release: the one command for a release. With no VERSION it asks what to do, step by step;
# with VERSION=<version> it asks only for the key's password.
#
#   make release                              asks: version, what to do, the password
#   make release VERSION=0.1.0 PUBLISH=0      build only, no questions but the password
#   make release VERSION=0.1.1-test.1         build, then publish (RELEASE_CHANNEL=test is the default)
#   make publish VERSION=0.1.1-test.1         publish what the builds made (BUILD=0); nothing is built
#
# It checks the key and the password by signing a small file and verifying it against the committed
# public key, so a wrong password or key stops at once and not after the gate. The password is read from
# the terminal without echo and reaches the later steps only through the environment (PB-REL-3). With
# TAURI_SIGNING_PRIVATE_KEY unset the key is ~/ordin-update.key.
# spec: ordin-desktop/phase-5-playbook#PB-REL-3
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf
use_pinned_toolchain
use_container_socket

# Every answer comes from one input, read in order: the terminal, or (tests only) the file in
# RELEASE_PROMPT_INPUT.
open_input() {
  local input="${RELEASE_PROMPT_INPUT:-/dev/tty}"
  ( exec 3< "$input" ) 2>/dev/null \
    || die "there is no terminal to ask on. Run this in a terminal window, or give VERSION=<version>. Nothing was built."
  exec 3< "$input"
}
ask() { # ask <prompt> <default>; the answer is in $ANSWER
  local reply=""
  printf '%s' "$1" >&2
  IFS= read -r reply <&3 || true
  ANSWER="${reply:-$2}"
}
yes_no() { # yes_no <prompt> <default y|n>; returns 0 for yes
  ask "$1" "$2"
  case "$ANSWER" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

INTERACTIVE=0
if [ "${BUILD:-1}" = 0 ]; then
  [ -n "${VERSION:-}" ] || die "give the version to publish: make publish VERSION=<version>. Nothing was published."
  [ "${PUBLISH:-1}" != 0 ] || die "BUILD=0 with PUBLISH=0 does nothing. Nothing was published."
fi
if [ -z "${VERSION:-}" ]; then
  INTERACTIVE=1
  open_input
  have="$(package_version)"
  printf '\nordin release. This computer builds and signs; publishing puts the files on GitHub.\n\n' >&2
  ask "Version to release [$have]: " "$have"
  VERSION="$ANSWER"
  require_version
  printf '\n  1  Build only (installers and signatures on this computer, nothing published)\n' >&2
  printf '  2  Build and publish to the TEST feed\n' >&2
  printf '  3  Build and publish to the REAL feed (what people get)\n\n' >&2
  ask "What do you want to do? [1]: " 1
  case "$ANSWER" in
    1) PUBLISH=0 ;;
    2) PUBLISH=1; RELEASE_CHANNEL=test
       if ! ls "${RELEASE_OUT_DIR:-$ROOT/target/release-out/$VERSION}"/platform-windows-*.json >/dev/null 2>&1; then
         yes_no "There is no Windows build yet. Publish the test feed with macOS only? [y/N]: " n \
           && RELEASE_ALLOW_SINGLE_PLATFORM=1 \
           || die "build on a Windows computer too, or answer y to publish macOS only. Nothing was built."
       fi ;;
    3) PUBLISH=1; RELEASE_CHANNEL=real
       ask "This publishes $VERSION to everyone. Type the version again to confirm: " ""
       [ "$ANSWER" = "$VERSION" ] || die "the version did not match; nothing was built."
       ;;
    *) die "answer 1, 2 or 3. Nothing was built." ;;
  esac
  if ! bundled_inputs_set && [ "${RELEASE_ALLOW_NO_BUNDLED_AGENTS:-}" != 1 ]; then
    yes_no "No bundled agents are set. Build without them? [y/N]: " n \
      && RELEASE_ALLOW_NO_BUNDLED_AGENTS=1 \
      || die "set the bundled agents in release/release.conf (OQ-REC-13), or answer y. Nothing was built."
  fi
  OUT_DIR="${RELEASE_OUT_DIR:-$ROOT/target/release-out/$VERSION}"
  if [ "$PUBLISH" = 1 ] && [ -z "${RELEASE_NOTES_FILE:-}" ] && [ ! -s "$OUT_DIR/notes.md" ]; then
    ask "Release notes, one line (shown to people who update): " ""
    [ -n "$ANSWER" ] || die "release notes are needed to publish. Nothing was built."
    mkdir -p "$OUT_DIR"
    printf '%s\n' "$ANSWER" > "$OUT_DIR/notes.md"
  fi
  export PUBLISH RELEASE_ALLOW_NO_BUNDLED_AGENTS RELEASE_ALLOW_SINGLE_PLATFORM RELEASE_CHANNEL 2>/dev/null || true
else
  require_version
fi
export VERSION

TAURI_SIGNING_PRIVATE_KEY="${TAURI_SIGNING_PRIVATE_KEY:-$HOME/ordin-update.key}"
[ -f "$TAURI_SIGNING_PRIVATE_KEY" ] && [ -r "$TAURI_SIGNING_PRIVATE_KEY" ] \
  || die "the update key is not at $TAURI_SIGNING_PRIVATE_KEY; set TAURI_SIGNING_PRIVATE_KEY to its path. Nothing was built."
require_public_key
need_tool minisign "brew install minisign"

# With RELEASE_KEY_KEYCHAIN naming a macOS keychain item (its service name), the password is read from it,
# never printed; an item that cannot be read stops the release.
# spec: ordin-desktop/phase-5-playbook#PB-REL-34
if [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+x}" ] && [ -n "${RELEASE_KEY_KEYCHAIN:-}" ]; then
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$("${RELEASE_SECURITY_TOOL:-security}" find-generic-password -a "${USER:-$(id -un)}" -s "$RELEASE_KEY_KEYCHAIN" -w 2>/dev/null)" \
    || die "the keychain item $RELEASE_KEY_KEYCHAIN could not be read; add it with: security add-generic-password -a \"\$USER\" -s $RELEASE_KEY_KEYCHAIN -w. Nothing was built."
  note "the update key's password comes from the keychain item $RELEASE_KEY_KEYCHAIN"
fi
if [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+x}" ]; then
  [ "$INTERACTIVE" = 1 ] || open_input
  printf 'Password for the update key %s: ' "$TAURI_SIGNING_PRIVATE_KEY" >&2
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
  IFS= read -rs TAURI_SIGNING_PRIVATE_KEY_PASSWORD <&3 || true
  printf '\n' >&2
fi
export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD

# A wrong password, or a key that is not the committed public key's pair, stops here and not after the gate.
CHECK_DIR="$(mktemp -d)"
trap 'rm -rf "$CHECK_DIR"' EXIT
printf 'ordin key check\n' > "$CHECK_DIR/probe"
if ! SIGN_OUT="$(sign_file "$CHECK_DIR/probe" 2>&1)" || [ ! -s "$CHECK_DIR/probe.sig" ]; then
  case "$SIGN_OUT" in
    *"Wrong password"*) die "the key could not sign: the password is wrong. Nothing was built." ;;
    *) die "the key could not sign: $(printf '%s' "$SIGN_OUT" | tail -n 1 | cut -c1-200). Nothing was built." ;;
  esac
fi
base64 -d < "$CHECK_DIR/probe.sig" > "$CHECK_DIR/probe.minisig"
minisign -Vm "$CHECK_DIR/probe" -p "$PUBLIC_KEY_FILE" -x "$CHECK_DIR/probe.minisig" >/dev/null 2>&1 \
  || die "the key signs, but not as release/ordin-update.pub: this is not that key pair. Nothing was built."
note "the key and the password are good"

if [ "${BUILD:-1}" = 0 ]; then
  note "publishing what is in ${RELEASE_OUT_DIR:-target/release-out/$VERSION}; nothing is built"
  exec "$(dirname "$0")/publish.sh"
fi
"$(dirname "$0")/build.sh"
if [ "${PUBLISH:-1}" = 0 ]; then
  note "stopped after the build; the installers are in ${RELEASE_OUT_DIR:-target/release-out/$VERSION}"
else
  "$(dirname "$0")/publish.sh"
fi
