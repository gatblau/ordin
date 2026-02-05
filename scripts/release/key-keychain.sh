#!/usr/bin/env bash
# make release-key-keychain: gives the update key a new password and keeps that password in the macOS
# keychain, so make release and make publish read it from there (PB-REL-34) and ask for nothing.
#
# Every password is typed by the person at the terminal, to minisign and to security; none passes through
# this script, its arguments or the environment. The key is decrypted nowhere: minisign re-encrypts it in a
# private temporary folder, deleted on exit. The key file is replaced only after the new key and the
# keychain's password sign a file that verifies against release/ordin-update.pub.
# spec: ordin-desktop/phase-5-playbook#PB-REL-35
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf

KEY="${TAURI_SIGNING_PRIVATE_KEY:-$HOME/ordin-update.key}"
ITEM="${RELEASE_KEY_KEYCHAIN:-ordin-update-key}"
SECURITY="${RELEASE_SECURITY_TOOL:-security}"
[ -f "$KEY" ] && [ -r "$KEY" ] || die "the update key is not at $KEY; set TAURI_SIGNING_PRIVATE_KEY to its path. Nothing was changed."
require_public_key
need_tool minisign "brew install minisign"
[ -r /dev/tty ] || die "there is no terminal to type the passwords on. Run this in a Terminal window. Nothing was changed."

WORK="$(mktemp -d)"
chmod 700 "$WORK"
trap 'rm -rf "$WORK"' EXIT
base64 -d -i "$KEY" > "$WORK/key.minisign" 2>/dev/null || base64 -d < "$KEY" > "$WORK/key.minisign"

printf '\nStep 1 of 2: minisign asks for the key'"'"'s current password, then a new one twice.\n' >&2
printf 'Choose a long new password (20 characters or more); you will not need to type it again.\n\n' >&2
minisign -C -s "$WORK/key.minisign" < /dev/tty || die "minisign did not change the password. Nothing was changed."
# Tauri reads the key file as one base64 line with no newline at the end, like the file it generated.
base64 < "$WORK/key.minisign" | tr -d '\n' > "$WORK/key.new"

printf '\nStep 2 of 2: type the NEW password again, twice, for the keychain item %s.\n\n' "$ITEM" >&2
"$SECURITY" add-generic-password -U -a "${USER:-$(id -un)}" -s "$ITEM" -w < /dev/tty \
  || die "the keychain item $ITEM was not written. The key file is unchanged."

# The new key and the keychain's password must sign as the committed public key before the key file changes.
TAURI_SIGNING_PRIVATE_KEY="$WORK/key.new"
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$("$SECURITY" find-generic-password -a "${USER:-$(id -un)}" -s "$ITEM" -w 2>/dev/null)" \
  || die "the keychain item $ITEM could not be read back. The key file is unchanged."
export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
printf 'ordin key check\n' > "$WORK/probe"
if ! SIGN_OUT="$(sign_file "$WORK/probe" 2>&1)" || [ ! -s "$WORK/probe.sig" ]; then
  case "$SIGN_OUT" in
    *"Wrong password"*|*"password"*) die "the new key did not sign with the keychain's password (the two passwords typed may differ). The key file is unchanged; run this again." ;;
    *) die "the new key did not sign: $(printf '%s' "$SIGN_OUT" | tail -n 1 | cut -c1-200). The key file is unchanged." ;;
  esac
fi
base64 -d < "$WORK/probe.sig" > "$WORK/probe.minisig"
minisign -Vm "$WORK/probe" -p "$PUBLIC_KEY_FILE" -x "$WORK/probe.minisig" >/dev/null 2>&1 \
  || die "the key signs, but not as release/ordin-update.pub. The key file is unchanged."

[ -e "$KEY.bak" ] || cp -p "$KEY" "$KEY.bak"
cp "$WORK/key.new" "$KEY"
chmod 600 "$KEY"
note "the key at $KEY has its new password, kept in the keychain item $ITEM"
note "the old key is at $KEY.bak; delete it with rm -P once a release has worked"
if ! grep -qs "RELEASE_KEY_KEYCHAIN=" "$HOME/.zshrc"; then
  printf 'export RELEASE_KEY_KEYCHAIN=%s\n' "$ITEM" >> "$HOME/.zshrc"
  note "added RELEASE_KEY_KEYCHAIN=$ITEM to ~/.zshrc; open a new Terminal window for it to apply"
fi
