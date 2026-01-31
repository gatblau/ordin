#!/usr/bin/env bash
# make release-build-windows-remote: the Windows installer, built and signed on the Windows build machine,
# started from this computer. The Windows computer stays the place the real feed takes a Windows installer
# from (PB-REL-15); this only saves typing its steps. It builds the commit this repository is at, so that
# commit must be in the git repository the Windows machine fetches from (and have its gate and journeys
# records there, as release-check needs).
#
#   VERSION=<version>        the version, as for release-build.
#   TAURI_SIGNING_PRIVATE_KEY  the update key's file on this computer; default ~/ordin-update.key.
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD  asked for, without echo, when not set.
#   RELEASE_WINDOWS_SSH      how to reach the Windows machine; default JOURNEYS_WINDOWS_SSH, else
#                            "ssh -p 2222 ordin@127.0.0.1".
#   RELEASE_WINDOWS_REPO     the build checkout there, in Git Bash's form; default /c/o.
#   RELEASE_WINDOWS_REMOTE   the git remote of that checkout to fetch from; default "server".
#   RELEASE_GATE_REMOTE      the remote the Windows machine reads the records from; default "server".
#
# The key goes to the Windows machine for the build only: its file there is made readable by the account
# alone, and removed when the build ends, whatever its result. The password reaches the build through
# standard input, never as an argument. The files come back into the release folder here, and their
# signatures and hashes are checked here before the script reports success.
# spec: ordin-desktop/phase-5-playbook#PB-REL-29
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf
require_version
require_package_version
need_tool minisign "brew install minisign"

COMMIT=$(git -C "$ROOT" rev-parse --verify "HEAD^{commit}")
# PB-REL-31: the GitHub App client id goes with the build, checked here so it cannot break the quoting.
GITHUB_APP_ID="${RELEASE_GITHUB_APP_CLIENT_ID:-}"
[[ "$GITHUB_APP_ID" =~ ^[A-Za-z0-9._-]{0,128}$ ]] || die "RELEASE_GITHUB_APP_CLIENT_ID may hold only letters, digits, '.', '_' and '-' (at most 128)"
read -r -a WIN <<< "${RELEASE_WINDOWS_SSH:-${JOURNEYS_WINDOWS_SSH:-ssh -p 2222 ordin@127.0.0.1}}"
REPO="${RELEASE_WINDOWS_REPO:-/c/o}"
REMOTE="${RELEASE_WINDOWS_REMOTE:-server}"
GATE_REMOTE="${RELEASE_GATE_REMOTE:-server}"
OUT="${RELEASE_OUT_DIR:-$ROOT/target/release-out/$VERSION}"
REMOTE_KEY='~/.ordin-release-key'

# Runs a Git Bash script on the Windows machine (its SSH shell is PowerShell); see journeys-windows.sh for
# why the scripts below hold no apostrophe and quote nothing that holds a space.
win_bash() {
  "${WIN[@]}" "& 'C:\\Program Files\\Git\\bin\\bash.exe' -lc '$(printf %s "$1" | sed "s/'/''/g")'"
}

TAURI_SIGNING_PRIVATE_KEY="${TAURI_SIGNING_PRIVATE_KEY:-$HOME/ordin-update.key}"
[ -f "$TAURI_SIGNING_PRIVATE_KEY" ] && [ -r "$TAURI_SIGNING_PRIVATE_KEY" ] \
  || die "the update key is not at $TAURI_SIGNING_PRIVATE_KEY; set TAURI_SIGNING_PRIVATE_KEY to its path. Nothing was built."
require_public_key
if [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+x}" ]; then
  input="${RELEASE_PROMPT_INPUT:-/dev/tty}"
  ( exec 3< "$input" ) 2>/dev/null || die "there is no terminal to ask the key's password on; set TAURI_SIGNING_PRIVATE_KEY_PASSWORD. Nothing was built."
  exec 3< "$input"
  printf 'Password for the update key %s: ' "$TAURI_SIGNING_PRIVATE_KEY" >&2
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
  IFS= read -rs TAURI_SIGNING_PRIVATE_KEY_PASSWORD <&3 || true
  printf '\n' >&2
fi
export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# A wrong password stops here, before the key leaves this computer.
check_signing_key

if [ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null || true)" ]; then
  note "this repository has uncommitted changes; the Windows machine builds the commit $COMMIT, not them"
fi

note "windows: checking that $COMMIT is on the Windows machine"
win_bash "set -euo pipefail
cd '$REPO'
git fetch -q '$REMOTE' '+refs/heads/*:refs/remotes/$REMOTE/*'
git cat-file -e '$COMMIT^{commit}'" \
  || die "the Windows machine cannot read the commit $COMMIT from its git remote '$REMOTE'; push the commit to that repository first. Nothing was built."

# The key is removed from the Windows machine whatever happens next.
remove_remote_key() {
  win_bash "rm -f $REMOTE_KEY" >/dev/null 2>&1 \
    || note "windows: could not reach the Windows machine to remove the update key there; remove $REMOTE_KEY by hand"
}
trap remove_remote_key EXIT
win_bash "umask 077; cat > $REMOTE_KEY" < "$TAURI_SIGNING_PRIVATE_KEY" \
  || die "could not copy the update key to the Windows machine. Nothing was built."

note "windows: building $VERSION from $COMMIT on the Windows machine"
printf '%s\n' "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" | win_bash "set -euo pipefail
read -r TAURI_SIGNING_PRIVATE_KEY_PASSWORD
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
export TAURI_SIGNING_PRIVATE_KEY=\$HOME/.ordin-release-key
export RELEASE_GATE_REMOTE='$GATE_REMOTE'
cd '$REPO'
git checkout -q -f '$COMMIT'
git clean -fdq -e node_modules -e target
rm -rf 'target/release-out/$VERSION'
RELEASE_GITHUB_APP_CLIENT_ID='$GITHUB_APP_ID' make release-build VERSION='$VERSION'" \
  || die "the build on the Windows machine failed; see its output above. The update key was removed from that machine."
remove_remote_key
trap - EXIT

note "windows: bringing the Windows files into $OUT"
mkdir -p "$OUT"
win_bash "cd '$REPO/target/release-out/$VERSION' && tar -c ordin-$VERSION-windows-* SHA256SUMS.windows-* platform-windows-*.json build-windows-*.log" | tar -x -C "$OUT" \
  || die "the Windows files did not come back from the Windows machine."
INSTALLERS=("$OUT"/ordin-"$VERSION"-windows-*.setup.exe)
[ -f "${INSTALLERS[0]}" ] || die "no Windows installer came back from the Windows machine."
for f in "${INSTALLERS[@]}"; do
  [ -f "$f.minisig" ] || die "$(basename "$f") came back without its signature."
  minisign -Vm "$f" -p "$PUBLIC_KEY_FILE" -x "$f.minisig" >/dev/null 2>&1 \
    || die "the signature of $(basename "$f") is not valid for release/ordin-update.pub."
done
SUMS=("$OUT"/SHA256SUMS.windows-*)
(cd "$OUT" && { shasum -a 256 -c "$(basename "${SUMS[0]}")" >/dev/null 2>&1 || sha256sum -c "$(basename "${SUMS[0]}")" >/dev/null 2>&1; }) \
  || die "the hashes of the Windows files do not match."
"$(dirname "$0")/leak-check.sh" "$OUT" || die "the release folder holds the update key or its password."
note "windows: the Windows installer is in $OUT and its signature and hash are good; publish with make publish VERSION=$VERSION"
