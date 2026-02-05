#!/usr/bin/env bash
# Tests of the release scripts (PB-REL-10) against stand-ins for `make`, `cargo` (pkgid, tauri build,
# tauri signer sign) and `minisign`. The stand-in signature is "SIG-<sha256 of the file>", so a file
# changed by one byte fails exactly as it does with the real minisign. Nothing here needs a real key,
# a network or a Tauri install. Run: scripts/release/test.sh
# spec: ordin-desktop/phase-5-playbook#PB-REL-10
set -uo pipefail
command -v jq >/dev/null 2>&1 || { echo "release tests: jq is not installed (brew install jq, or apt-get install jq)" >&2; exit 1; }
SRC="$(cd "$(dirname "$0")/../.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
PASS=0; FAIL=0

# A copy of the scripts in a throw-away repository, so no test touches the real tree.
REPO="$T/repo"
mkdir -p "$REPO/scripts/release" "$REPO/release/memory-models" "$REPO/apps/desktop" "$T/bin"
cp "$SRC"/scripts/release/*.sh "$REPO/scripts/release/"
cp "$SRC/scripts/journeys-windows.sh" "$REPO/scripts/"
# The committed bundled-agents pin names a clone beside the repository; the tests set those inputs
# themselves, so the copy leaves them out and runs the same on any computer.
grep -v '^RELEASE_BUNDLED_AGENTS_' "$SRC/release/release.conf" > "$REPO/release/release.conf"
cp "$SRC/release/memory-models/files.txt" "$REPO/release/memory-models/files.txt"
printf 'untrusted comment: stand-in public key\nRWQSTANDINPUBLICKEY0000000000000000000000000000000\n' > "$REPO/release/ordin-update.pub"

KEY="$T/test.key"
printf 'untrusted comment: stand-in secret key\nRWRTSECRETKEYLINEDONOTLEAK1234567890abcdef\n' > "$KEY"
PASSWORD='correct-horse-battery'
SHIM_LOG="$T/shim.log"; export SHIM_LOG

cat > "$T/bin/make" <<'SH'
#!/usr/bin/env bash
echo "make $* XWIN=${XWIN_ACCEPT_LICENSE:-}" >> "$SHIM_LOG"
[ -z "${SHIM_MAKE_FAIL:-}" ] || exit 2
# the hook beside the program, as the real targets copy it; its imports are lines the objdump stand-in reads
case "$3" in desktop-build-inputs|desktop-build-inputs-windows)
  mkdir -p "$2/apps/desktop/binaries"
  for d in ${SHIM_HOOK_IMPORTS:-kernel32.dll ntdll.dll}; do echo "import: $d"; done > "$2/apps/desktop/binaries/ordin-hook-x86_64-pc-windows-msvc.exe" ;;
esac
SH
# Reads a stand-in Windows program: each "import: <dll>" line is printed as objdump -p prints an import.
cat > "$T/bin/llvm-objdump" <<'SH'
#!/usr/bin/env bash
[ "$1" = -p ] || exit 2
sed -n 's/^import: \(.*\)$/	DLL Name: \1/p' "$2"
SH
# A stand-in package: one "<member path><TAB><content line>" per line; a member may have several lines.
cat > "$T/bin/unzip" <<'SH'
#!/usr/bin/env bash
case "$1" in
  -Z1) cut -f1 "$2" | sort -u ;;
  -p) awk -F'\t' -v m="$3" '$1 == m { print $2; found = 1 } END { exit !found }' "$2" ;;
  *) exit 2 ;;
esac
SH
cat > "$T/bin/cargo" <<'SH'
#!/usr/bin/env bash
echo "cargo $*" >> "$SHIM_LOG"
if [ "$1" = pkgid ]; then echo "path+file:///x/apps/desktop#ordin-desktop@${SHIM_PKG_VERSION:-1.2.3}"; exit 0; fi
if [ "$1" = tauri ] && [ "$2" = --version ]; then echo "tauri-cli ${SHIM_TAURI_VERSION:-2.12.4}"; exit 0; fi
if [ "$1" = tauri ] && [ "$2" = build ]; then
  [ -z "${SHIM_LEAK:-}" ] || cat "$TAURI_SIGNING_PRIVATE_KEY"
  echo "MODELS_URL=${ORDIN_MEMORY_MODELS_URL:-}" >> "$SHIM_LOG"
  echo "MODELS_SHA256=${ORDIN_MEMORY_MODELS_SHA256:-}" >> "$SHIM_LOG"
  echo "MODELS_BYTES=${ORDIN_MEMORY_MODELS_BYTES:-}" >> "$SHIM_LOG"
  echo "MODELS_MANIFEST=${ORDIN_MEMORY_MODELS_MANIFEST:-}" >> "$SHIM_LOG"
  echo "CONFIG=$(tr -d '\n' < "$4")" >> "$SHIM_LOG"
  echo "XWIN_ACCEPT_LICENSE=${XWIN_ACCEPT_LICENSE:-}" >> "$SHIM_LOG"
  echo "XWIN_CACHE_DIR=${XWIN_CACHE_DIR:-}" >> "$SHIM_LOG"
  echo "GITHUB_APP=${GITHUB_APP_CLIENT_ID:-}" >> "$SHIM_LOG"
  b="$RELEASE_BUNDLE_DIR"
  plat="$RELEASE_PLATFORM"
  case " $* " in *" --runner cargo-xwin "*) plat=windows-x86_64 ;; esac
  case "$plat" in
    darwin-*)
      mkdir -p "$b/dmg" "$b/macos/ordin.app/Contents/Resources"
      echo "dmg of $RELEASE_PLATFORM" > "$b/dmg/ordin_1.2.3.dmg"
      echo "updater of $RELEASE_PLATFORM" > "$b/macos/ordin.app.tar.gz"
      if [ -d catalogue ]; then cp -R catalogue "$b/macos/ordin.app/Contents/Resources/catalogue"; fi ;;
    windows-*)
      mkdir -p "$b/nsis"
      echo "setup of $plat" > "$b/nsis/ordin_1.2.3_x64-setup.exe"
      for d in ${SHIM_PROGRAM_IMPORTS:-kernel32.dll DirectML.dll MSVCP140.dll MSVCP140_1.dll api-ms-win-crt-heap-l1-1-0.dll}; do echo "import: $d"; done > "$b/../ordin.exe"
      if [ -d windows-runtime ]; then echo "STAGED=$(cd windows-runtime && ls | tr '\n' ' ')" >> "$SHIM_LOG"; fi ;;
  esac
  exit 0
fi
if [ "$1" = tauri ] && [ "$2" = signer ] && [ "$3" = sign ]; then
  f="${!#}"
  # like the real signer: the key variable is read as text, so a path in it is an error
  [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ] || { echo "Error failed to decode base64 secret key" >&2; exit 1; }
  [ "$4" = -f ] || { echo "Error no key file given" >&2; exit 1; }
  [ -z "${SHIM_SIGN_FAIL:-}" ] || { echo "Error incorrect updater private key password: Wrong password for that key" >&2; exit 1; }
  sum="$( (sha256sum "$f" 2>/dev/null || shasum -a 256 "$f") | cut -d' ' -f1)"
  printf 'untrusted comment: stand-in signature\nSIG-%s\n' "$sum" | base64 | tr -d '\n' > "$f.sig"
  exit 0
fi
exit 0
SH
cat > "$T/bin/minisign" <<'SH'
#!/usr/bin/env bash
file=""; sig=""
while [ "$#" -gt 0 ]; do
  case "$1" in -Vm) file="$2"; shift 2 ;; -x) sig="$2"; shift 2 ;; -p) shift 2 ;; *) shift ;; esac
done
if [ -n "${SHIM_MINISIGN_FAIL_PROBE:-}" ]; then case "$file" in */probe) exit 1 ;; esac; fi
if [ -n "${SHIM_MINISIGN_FAIL_READBACK:-}" ]; then case "$file" in */readback/*) exit 1 ;; esac; fi
sum="$( (sha256sum "$file" 2>/dev/null || shasum -a 256 "$file") | cut -d' ' -f1)"
[ "$(sed -n 2p "$sig")" = "SIG-$sum" ]
SH
chmod +x "$T/bin"/*
export PATH="$T/bin:$PATH"

# Run one script: run <script> [VAR=value ...]; sets STATUS and OUTPUT. The copy is not a git repository
# until the gate's own tests at the end, so every other test runs the gate here (RELEASE_GATE=local, the
# stand-in make); a test that gives RELEASE_GATE=evidence overrides it.
# The files beside ordin.exe (PB-REL-25), served from file:// addresses: a stand-in DirectML package and the
# fingerprint of the DLL it holds; a stand-in Visual Studio channel, its manifest listing an older and a newer
# x64 Visual C++ redistributable, and the newer package with the four runtime DLLs (and a debug one).
sha_of_stdin() { { sha256sum 2>/dev/null || shasum -a 256; } | cut -d' ' -f1; }
DML_PKG="$T/directml.nupkg"
printf 'bin/x64-win/DirectML.dll\timport: d3d12.dll\nbin/arm64-win/DirectML.dll\timport: other.dll\n' > "$DML_PKG"
DML_SHA="$(printf 'import: d3d12.dll\n' | sha_of_stdin)"
VC_PKG="$T/vc-redist.zip"
CRT="Contents/VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT"
printf '%s\n' "$CRT/msvcp140.dll	import: VCRUNTIME140.dll" "$CRT/msvcp140.dll	import: VCRUNTIME140_1.dll" \
  "$CRT/msvcp140_1.dll	import: MSVCP140.dll" "$CRT/vcruntime140.dll	import: KERNEL32.dll" \
  "$CRT/vcruntime140_1.dll	import: VCRUNTIME140.dll" \
  "Contents/VC/Redist/MSVC/14.44.35112/debug_nonredist/x64/Microsoft.VC143.DebugCRT/msvcp140.dll	import: debug.dll" > "$VC_PKG"
vc_manifest() { # vc_manifest <newest package's sha256> <newest package's address>
  jq -n --arg sha "$1" --arg url "$2" --arg old "file://$T/no-such-old.zip" '{packages: [
    {id: "Microsoft.VC.14.9.17.0.CRT.Redist.X64.base", version: "14.9.30000", payloads: [{url: $old, sha256: ("0" * 64)}]},
    {id: "Microsoft.VC.14.44.17.14.CRT.Redist.X64.base", version: "14.44.35211", payloads: [{url: $url, sha256: $sha}]},
    {id: "Microsoft.VC.14.44.17.14.CRT.Redist.X64", version: "99.0", payloads: [{url: $old, sha256: ("0" * 64)}]},
    {id: "Microsoft.VC.14.44.17.14.CRT.Redist.ARM64.base", version: "99.0", payloads: [{url: $old, sha256: ("0" * 64)}]}]}'
}
vc_manifest "$(sha_of_stdin < "$VC_PKG")" "file://$VC_PKG" > "$T/vs-manifest.json"
jq -n --arg url "file://$T/vs-manifest.json" '{channelItems: [{type: "Bootstrapper"}, {type: "Manifest", payloads: [{url: $url}]}]}' > "$T/vs-channel.json"
run() { run_at "$REPO" "$@"; }
run_at() { # run_at <repository> <script> [VAR=value ...]
  local root="$1" script="$2"; shift 2
  : > "$SHIM_LOG"
  OUTPUT="$(env -i PATH="$PATH" HOME="$T" SHIM_LOG="$SHIM_LOG" RELEASE_PLATFORM=darwin-aarch64 \
    RELEASE_BUNDLE_DIR="$T/bundle" RELEASE_OUT_DIR="$T/out" RELEASE_ALLOW_FILE_URLS=1 RELEASE_GATE=local \
    RELEASE_WINDOWS_DIRECTML_URL="file://$DML_PKG" RELEASE_WINDOWS_DIRECTML_SHA256="$DML_SHA" \
    RELEASE_WINDOWS_VC_CHANNEL_URL="file://$T/vs-channel.json" \
    "$@" bash "$root/scripts/release/$script" 2>&1)"
  STATUS=$?
}
ok() { PASS=$((PASS + 1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf 'FAIL %s\n' "$1"; printf '%s\n' "$OUTPUT" | sed 's/^/     | /'; }
expect_fail() { if [ "$STATUS" -ne 0 ] && printf '%s' "$OUTPUT" | grep -qF -- "$2"; then ok "$1"; else bad "$1 (status $STATUS, wanted: $2)"; fi; }
expect_pass() { if [ "$STATUS" -eq 0 ]; then ok "$1"; else bad "$1 (status $STATUS)"; fi; }
expect_no_calls() { if ! grep -qE "$2" "$SHIM_LOG"; then ok "$1"; else bad "$1 (log: $(cat "$SHIM_LOG"))"; fi; }

KEYENV=(TAURI_SIGNING_PRIVATE_KEY="$KEY" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$PASSWORD")
ALLOW=(RELEASE_ALLOW_NO_BUNDLED_AGENTS=1)

# --- version and key rules
run build.sh VERSION=1.2 "${KEYENV[@]}" "${ALLOW[@]}"
expect_fail "a bad version is refused" "VERSION must look like"
run check.sh VERSION=1.2.3
expect_pass "release-check needs no key"
grep -q 'make -C .* check' "$SHIM_LOG" && ok "release-check runs make check" || bad "release-check runs make check"
run check.sh VERSION=9.9.9
expect_fail "a version other than the package's is refused" "ordin-desktop package is 1.2.3"
run build.sh VERSION=1.2.3 TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$PASSWORD" "${ALLOW[@]}"
expect_fail "build without the key variable stops" "TAURI_SIGNING_PRIVATE_KEY is not set"
expect_no_calls "build without the key variable built nothing" 'make|tauri build'
run build.sh VERSION=1.2.3 TAURI_SIGNING_PRIVATE_KEY="$KEY" "${ALLOW[@]}"
expect_fail "build without the password variable stops" "TAURI_SIGNING_PRIVATE_KEY_PASSWORD is not set"
expect_no_calls "build without the password built nothing" 'make|tauri build'
run publish.sh VERSION=1.2.3 "${ALLOW[@]}"
expect_fail "publish without the key variable stops" "TAURI_SIGNING_PRIVATE_KEY is not set"
mv "$REPO/release/ordin-update.pub" "$T/pub.saved"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}"
expect_fail "a missing public key stops the build" "ordin-update.pub is missing or empty"
: > "$REPO/release/ordin-update.pub"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}"
expect_fail "an empty public key stops the build" "ordin-update.pub is missing or empty"
mv "$T/pub.saved" "$REPO/release/ordin-update.pub"

# --- the bundled agents input
run build.sh VERSION=1.2.3 "${KEYENV[@]}"
expect_fail "no bundled agents input stops the build" "no bundled agents"
expect_no_calls "no bundled agents input built nothing" 'make|tauri build'
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}"
expect_pass "the allow flag builds without bundled agents"
printf '%s' "$OUTPUT" | grep -q 'WITHOUT the bundled agents' && ok "the allow flag says what it does" || bad "the allow flag says what it does"
AG="$T/agents"; mkdir -p "$AG/workspaces"; echo "profile" > "$AG/workspaces/coordinator.yaml"
git -C "$AG" init -q && git -C "$AG" add -A && git -C "$AG" -c user.name=t -c user.email=t@t commit -qm agents
COMMIT="$(git -C "$AG" rev-parse HEAD)"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" RELEASE_BUNDLED_AGENTS_SOURCE="$AG"
expect_fail "only one bundled input set stops" "set both"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" RELEASE_BUNDLED_AGENTS_SOURCE="$AG" RELEASE_BUNDLED_AGENTS_COMMIT=abc123
expect_fail "a short hash stops" "40-character"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" RELEASE_BUNDLED_AGENTS_SOURCE="$AG" RELEASE_BUNDLED_AGENTS_COMMIT=0000000000000000000000000000000000000000
expect_fail "a hash the clone lacks stops" "is not in"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" RELEASE_BUNDLED_AGENTS_SOURCE="$AG" RELEASE_BUNDLED_AGENTS_COMMIT="$COMMIT"
expect_pass "a good commit builds"
[ -f "$T/bundle/macos/ordin.app/Contents/Resources/catalogue/workspaces/coordinator.yaml" ] \
  && ok "the commit's files reach the application's catalogue folder" || bad "the commit's files reach the application's catalogue folder"
run check.sh VERSION=1.2.3 RELEASE_BUNDLED_AGENTS_SOURCE="$AG" RELEASE_BUNDLED_AGENTS_COMMIT=0000000000000000000000000000000000000000
expect_fail "release-check tests the commit too" "is not in"

# --- the memory models input (PB-REL-10 amended 2026-01-28, PB-REL-11 to PB-REL-13)
FILES="$REPO/release/memory-models/files.txt"
cp "$FILES" "$T/files.saved"
: > "$FILES"
run check.sh VERSION=1.2.3
expect_fail "release-check stops on an empty models manifest, naming the file" "release/memory-models/files.txt holds no entry"
expect_no_calls "an empty models manifest runs no gate" 'make'
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}"
expect_fail "release-build stops on an empty models manifest" "release/memory-models/files.txt holds no entry"
expect_no_calls "an empty models manifest built nothing" 'make|tauri build'
grep -v '^#' "$T/files.saved" | grep -v '^$' | head -9 > "$FILES"
run check.sh VERSION=1.2.3
expect_fail "release-check stops on nine models entries" "holds 9 entries"
sed 's#embedding/config.json#embedding/other.json#' "$T/files.saved" > "$FILES"
run check.sh VERSION=1.2.3
expect_fail "release-check stops on a path outside the ten" "embedding/other.json is not one of the ten files"
{ cat "$T/files.saved"; grep -m1 'reranker/model.onnx' "$T/files.saved"; } > "$FILES"
run check.sh VERSION=1.2.3
expect_fail "release-check stops on a path twice" "reranker/model.onnx appears twice"
awk '!done && $0 !~ /^#/ && NF { $1 = toupper($1); done = 1 } { print }' "$T/files.saved" > "$FILES"
run check.sh VERSION=1.2.3
expect_fail "release-check stops on a digest that is not lowercase hexadecimal" "the digest is not 64 lowercase hexadecimal characters"
cp "$T/files.saved" "$FILES"
for key in URL SHA256 BYTES; do
  run check.sh VERSION=1.2.3 "RELEASE_MEMORY_MODELS_$key="
  expect_fail "release-check stops when RELEASE_MEMORY_MODELS_$key is empty" "RELEASE_MEMORY_MODELS_$key is empty"
done
run check.sh VERSION=1.2.3 RELEASE_MEMORY_MODELS_URL=http://example.test/models-1.zip
expect_fail "release-check stops on an http models address" "RELEASE_MEMORY_MODELS_URL must begin with https://"
run check.sh VERSION=1.2.3 RELEASE_MEMORY_MODELS_SHA256=c97ba495
expect_fail "release-check stops on a short models fingerprint" "64 lowercase hexadecimal"
run check.sh VERSION=1.2.3 RELEASE_MEMORY_MODELS_BYTES=0
expect_fail "release-check stops on a zero models size" "a whole number above 0"
run check.sh VERSION=1.2.3 RELEASE_MEMORY_MODELS_BYTES=173,982,577
expect_fail "release-check stops on a models size with separators" "a whole number above 0"
run check.sh VERSION=1.2.3
expect_pass "release-check passes with the committed models input"
rm -rf "$T/out" "$T/bundle"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}"
expect_pass "release-build passes with the committed models input"
MANIFEST_LINE="$(grep '^MODELS_MANIFEST=' "$SHIM_LOG" | head -1)"
PAIRS="$(printf '%s' "${MANIFEST_LINE#MODELS_MANIFEST=}" | tr ',' '\n' | grep -c '^[a-z_/]*\.[a-z]*=[0-9a-f]\{64\}$')"
[ "$PAIRS" = 10 ] && ok "release-build passes ORDIN_MEMORY_MODELS_MANIFEST with ten pairs" || bad "release-build passes ORDIN_MEMORY_MODELS_MANIFEST with ten pairs (got $PAIRS)"
FIRST_PAIR="$(grep -v '^#' "$T/files.saved" | grep -v '^$' | head -1 | awk '{print $2 "=" $1}')"
printf '%s' "$MANIFEST_LINE" | grep -qF "MODELS_MANIFEST=$FIRST_PAIR," && ok "the manifest keeps the file's order" || bad "the manifest keeps the file's order"
grep -qx 'MODELS_URL=https://github.com/gatblau/ordin-releases/releases/download/models-1/ordin-memory-models-1.zip' "$SHIM_LOG" \
  && grep -qx 'MODELS_SHA256=c97ba4951a33720e317d1b95f5859f0fba075e2c0b5fa4e19bdb87694ac995d7' "$SHIM_LOG" \
  && grep -qx 'MODELS_BYTES=173982577' "$SHIM_LOG" \
  && ok "release-build passes the zip's address, fingerprint and size" || bad "release-build passes the zip's address, fingerprint and size"
grep '^CONFIG=' "$SHIM_LOG" | grep -qi 'models' && bad "the build's --config names no models resource" || ok "the build's --config names no models resource"

# --- the secret-leak check
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" SHIM_LEAK=1
expect_fail "the key found in the build log fails the build" "the key or its password appears in"
printf '%s\n' "$OUTPUT" | sed -n '/^leak-check:/,$p' | grep -q 'DONOTLEAK' && bad "the leak message does not print the key" || ok "the leak message does not print the key"
PASSWORD_LOG="$T/pw.log"; echo "the password is $PASSWORD" > "$PASSWORD_LOG"
OUTPUT="$(env TAURI_SIGNING_PRIVATE_KEY="$KEY" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$PASSWORD" "$REPO/scripts/release/leak-check.sh" "$PASSWORD_LOG" 2>&1)"; STATUS=$?
expect_fail "the password found in a file fails the check" "appears in"

# --- make release: asks once for the password
printf '%s\n' "$PASSWORD" > "$T/answer"
rm -rf "$T/out" "$T/bundle"
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer" "${ALLOW[@]}"
expect_pass "make release asks for the password and builds"
printf '%s' "$OUTPUT" | grep -qF "stopped after the build" && ok "PUBLISH=0 stops after the build" || bad "PUBLISH=0 stops after the build"
printf '%s' "$OUTPUT" | grep -qF "$PASSWORD" && bad "make release never prints the password" || ok "make release never prints the password"
grep -q 'tauri build' "$SHIM_LOG" && ok "make release ran the build" || bad "make release ran the build"
[ ! -e "$T/out/latest.json" ] && ok "PUBLISH=0 published nothing" || bad "PUBLISH=0 published nothing"
# --- GitHub sign-in (PB-REL-31): RELEASE_GITHUB_APP_CLIENT_ID reaches the build as GITHUB_APP_CLIENT_ID
grep -qx 'GITHUB_APP=' "$SHIM_LOG" && ok "an empty GitHub App client id builds with none (the token box)" || bad "an empty GitHub App client id builds with none (the token box)"
printf '%s' "$OUTPUT" | grep -qF "people paste a personal access token" && ok "the build says GitHub is connected with a token" || bad "the build says GitHub is connected with a token"
rm -rf "$T/out" "$T/bundle"
run release.sh VERSION=1.2.3 PUBLISH=0 RELEASE_GITHUB_APP_CLIENT_ID=Iv1.0a1b2c3d "${KEYENV[@]}" "${ALLOW[@]}" GITHUB_APP_CLIENT_ID=stale
expect_pass "a build with a GitHub App client id passes"
grep -qx 'GITHUB_APP=Iv1.0a1b2c3d' "$SHIM_LOG" && ok "the GitHub App client id reaches the build" || bad "the GitHub App client id reaches the build"
rm -rf "$T/out" "$T/bundle"
run release.sh VERSION=1.2.3 PUBLISH=0 "RELEASE_GITHUB_APP_CLIENT_ID=Iv1 x;rm" "${KEYENV[@]}" "${ALLOW[@]}"
expect_fail "a GitHub App client id with other characters is refused" "RELEASE_GITHUB_APP_CLIENT_ID may hold only"
expect_no_calls "a refused GitHub App client id built nothing" 'tauri build'
rm -rf "$T/out" "$T/bundle"
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/none" "${KEYENV[@]}" "${ALLOW[@]}"
expect_pass "a password already in the environment is not asked for"
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/none" "${ALLOW[@]}"
expect_fail "no terminal and no password stops" "there is no terminal to ask on"
expect_no_calls "no terminal built nothing" 'make|tauri build'
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer" SHIM_SIGN_FAIL=1 "${ALLOW[@]}"
expect_fail "a wrong password stops before the gate" "the password is wrong"
grep -q "tauri signer sign -f" "$SHIM_LOG" && ok "the signer is given the key file with -f" || bad "the signer is given the key file with -f"
expect_no_calls "a wrong password built nothing" 'make|tauri build'
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer" SHIM_MINISIGN_FAIL_PROBE=1 "${ALLOW[@]}"
expect_fail "a key that is not the committed public key's pair stops" "not that key pair"
expect_no_calls "a mismatched key built nothing" 'make|tauri build'
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$T/missing.key" RELEASE_PROMPT_INPUT="$T/answer" "${ALLOW[@]}"
expect_fail "a missing key file stops" "the update key is not at"
run release.sh VERSION=1.2 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer"
expect_fail "make release refuses a bad version" "VERSION must look like"

# make publish: BUILD=0 asks for the password, then publishes only
rm -rf "$T/out" "$T/bundle"
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer" "${ALLOW[@]}"
expect_pass "a build to publish from"
DEST4="$T/served-publish"; mkdir -p "$DEST4"; echo "Notes." > "$T/out/notes.md"; : > "$SHIM_LOG"
run release.sh BUILD=0 VERSION=1.2.3 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer" RELEASE_ALLOW_SINGLE_PLATFORM=1 "RELEASE_TEST_FEED_URL=file://$DEST4/latest.json" "RELEASE_TEST_ARTEFACT_BASE_URL=file://$DEST4" "RELEASE_TEST_UPLOAD=dir:$DEST4"
expect_pass "make publish asks for the password and publishes"
expect_no_calls "make publish built nothing" 'make|tauri build'
[ "$(jq -r '.version' "$DEST4/latest.json" 2>/dev/null)" = "1.2.3" ] && ok "make publish wrote the feed" || bad "make publish wrote the feed"
printf '%s' "$OUTPUT" | grep -qF "$PASSWORD" && bad "make publish never prints the password" || ok "make publish never prints the password"
run release.sh BUILD=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer"
expect_fail "make publish without a version stops" "give the version to publish"
run release.sh BUILD=0 PUBLISH=0 VERSION=1.2.3 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer"
expect_fail "make publish with PUBLISH=0 stops" "does nothing"
run release.sh BUILD=0 VERSION=1.2.3 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer" SHIM_SIGN_FAIL=1 RELEASE_ALLOW_SINGLE_PLATFORM=1
expect_fail "make publish with a wrong password stops" "the password is wrong"

# make release with no VERSION asks what to do
rm -rf "$T/out" "$T/bundle"
printf '\n1\ny\n%s\n' "$PASSWORD" > "$T/answers"
run release.sh TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answers"
expect_pass "make release with no version asks, then builds (answers: default version, build only, no agents, password)"
printf '%s' "$OUTPUT" | grep -qF "stopped after the build" && ok "choice 1 stops after the build" || bad "choice 1 stops after the build"
printf '%s' "$OUTPUT" | grep -qF "$PASSWORD" && bad "the interactive flow never prints the password" || ok "the interactive flow never prints the password"
rm -rf "$T/out" "$T/bundle"; DEST3="$T/served-ask"; mkdir -p "$DEST3"
printf '\n2\ny\ny\nOne line of notes.\n%s\n' "$PASSWORD" > "$T/answers"
run release.sh TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answers" "RELEASE_TEST_FEED_URL=file://$DEST3/latest.json" "RELEASE_TEST_ARTEFACT_BASE_URL=file://$DEST3" "RELEASE_TEST_UPLOAD=dir:$DEST3"
expect_pass "choice 2 builds and publishes the test feed with macOS only after being told so"
[ "$(jq -r '.platforms | keys | join(",")' "$DEST3/latest.json" 2>/dev/null)" = "darwin-aarch64" ] && ok "the asked publish carried macOS only" || bad "the asked publish carried macOS only"
[ "$(jq -r '.notes' "$DEST3/latest.json" 2>/dev/null)" = "One line of notes." ] && ok "the typed notes reached the feed" || bad "the typed notes reached the feed"
rm -rf "$T/out" "$T/bundle"
printf '\n3\n9.9.9\n' > "$T/answers"
run release.sh TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answers" "${ALLOW[@]}"
expect_fail "choice 3 asks for the version again and refuses a different one" "the version did not match"
expect_no_calls "a refused real publish built nothing" 'make|tauri build'
printf '\n7\n' > "$T/answers"
run release.sh TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answers" "${ALLOW[@]}"
expect_fail "an answer that is not 1, 2 or 3 is refused" "answer 1, 2 or 3"
run release.sh TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/missing-answers"
expect_fail "no terminal and no version stops" "there is no terminal to ask on"

# --- publish
rm -rf "$T/out" "$T/bundle"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}"
expect_pass "build for macOS"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" RELEASE_PLATFORM=windows-x86_64 RELEASE_BUNDLE_DIR="$T/bundle-win"
expect_pass "build for Windows"
for f in ordin-1.2.3-darwin-aarch64.dmg ordin-1.2.3-darwin-aarch64.dmg.sig ordin-1.2.3-darwin-aarch64.dmg.minisig \
         ordin-1.2.3-windows-x86_64.setup.exe platform-darwin-aarch64.json platform-windows-x86_64.json SHA256SUMS.darwin-aarch64; do
  [ -f "$T/out/$f" ] && ok "build wrote $f" || bad "build wrote $f"
done
DEST="$T/served"; mkdir -p "$DEST"
PUB=(RELEASE_CHANNEL=test "RELEASE_TEST_FEED_URL=file://$DEST/latest.json" "RELEASE_TEST_ARTEFACT_BASE_URL=file://$DEST" "RELEASE_TEST_UPLOAD=dir:$DEST" "${KEYENV[@]}" VERSION=1.2.3)
run publish.sh "${PUB[@]}"
expect_fail "publish stops without release notes" "no release notes"
head -c 4001 /dev/zero | tr '\0' 'a' > "$T/out/notes.md"
run publish.sh "${PUB[@]}"
expect_fail "notes over 4,000 characters are refused" "limit is 4,000"
[ ! -e "$DEST/latest.json" ] && ok "refused notes uploaded nothing" || bad "refused notes uploaded nothing"
printf 'Fixes and a new About page.\n' > "$T/out/notes.md"
run publish.sh VERSION=1.2.3 RELEASE_CHANNEL=test "${KEYENV[@]}" "RELEASE_TEST_FEED_URL=http://example.test/latest.json" "RELEASE_TEST_ARTEFACT_BASE_URL=https://example.test" "RELEASE_TEST_UPLOAD=dir:$DEST"
expect_fail "a feed address that is not https is refused" "must begin with https://"
printf 'RELEASE_TEST_FEED_URL=\n' > "$T/empty.conf"
run publish.sh VERSION=1.2.3 "${KEYENV[@]}" RELEASE_CHANNEL=test "RELEASE_CONF=$T/empty.conf"
expect_fail "publish names the empty setting" "RELEASE_TEST_FEED_URL is empty"
# a macOS-only folder: refused unless the test channel is told to allow it
mkdir -p "$T/out-mac"
cp "$T"/out/ordin-1.2.3-darwin-* "$T"/out/platform-darwin-*.json "$T"/out/SHA256SUMS.darwin-* "$T/out/notes.md" "$T/out-mac/"
DEST2="$T/served-mac"; mkdir -p "$DEST2"
PUB2=(RELEASE_CHANNEL=test "RELEASE_TEST_FEED_URL=file://$DEST2/latest.json" "RELEASE_TEST_ARTEFACT_BASE_URL=file://$DEST2" "RELEASE_TEST_UPLOAD=dir:$DEST2" "${KEYENV[@]}" VERSION=1.2.3 RELEASE_OUT_DIR="$T/out-mac")
run publish.sh "${PUB2[@]}"
expect_fail "a macOS-only publish is refused without the single-platform setting" "has no Windows build"
run publish.sh VERSION=1.2.3 RELEASE_CHANNEL=real RELEASE_ALLOW_SINGLE_PLATFORM=1 "${KEYENV[@]}" RELEASE_OUT_DIR="$T/out-mac" \
  "RELEASE_REAL_FEED_URL=file://$DEST2/latest.json" "RELEASE_REAL_ARTEFACT_BASE_URL=file://$DEST2" "RELEASE_REAL_UPLOAD=dir:$DEST2"
expect_fail "the real channel refuses a macOS-only folder even when told to allow it" "the real channel always needs both"
[ ! -e "$DEST2/latest.json" ] && ok "the refused macOS-only publishes uploaded nothing" || bad "the refused macOS-only publishes uploaded nothing"
run publish.sh "${PUB2[@]}" RELEASE_ALLOW_SINGLE_PLATFORM=1
expect_pass "the test channel publishes a macOS-only folder when told to allow it"
printf '%s' "$OUTPUT" | grep -qF "no Windows build" && ok "the macOS-only publish says it has no Windows build" || bad "the macOS-only publish says it has no Windows build"
[ "$(jq -r '.platforms | keys | join(",")' "$DEST2/latest.json")" = "darwin-aarch64" ] && ok "the macOS-only feed carries only macOS" || bad "the macOS-only feed carries only macOS"
# a changed installer is refused before anything is uploaded
cp "$T/out/ordin-1.2.3-windows-x86_64.setup.exe" "$T/setup.saved"
printf 'x' >> "$T/out/ordin-1.2.3-windows-x86_64.setup.exe"
run publish.sh "${PUB[@]}"
expect_fail "an installer changed by one byte is refused" "does not match its hash"
[ ! -e "$DEST/latest.json" ] && [ ! -d "$DEST/v1.2.3" ] && ok "the changed installer uploaded nothing" || bad "the changed installer uploaded nothing"
cp "$T/setup.saved" "$T/out/ordin-1.2.3-windows-x86_64.setup.exe"
# a read-back failure puts the previous feed back
printf '{"version":"0.0.1"}' > "$DEST/latest.json"
run publish.sh "${PUB[@]}" SHIM_MINISIGN_FAIL_READBACK=1
expect_fail "a read-back failure fails the publish" "read-back failed"
[ "$(cat "$DEST/latest.json")" = '{"version":"0.0.1"}' ] && ok "the previous feed was restored" || bad "the previous feed was restored"
rm -f "$DEST/latest.json"
run publish.sh "${PUB[@]}" SHIM_MINISIGN_FAIL_READBACK=1
expect_fail "a read-back failure with no previous feed fails the publish" "read-back failed"
[ ! -e "$DEST/latest.json" ] && ok "with no previous feed, none is left" || bad "with no previous feed, none is left"
# the full publish
rm -rf "$DEST"; mkdir -p "$DEST"
run publish.sh "${PUB[@]}"
expect_pass "a full publish to a folder target"
printf '%s' "$OUTPUT" | grep -q 'now offers 1.2.3' && ok "publish reports the offer" || bad "publish reports the offer"
jq -e '.version == "1.2.3" and .notes == "Fixes and a new About page." and (.pub_date | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$"))
       and (.platforms | keys == ["darwin-aarch64","windows-x86_64"])
       and (.platforms["windows-x86_64"].url == "file://'"$DEST"'/v1.2.3/ordin-1.2.3-windows-x86_64.setup.exe")
       and (.platforms["darwin-aarch64"].signature | length > 0)' "$DEST/latest.json" >/dev/null \
  && ok "the feed has Tauri's updater format" || bad "the feed has Tauri's updater format"
(cd "$DEST/v1.2.3" && { shasum -a 256 -c ordin-1.2.3-SHA256SUMS.txt >/dev/null 2>&1 || sha256sum -c ordin-1.2.3-SHA256SUMS.txt >/dev/null 2>&1; }) \
  && ok "the published hash file matches the installers" || bad "the published hash file matches the installers"
[ -f "$DEST/v1.2.3/ordin-1.2.3-darwin-aarch64.dmg.minisig" ] && ok "each installer has a detached signature beside it" || bad "each installer has a detached signature beside it"
printf '%s' "$OUTPUT" | grep -q "$PASSWORD" && bad "publish output omits the password" || ok "publish output omits the password"

# The container socket comes from the active docker context when DOCKER_HOST and /var/run/docker.sock are absent.
SD="$(mktemp -d /tmp/ordin-sock.XXXXXX)"
python3 -c "import socket,sys; s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1])" "$SD/d.sock" 2>/dev/null
printf '#!/usr/bin/env bash\necho "unix://%s"\n' "$SD/d.sock" > "$T/bin/docker"; chmod +x "$T/bin/docker"
if [ -S "$SD/d.sock" ] && [ ! -S /var/run/docker.sock ]; then
  GOT="$(env -u DOCKER_HOST PATH="$T/bin:$PATH" bash -c '. "$1"; use_container_socket >/dev/null; echo "$DOCKER_HOST"' _ "$REPO/scripts/release/lib.sh")"
  [ "$GOT" = "unix://$SD/d.sock" ] && ok "the docker context supplies the container socket" || bad "the docker context supplies the container socket"
fi
GOT="$(DOCKER_HOST=unix:///already PATH="$T/bin:$PATH" bash -c '. "$1"; use_container_socket >/dev/null; echo "$DOCKER_HOST"' _ "$REPO/scripts/release/lib.sh")"
[ "$GOT" = "unix:///already" ] && ok "a DOCKER_HOST already set is kept" || bad "a DOCKER_HOST already set is kept"
rm -rf "$SD"

# --- the Windows installer built on the Mac (PB-REL-15 to PB-REL-20)
XB="$T/xbin"; mkdir -p "$XB"
for t in clang llvm-rc lld-link makensis npm cargo-xwin; do printf '#!/usr/bin/env bash\nexit 0\n' > "$XB/$t"; done
cat > "$XB/rustup" <<'SH'
#!/usr/bin/env bash
if [ "$1 $2 $3" = "target list --installed" ]; then
  [ -n "${SHIM_NO_TARGET:-}" ] || echo x86_64-pc-windows-msvc
  echo aarch64-apple-darwin
fi
SH
cat > "$XB/uname" <<'SH'
#!/usr/bin/env bash
case "$1" in -s) echo "${SHIM_UNAME_S:-Darwin}" ;; -m) echo arm64 ;; esac
SH
chmod +x "$XB"/*
XCACHE="$T/xwin-cache"; mkdir -p "$XCACHE/crt/lib/x86_64" "$XCACHE/sdk"
echo "lib" > "$XCACHE/crt/lib/x86_64/libcmt.lib"; echo "sdk" > "$XCACHE/sdk/readme-sdk.txt"
XOUT="$T/out-x"
CROSS=(PATH="$XB:$PATH" RELEASE_CROSS=windows-x86_64 RELEASE_XWIN_CACHE_DIR="$XCACHE" RELEASE_BUNDLE_DIR="$T/bundle-x" RELEASE_OUT_DIR="$XOUT")
LIC=(RELEASE_ACCEPT_MSVC_LICENSE=1)

run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" RELEASE_CROSS=linux-x86_64
expect_fail "a cross target other than windows-x86_64 is refused" "RELEASE_CROSS must be windows-x86_64"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" SHIM_UNAME_S=Linux
expect_fail "the Windows build on the Mac stops on a computer that is not a Mac" "on a Mac only"
expect_no_calls "a cross build off the Mac built nothing" 'make|tauri build'
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}"
expect_fail "the Windows build on the Mac stops without the licence answer" "Visual Studio Build Tools licence"
printf '%s' "$OUTPUT" | grep -qF "https://go.microsoft.com/fwlink/?LinkId=2086102" && ok "the licence stop names the licence's address" || bad "the licence stop names the licence's address"
expect_no_calls "without the licence answer nothing was built" 'make|tauri build'
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" RELEASE_ACCEPT_MSVC_LICENSE=yes
expect_fail "a licence answer other than 1 is not an acceptance" "Visual Studio Build Tools licence"
{ cat "$REPO/release/release.conf"; echo "RELEASE_ACCEPT_MSVC_LICENSE=1"; } > "$T/licence.conf"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "RELEASE_CONF=$T/licence.conf"
expect_fail "the licence answer written in release.conf is refused" "must not be written in licence.conf"
expect_no_calls "a licence answer in the file built nothing" 'make|tauri build'
XB2="$T/xbin-nonsis"; mkdir -p "$XB2"; cp "$XB"/* "$XB2/"; rm "$XB2/makensis"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" PATH="$XB2:$PATH"
expect_fail "a missing NSIS stops, naming how to install it" "makensis is not installed (brew install nsis)"
expect_no_calls "a missing prerequisite built nothing" 'make|tauri build'
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" SHIM_NO_TARGET=1
expect_fail "a missing Rust target stops, naming the command" "rustup target add x86_64-pc-windows-msvc"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" SHIM_TAURI_VERSION=2.11.0
expect_fail "another tauri-cli stops the Windows build on the Mac" "needs tauri-cli 2.12.4 (this computer has 2.11.0)"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" RELEASE_XWIN_CACHE_DIR="$REPO/xwin"
expect_fail "a cargo-xwin cache inside the repository is refused" "is inside the repository"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" RELEASE_OUT_DIR="$XCACHE/out"
expect_fail "an output folder inside the cargo-xwin cache is refused" "is inside the cargo-xwin cache"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" RELEASE_XWIN_CACHE_DIR="$XOUT/cache"
expect_fail "a cargo-xwin cache inside the output folder is refused" "is inside the release output folder"
expect_no_calls "a misplaced cache built nothing" 'make|tauri build'

rm -rf "$XOUT" "$T/bundle" "$T/bundle-x"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${LIC[@]}" RELEASE_OUT_DIR="$XOUT"
expect_pass "the macOS build fills the folder first"
grep -qx 'XWIN_ACCEPT_LICENSE=' "$SHIM_LOG" && ok "the licence answer never reaches a native build" || bad "the licence answer never reaches a native build"
echo "macOS build log" >> "$XOUT/build.log"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}"
expect_pass "the Windows installer is built on the Mac with the licence answer"
grep -q 'make -C .* check XWIN=$' "$SHIM_LOG" && ok "the gate runs without the licence answer" || bad "the gate runs without the licence answer"
grep -q 'make -C .* desktop-build-inputs-windows XWIN=1$' "$SHIM_LOG" && ok "the Windows inputs are built with cargo-xwin's licence answer" || bad "the Windows inputs are built with cargo-xwin's licence answer"
grep -q 'make -C .* desktop-build-inputs XWIN=' "$SHIM_LOG" && bad "the cross build does not build the Mac's inputs after the gate" || ok "the cross build does not build the Mac's inputs after the gate"
grep -q 'cargo tauri build --config .* --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis$' "$SHIM_LOG" \
  && ok "tauri builds the NSIS installer with the cargo-xwin runner" || bad "tauri builds the NSIS installer with the cargo-xwin runner"
grep -qx 'XWIN_ACCEPT_LICENSE=1' "$SHIM_LOG" && grep -qx "XWIN_CACHE_DIR=$XCACHE" "$SHIM_LOG" \
  && ok "tauri's build gets the licence answer and the cache folder" || bad "tauri's build gets the licence answer and the cache folder"
grep '^CONFIG=' "$SHIM_LOG" | grep -q '"createUpdaterArtifacts": *true' && ! grep '^CONFIG=' "$SHIM_LOG" | grep -q signingIdentity \
  && ok "the cross build's --config is the Windows one" || bad "the cross build's --config is the Windows one"
grep -qx 'MODELS_BYTES=173982577' "$SHIM_LOG" && ok "the cross build gets the models input" || bad "the cross build gets the models input"
for f in ordin-1.2.3-windows-x86_64.setup.exe ordin-1.2.3-windows-x86_64.setup.exe.sig ordin-1.2.3-windows-x86_64.setup.exe.minisig \
         SHA256SUMS.windows-x86_64 platform-windows-x86_64.json build-windows-x86_64.log ordin-1.2.3-darwin-aarch64.dmg; do
  [ -f "$XOUT/$f" ] && ok "the shared folder holds $f" || bad "the shared folder holds $f"
done
grep -qx "macOS build log" "$XOUT/build.log" && ok "the macOS build.log is kept" || bad "the macOS build.log is kept"
jq -e '.cross == true and .platform == "windows-x86_64" and .file == "ordin-1.2.3-windows-x86_64.setup.exe"' "$XOUT/platform-windows-x86_64.json" >/dev/null \
  && ok "the Windows platform file says it was built on the Mac" || bad "the Windows platform file says it was built on the Mac"
jq -e 'has("cross") | not' "$XOUT/platform-darwin-aarch64.json" >/dev/null && ok "a native platform file has no cross mark" || bad "a native platform file has no cross mark"
minisign -Vm "$XOUT/ordin-1.2.3-windows-x86_64.setup.exe" -x "$XOUT/ordin-1.2.3-windows-x86_64.setup.exe.minisig" \
  && ok "the cross-built installer is signed as a native one is" || bad "the cross-built installer is signed as a native one is"

# --- the Windows programs need no runtime the PC may lack (PB-REL-24 to PB-REL-26)
grep -qx "RELEASE_WINDOWS_DIRECTML_URL=https://www.nuget.org/api/v2/package/Microsoft.AI.DirectML/1.15.4" "$SRC/release/release.conf" \
  && grep -qx "RELEASE_WINDOWS_DIRECTML_SHA256=9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1" "$SRC/release/release.conf" \
  && grep -qx "RELEASE_WINDOWS_VC_CHANNEL_URL=https://aka.ms/vs/17/release/channel" "$SRC/release/release.conf" \
  && ok "release.conf pins DirectML 1.15.4 and names the Visual Studio channel" || bad "release.conf pins DirectML 1.15.4 and names the Visual Studio channel"
RUNTIME="DirectML.dll msvcp140.dll msvcp140_1.dll vcruntime140.dll vcruntime140_1.dll "
# the cross build above staged the five files and mapped each beside the program
grep -qx "STAGED=$RUNTIME" "$SHIM_LOG" && ok "the cross build stages the five files before tauri builds" || bad "the cross build stages the five files before tauri builds ($(grep '^STAGED=' "$SHIM_LOG"))"
CONFIG_LINE="$(grep '^CONFIG=' "$SHIM_LOG" | head -1)"
MAPPED=0; for f in $RUNTIME; do printf '%s' "$CONFIG_LINE" | grep -q "\"windows-runtime/$f\": *\"$f\"" && MAPPED=$((MAPPED + 1)); done
[ "$MAPPED" = 5 ] && ok "the Windows --config puts the five files beside ordin.exe" || bad "the Windows --config puts the five files beside ordin.exe ($MAPPED)"
[ "$(cat "$REPO/apps/desktop/windows-runtime/DirectML.dll")" = "import: d3d12.dll" ] && ok "the x64 DirectML.dll is the one staged" || bad "the x64 DirectML.dll is the one staged"
grep -q 'debug' "$REPO/apps/desktop/windows-runtime/msvcp140.dll" && bad "the release msvcp140.dll is staged, not the debug one" || ok "the release msvcp140.dll is staged, not the debug one"
printf '%s' "$OUTPUT" | grep -qF "Visual C++ runtime 14.44.35211" && ok "the newest redistributable in the manifest is taken, and named" || bad "the newest redistributable in the manifest is taken, and named"
printf '%s' "$OUTPUT" | grep -qF "need no Visual C++ runtime beyond the files beside ordin.exe" && ok "the cross build checks the programs' imports" || bad "the cross build checks the programs' imports"
[ -f "$T/.cache/ordin-release/DirectML-$DML_SHA.dll" ] && [ -f "$T/.cache/ordin-release/vc-redist-$(sha_of_stdin < "$VC_PKG").zip" ] \
  && ok "the checked downloads are kept in the download cache" || bad "the checked downloads are kept in the download cache"
rm -rf "$T/out" "$T/bundle"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}"
grep '^CONFIG=' "$SHIM_LOG" | grep -q 'windows-runtime' && bad "the macOS --config names no Windows runtime file" || ok "the macOS --config names no Windows runtime file"
# a native Windows build does the same; with the packages gone it uses the cached copies, and a stale file goes
mv "$DML_PKG" "$T/directml.saved"; mv "$VC_PKG" "$T/vc.saved"
echo stale > "$REPO/apps/desktop/windows-runtime/stale.dll"
WIN=(RELEASE_PLATFORM=windows-x86_64 RELEASE_BUNDLE_DIR="$T/bundle-w" RELEASE_OUT_DIR="$T/out-w")
rm -rf "$T/out-w" "$T/bundle-w"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}"
expect_pass "a native Windows build uses the cached downloads when the packages cannot be read"
grep -qx "STAGED=$RUNTIME" "$SHIM_LOG" && ok "the native Windows build stages the five files and nothing stale" || bad "the native Windows build stages the five files and nothing stale ($(grep '^STAGED=' "$SHIM_LOG"))"
CONFIG_LINE="$(grep '^CONFIG=' "$SHIM_LOG" | head -1)"
MAPPED=0; for f in $RUNTIME; do printf '%s' "$CONFIG_LINE" | grep -q "\"windows-runtime/$f\": *\"$f\"" && MAPPED=$((MAPPED + 1)); done
[ "$MAPPED" = 5 ] && ok "the native Windows --config puts the five files beside ordin.exe" || bad "the native Windows --config puts the five files beside ordin.exe ($MAPPED)"
rm -rf "$T/.cache/ordin-release"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}"
expect_fail "an unreadable DirectML package with nothing cached stops" "could not download DirectML"
expect_no_calls "an unreadable DirectML package built nothing" 'tauri build|desktop-build-inputs'
mv "$T/directml.saved" "$DML_PKG"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}"
expect_fail "an unreadable redistributable package with nothing cached stops" "could not download file://$VC_PKG"
expect_no_calls "an unreadable redistributable package built nothing" 'tauri build|desktop-build-inputs'
mv "$T/vc.saved" "$VC_PKG"
ZEROS="$(printf '0%.0s' $(seq 64))"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_WINDOWS_DIRECTML_SHA256="$ZEROS"
expect_fail "a DirectML.dll with another fingerprint stops" "not RELEASE_WINDOWS_DIRECTML_SHA256"
expect_no_calls "a DirectML.dll with another fingerprint built nothing" 'tauri build|desktop-build-inputs'
[ ! -e "$T/.cache/ordin-release/DirectML-$ZEROS.dll" ] && ok "a DLL with another fingerprint is not cached" || bad "a DLL with another fingerprint is not cached"
printf 'bin/arm64-win/DirectML.dll\timport: other.dll\n' > "$T/other.nupkg"
rm -rf "$T/.cache/ordin-release"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_WINDOWS_DIRECTML_URL="file://$T/other.nupkg"
expect_fail "a package without the x64 DirectML.dll stops" "holds no bin/x64-win/DirectML.dll"
vc_manifest "$ZEROS" "file://$VC_PKG" > "$T/vs-manifest-bad.json"
jq -n --arg url "file://$T/vs-manifest-bad.json" '{channelItems: [{type: "Manifest", payloads: [{url: $url}]}]}' > "$T/vs-channel-bad.json"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_WINDOWS_VC_CHANNEL_URL="file://$T/vs-channel-bad.json"
expect_fail "a redistributable package with another fingerprint than the manifest's stops" "not $ZEROS"
expect_no_calls "a redistributable with another fingerprint built nothing" 'tauri build|desktop-build-inputs'
printf '{"channelItems": [{"type": "Bootstrapper"}]}\n' > "$T/vs-channel-none.json"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_WINDOWS_VC_CHANNEL_URL="file://$T/vs-channel-none.json"
expect_fail "a channel that names no manifest stops" "names no manifest"
printf '{"packages": []}\n' > "$T/vs-manifest-empty.json"
jq -n --arg url "file://$T/vs-manifest-empty.json" '{channelItems: [{type: "Manifest", payloads: [{url: $url}]}]}' > "$T/vs-channel-empty.json"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_WINDOWS_VC_CHANNEL_URL="file://$T/vs-channel-empty.json"
expect_fail "a manifest with no x64 redistributable stops" "lists no x64 Visual C++ redistributable"
grep -v 'vcruntime140_1.dll' "$VC_PKG" > "$T/vc-short.zip"
vc_manifest "$(sha_of_stdin < "$T/vc-short.zip")" "file://$T/vc-short.zip" > "$T/vs-manifest-short.json"
jq -n --arg url "file://$T/vs-manifest-short.json" '{channelItems: [{type: "Manifest", payloads: [{url: $url}]}]}' > "$T/vs-channel-short.json"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_WINDOWS_VC_CHANNEL_URL="file://$T/vs-channel-short.json"
expect_fail "a redistributable without vcruntime140_1.dll stops" "holds no x64 vcruntime140_1.dll"
for key in DIRECTML_SHA256 VC_CHANNEL_URL; do
  run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" "RELEASE_WINDOWS_$key="
  expect_fail "an empty RELEASE_WINDOWS_$key stops" "RELEASE_WINDOWS_$key is empty"
  expect_no_calls "an empty RELEASE_WINDOWS_$key runs no gate" 'make'
done
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_WINDOWS_DIRECTML_URL=http://example.test/directml.nupkg
expect_fail "an http DirectML address stops" "RELEASE_WINDOWS_DIRECTML_URL must begin with https://"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" RELEASE_PLATFORM=windows-aarch64
expect_fail "a Windows build for another processor stops" "chosen for windows-x86_64 only"
expect_no_calls "a Windows build for another processor built nothing" 'make|tauri build'
# a program that needs a Visual C++ runtime DLL the installer does not carry is refused, and not kept
rm -rf "$T/out-w"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" SHIM_PROGRAM_IMPORTS="kernel32.dll MSVCP140.dll MSVCP140_2.dll"
expect_fail "a program importing a runtime DLL that is not shipped stops the build" "ordin.exe imports MSVCP140_2.dll;"
[ ! -e "$T/out-w/ordin-1.2.3-windows-x86_64.setup.exe" ] && ok "the refused installer is not kept" || bad "the refused installer is not kept"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" SHIM_HOOK_IMPORTS="kernel32.dll vcruntime140.dll"
expect_fail "a hook importing vcruntime140.dll (any case) stops the build" "ordin-hook-x86_64-pc-windows-msvc.exe imports vcruntime140.dll;"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" SHIM_PROGRAM_IMPORTS="VCRUNTIME140_threads.dll"
expect_fail "the cross build refuses a program importing an unshipped runtime DLL" "ordin.exe imports VCRUNTIME140_threads.dll;"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${WIN[@]}" SHIM_PROGRAM_IMPORTS="kernel32.dll msvcrt.dll"
expect_pass "Windows' own msvcrt.dll is not the Visual C++ runtime"

# the leak check refuses Microsoft's files
L="$T/leak-x"; mkdir -p "$L/a" "$L/b" "$L/c"
cp "$XCACHE/crt/lib/x86_64/libcmt.lib" "$L/a/"; cp "$XCACHE/sdk/readme-sdk.txt" "$L/b/"; echo ok > "$L/c/ordin.txt"
LC=(env TAURI_SIGNING_PRIVATE_KEY="$KEY" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$PASSWORD" RELEASE_XWIN_CACHE_DIR="$XCACHE" "$REPO/scripts/release/leak-check.sh")
OUTPUT="$("${LC[@]}" "$L/a" 2>&1)"; STATUS=$?
expect_fail "a library from the cargo-xwin cache fails the leak check" "files from the cargo-xwin cache"
OUTPUT="$("${LC[@]}" "$L/b" 2>&1)"; STATUS=$?
expect_fail "a file named as one in the cargo-xwin cache fails the leak check" "readme-sdk.txt"
OUTPUT="$("${LC[@]}" "$XCACHE/sdk" 2>&1)"; STATUS=$?
expect_fail "a file inside the cargo-xwin cache fails the leak check" "files from the cargo-xwin cache"
OUTPUT="$("${LC[@]}" "$L/c" "$XOUT" 2>&1)"; STATUS=$?
expect_pass "the cross-built release output passes the leak check"
OUTPUT="$(env TAURI_SIGNING_PRIVATE_KEY="$KEY" "$REPO/scripts/release/leak-check.sh" "$L/a" 2>&1)"; STATUS=$?
expect_fail "a Windows SDK library fails the leak check with no cache present" "files from the cargo-xwin cache"

# the test feed takes it; the real feed does not, yet
printf 'Windows built on the Mac.\n' > "$XOUT/notes.md"
DEST4="$T/served-x"; mkdir -p "$DEST4"
run publish.sh VERSION=1.2.3 "${KEYENV[@]}" RELEASE_OUT_DIR="$XOUT" RELEASE_CHANNEL=test \
  "RELEASE_TEST_FEED_URL=file://$DEST4/latest.json" "RELEASE_TEST_ARTEFACT_BASE_URL=file://$DEST4" "RELEASE_TEST_UPLOAD=dir:$DEST4"
expect_pass "the test feed publishes a Windows installer built on the Mac"
[ "$(jq -r '.platforms | keys | join(",")' "$DEST4/latest.json" 2>/dev/null)" = "darwin-aarch64,windows-x86_64" ] \
  && ok "the test feed carries both platforms" || bad "the test feed carries both platforms"
DEST5="$T/served-xr"; mkdir -p "$DEST5"
run publish.sh VERSION=1.2.3 "${KEYENV[@]}" RELEASE_OUT_DIR="$XOUT" RELEASE_CHANNEL=real \
  "RELEASE_REAL_FEED_URL=file://$DEST5/latest.json" "RELEASE_REAL_ARTEFACT_BASE_URL=file://$DEST5" "RELEASE_REAL_UPLOAD=dir:$DEST5"
expect_fail "the real feed refuses a Windows installer built on the Mac" "was built on the Mac"
[ ! -e "$DEST5/latest.json" ] && [ ! -d "$DEST5/v1.2.3" ] && ok "the refused real publish uploaded nothing" || bad "the refused real publish uploaded nothing"
# --- the gate's evidence (PB-REL-21 to PB-REL-23)
# The copy becomes a repository with one commit; a clone of it stands in for the computer that runs the tests.
printf 'apps/desktop/catalogue/\ntarget/\n' > "$REPO/.gitignore"
GIT_ID=(-c user.name=t -c user.email=t@t)
git -C "$REPO" init -q && git -C "$REPO" add -A && git -C "$REPO" "${GIT_ID[@]}" commit -qm release
COMMIT_A="$(git -C "$REPO" rev-parse HEAD)"
TESTBOX="$T/testbox"
git clone -q "$REPO" "$TESTBOX"
git -C "$REPO" remote add testbox "$TESTBOX"
EV=(RELEASE_GATE=evidence RELEASE_GATE_REMOTE=testbox)

run check.sh VERSION=1.2.3 RELEASE_GATE=evidence RELEASE_GATE_REMOTE=nowhere
expect_fail "a remote that cannot be fetched stops release-check" "could not fetch the gate's records (refs/notes/ordin-gate) from the git remote 'nowhere'"
expect_no_calls "a failed fetch runs no gate" 'make'
echo stray > "$TESTBOX/stray.txt"
run_at "$TESTBOX" gate-record.sh
expect_fail "gate-record refuses a tree with an untracked file" "the working tree is not clean"
expect_no_calls "gate-record on an unclean tree runs no gate" 'make'
rm -f "$TESTBOX/stray.txt"
run_at "$TESTBOX" gate-record.sh SHIM_MAKE_FAIL=1
expect_fail "a failed gate records nothing" "the gate failed on $COMMIT_A"
git -C "$TESTBOX" notes --ref=refs/notes/ordin-gate show "$COMMIT_A" >/dev/null 2>&1 \
  && bad "a failed gate wrote no note" || ok "a failed gate wrote no note"
run_at "$TESTBOX" gate-record.sh
expect_pass "gate-record records a passed gate"
grep -q 'make -C .* check' "$SHIM_LOG" && ok "gate-record runs make check" || bad "gate-record runs make check"
git -C "$TESTBOX" notes --ref=refs/notes/ordin-gate show "$COMMIT_A" 2>/dev/null | grep -q '^check passed [0-9]\{4\}-[0-9-]*T[0-9:]*Z ' \
  && ok "the note says the check passed, with the time" || bad "the note says the check passed, with the time"

# --- the journeys' evidence (PB-REL-27, PB-REL-28)
# A stand-in for the SSH command to the Windows build machine: it logs each script (its single quotes
# doubled, as PowerShell reads them) and passes, except
# the journeys' run when SHIM_JOURNEYS_FAIL is set.
cat > "$T/bin/winshell" <<'SH'
#!/usr/bin/env bash
echo "win $*" >> "$SHIM_LOG"
case "$*" in *"playwright test"*) [ -z "${SHIM_JOURNEYS_FAIL:-}" ] || exit 1 ;; esac
exit 0
SH
chmod +x "$T/bin/winshell"
# A stand-in agents repository; its commit is copied into the repository the Windows machine reads.
AGJ="$T/agents-j"; mkdir -p "$AGJ"; echo agent > "$AGJ/a.txt"
git -C "$AGJ" init -q && git -C "$AGJ" add -A && git -C "$AGJ" "${GIT_ID[@]}" commit -qm agents
JW=(JOURNEYS_WINDOWS_SSH="$T/bin/winshell" RELEASE_BUNDLED_AGENTS_SOURCE="$AGJ" RELEASE_BUNDLED_AGENTS_COMMIT="$(git -C "$AGJ" rev-parse HEAD)")
run_at "$TESTBOX" ../journeys-windows.sh JOURNEYS_WINDOWS_SSH="$T/bin/winshell" RELEASE_BUNDLED_AGENTS_SOURCE="$AGJ" RELEASE_BUNDLED_AGENTS_COMMIT=0123456789012345678901234567890123456789
expect_fail "an agents commit on no branch stops make journeys-windows" "is on no branch of $AGJ"
expect_no_calls "an agents commit on no branch builds nothing" '^win '

run check.sh VERSION=1.2.3 "${EV[@]}"
expect_fail "a commit whose gate passed but with no journeys record stops release-check" "could not fetch the journeys' records (refs/notes/ordin-journeys) from the git remote 'testbox'"
printf '%s' "$OUTPUT" | grep -qF "make journeys-windows COMMIT=$COMMIT_A" && ok "the refusal says how to record the journeys" || bad "the refusal says how to record the journeys"
run_at "$TESTBOX" ../journeys-windows.sh "${JW[@]}" SHIM_JOURNEYS_FAIL=1
expect_fail "failed journeys fail make journeys-windows" "journeys: FAILED on $COMMIT_A"
grep -qF -- 'taskkill //F //IM ordin.exe' "$SHIM_LOG" && ok "failed journeys still stop ordin.exe on the Windows machine" || bad "failed journeys still stop ordin.exe on the Windows machine"
git -C "$TESTBOX" notes --ref=refs/notes/ordin-journeys show "$COMMIT_A" >/dev/null 2>&1 \
  && bad "failed journeys write no note" || ok "failed journeys write no note"
run_at "$TESTBOX" ../journeys-windows.sh "${JW[@]}" GREP=Kept
expect_pass "some journeys passing passes make journeys-windows"
git -C "$TESTBOX" notes --ref=refs/notes/ordin-journeys show "$COMMIT_A" >/dev/null 2>&1 \
  && bad "a run of some journeys (GREP) writes no note" || ok "a run of some journeys (GREP) writes no note"
grep -qF -- "playwright test --grep ''Kept''" "$SHIM_LOG" && ok "GREP reaches Playwright" || bad "GREP reaches Playwright"
run_at "$TESTBOX" ../journeys-windows.sh "${JW[@]}"
expect_pass "make journeys-windows records every journey passing"
grep -qF -- "git checkout -q -f ''$COMMIT_A''" "$SHIM_LOG" && ok "make journeys-windows builds the commit" || bad "make journeys-windows builds the commit"
git -C "$TESTBOX" cat-file -e "$(git -C "$AGJ" rev-parse HEAD)^{commit}" && ok "the agents commit is in the repository the Windows machine reads" || bad "the agents commit is in the repository the Windows machine reads"
grep -q 'cargo tauri build --no-bundle --features journeys' "$SHIM_LOG" && ok "make journeys-windows builds with the journeys switch" || bad "make journeys-windows builds with the journeys switch"
git -C "$TESTBOX" notes --ref=refs/notes/ordin-journeys show "$COMMIT_A" 2>/dev/null | grep -q '^journeys passed [0-9]\{4\}-[0-9-]*T[0-9:]*Z windows' \
  && ok "the journeys' note says they passed, with the time" || bad "the journeys' note says they passed, with the time"

run check.sh VERSION=1.2.3 "${EV[@]}"
expect_pass "release-check passes on the recorded commit"
printf '%s' "$OUTPUT" | grep -qF "the journeys passed on $COMMIT_A" && ok "release-check names the journeys' record" || bad "release-check names the journeys' record"
expect_no_calls "release-check with the record runs no make check" 'make'
printf '%s' "$OUTPUT" | grep -qF "the gate passed on $COMMIT_A" && ok "release-check names the commit the record covers" || bad "release-check names the commit the record covers"
rm -rf "$T/out" "$T/bundle"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${EV[@]}"
expect_pass "release-build passes on the recorded commit"
expect_no_calls "release-build with the record runs no make check" 'make -C .* check'
grep -q 'tauri build' "$SHIM_LOG" && ok "release-build with the record builds" || bad "release-build with the record builds"
rm -rf "$XOUT" "$T/bundle-x"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${CROSS[@]}" "${LIC[@]}" "${EV[@]}"
expect_pass "the Windows build on the Mac passes on the recorded commit"
expect_no_calls "the Windows build on the Mac with the record runs no make check" 'make -C .* check'
rm -rf "$T/out" "$T/bundle"
run release.sh VERSION=1.2.3 PUBLISH=0 TAURI_SIGNING_PRIVATE_KEY="$KEY" RELEASE_PROMPT_INPUT="$T/answer" "${ALLOW[@]}" "${EV[@]}"
expect_pass "make release passes on the recorded commit"
expect_no_calls "make release with the record runs no make check" 'make -C .* check'

echo "# a change" >> "$REPO/.gitignore"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${EV[@]}"
expect_fail "a changed file stops release-build" "the working tree is not clean"
printf '%s' "$OUTPUT" | grep -qF ".gitignore" && ok "the refusal names the changed file" || bad "the refusal names the changed file"
expect_no_calls "a changed file built nothing" 'make|tauri build'
git -C "$REPO" checkout -q -- .gitignore
echo stray > "$REPO/stray.txt"
run check.sh VERSION=1.2.3 "${EV[@]}"
expect_fail "an untracked file stops release-check" "the working tree is not clean"
rm -f "$REPO/stray.txt"

echo b > "$REPO/b.txt"; git -C "$REPO" add b.txt; git -C "$REPO" "${GIT_ID[@]}" commit -qm b
COMMIT_B="$(git -C "$REPO" rev-parse HEAD)"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${EV[@]}"
expect_fail "a commit with no record stops release-build, naming it" "there is no record that the gate passed on commit $COMMIT_B"
printf '%s' "$OUTPUT" | grep -qF "make gate-record" && ok "the refusal says how to record the gate" || bad "the refusal says how to record the gate"
expect_no_calls "a commit with no record built nothing" 'make|tauri build'
git -C "$TESTBOX" fetch -q origin && git -C "$TESTBOX" checkout -q --detach "$COMMIT_B"
run_at "$TESTBOX" gate-record.sh
expect_pass "gate-record records the second commit"
run build.sh VERSION=1.2.3 "${KEYENV[@]}" "${ALLOW[@]}" "${EV[@]}"
expect_fail "a commit whose gate passed but whose journeys have no record stops release-build" "there is no record that the journeys passed on commit $COMMIT_B"
expect_no_calls "a commit with no journeys record built nothing" 'make|tauri build'
run check.sh VERSION=1.2.3
expect_pass "RELEASE_GATE=local passes on a commit with no record"
grep -q 'make -C .* check' "$SHIM_LOG" && ok "RELEASE_GATE=local runs make check" || bad "RELEASE_GATE=local runs make check"
run check.sh VERSION=1.2.3 RELEASE_GATE=local SHIM_MAKE_FAIL=1
if [ "$STATUS" -ne 0 ] && ! printf '%s' "$OUTPUT" | grep -qF "release-check passed"; then ok "RELEASE_GATE=local stops when make check fails"; else bad "RELEASE_GATE=local stops when make check fails"; fi
run check.sh VERSION=1.2.3 RELEASE_GATE=none
expect_pass "RELEASE_GATE=none passes on the test channel with no record"
printf '%s' "$OUTPUT" | grep -qF "WARNING: RELEASE_GATE=none" && ok "RELEASE_GATE=none says no test was checked" || bad "RELEASE_GATE=none says no test was checked"
expect_no_calls "RELEASE_GATE=none runs no gate" 'make'
run check.sh VERSION=1.2.3 RELEASE_GATE=none RELEASE_CHANNEL=real
expect_fail "RELEASE_GATE=none is refused on the real channel" "RELEASE_GATE=none is for the test channel only"
run check.sh VERSION=1.2.3 RELEASE_GATE=skip
expect_fail "another RELEASE_GATE value is refused" "RELEASE_GATE must be evidence, local or none"
expect_no_calls "a refused RELEASE_GATE runs no gate" 'make'

# --- the Windows build started from this computer (PB-REL-29)
# A stand-in for the SSH command to the Windows machine that also takes what is sent to it: the key
# (`cat >`), the password (`read -r`), and answers the commit check, the build and the copy back.
WINOUT="$T/winout"; mkdir -p "$WINOUT"
printf 'installer' > "$WINOUT/ordin-1.2.3-windows-x86_64.setup.exe"
printf 'untrusted comment: stand-in signature\nSIG-%s\n' "$(sha_of_stdin < "$WINOUT/ordin-1.2.3-windows-x86_64.setup.exe")" > "$WINOUT/ordin-1.2.3-windows-x86_64.setup.exe.minisig"
printf '%s  ordin-1.2.3-windows-x86_64.setup.exe\n' "$(sha_of_stdin < "$WINOUT/ordin-1.2.3-windows-x86_64.setup.exe")" > "$WINOUT/SHA256SUMS.windows-x86_64"
echo '{"platform":"windows-x86_64"}' > "$WINOUT/platform-windows-x86_64.json"; echo log > "$WINOUT/build-windows-x86_64.log"
WINOUT_NOSIG="$T/winout-nosig"; cp -r "$WINOUT" "$WINOUT_NOSIG"; rm -f "$WINOUT_NOSIG"/*.minisig
cat > "$T/bin/winbuild" <<'SH'
#!/usr/bin/env bash
D="${SHIM_LOG%/*}"
echo "win $*" >> "$SHIM_LOG"
case "$*" in
  *"cat > "*) cat > "$D/winkey" ;;
  *"read -r TAURI_SIGNING_PRIVATE_KEY_PASSWORD"*) read -r p; printf '%s' "$p" > "$D/winpw"; echo BUILD_RAN >> "$SHIM_LOG"; [ -z "${SHIM_WIN_BUILD_FAIL:-}" ] || exit 1 ;;
  *"git cat-file -e"*) [ -z "${SHIM_WIN_NO_COMMIT:-}" ] || exit 1 ;;
  *"rm -f ~/.ordin-release-key"*) rm -f "$D/winkey"; echo KEY_REMOVED >> "$SHIM_LOG" ;;
  *"tar -c ordin-"*) tar -c -C "${SHIM_WIN_OUT:-$D/winout}" . ;;
  *"tar -c *.exe"*) tar -c -C "${SHIM_WIN_OUT:-$D/winout}" ordin-1.2.3-windows-x86_64.setup.exe ;;
  *"make local-build"*) echo LOCAL_BUILD_RAN >> "$SHIM_LOG"; [ -z "${SHIM_WIN_BUILD_FAIL:-}" ] || exit 1 ;;
esac
exit 0
SH
chmod +x "$T/bin/winbuild"
WR=(RELEASE_WINDOWS_SSH="$T/bin/winbuild")

WR_COMMIT="$(git -C "$REPO" rev-parse HEAD)"
run build-windows-remote.sh VERSION=1.2 "${KEYENV[@]}" "${WR[@]}"
expect_fail "a bad version is refused by the Windows build from here" "VERSION must look like"
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" SHIM_SIGN_FAIL=1
expect_fail "a wrong password stops the Windows build from here" "the password is wrong"
expect_no_calls "a wrong password sends nothing to the Windows machine" '^win '
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" SHIM_WIN_NO_COMMIT=1
expect_fail "a commit the Windows machine cannot read builds nothing" "cannot read the commit $WR_COMMIT"
expect_no_calls "an unreadable commit sends no key and runs no build" 'cat > |BUILD_RAN|release-build'
run build-windows-remote.sh VERSION=1.2.3 TAURI_SIGNING_PRIVATE_KEY="$T/no-such.key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD=x "${WR[@]}"
expect_fail "a missing update key stops the Windows build from here" "the update key is not at"

WR_OUT="$T/out-wr"; rm -rf "$WR_OUT"
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" RELEASE_OUT_DIR="$WR_OUT"
expect_pass "the Windows build from here passes"
grep -qF -- "umask 077; cat > ~/.ordin-release-key" "$SHIM_LOG" && ok "the key goes to the Windows machine under umask 077" || bad "the key goes to the Windows machine under umask 077"
grep -qF -- "git checkout -q -f ''$WR_COMMIT''" "$SHIM_LOG" && ok "the Windows build is of this repository's commit" || bad "the Windows build is of this repository's commit"
grep -qF -- "make release-build VERSION=''1.2.3''" "$SHIM_LOG" && ok "the Windows machine runs make release-build" || bad "the Windows machine runs make release-build"
grep -qF -- "RELEASE_GITHUB_APP_CLIENT_ID='''' make release-build" "$SHIM_LOG" && ok "the Windows release build passes the GitHub App client id" || bad "the Windows release build passes the GitHub App client id"
grep -qx KEY_REMOVED "$SHIM_LOG" && [ ! -e "$T/winkey" ] && ok "the key is removed from the Windows machine after the build" || bad "the key is removed from the Windows machine after the build"
[ "$(cat "$T/winpw" 2>/dev/null)" = "$PASSWORD" ] && ok "the password reaches the build through standard input" || bad "the password reaches the build through standard input"
grep -qF "$PASSWORD" "$SHIM_LOG" && bad "the password is in no command sent to the Windows machine" || ok "the password is in no command sent to the Windows machine"
printf '%s' "$OUTPUT" | grep -qF "$PASSWORD" && bad "the Windows build output omits the password" || ok "the Windows build output omits the password"
[ -f "$WR_OUT/ordin-1.2.3-windows-x86_64.setup.exe" ] && [ -f "$WR_OUT/ordin-1.2.3-windows-x86_64.setup.exe.minisig" ] && [ -f "$WR_OUT/platform-windows-x86_64.json" ] \
  && ok "the Windows files come back into the release folder" || bad "the Windows files come back into the release folder"

# The tests run with RELEASE_GATE=local (see the top), so that is the choice the Windows build is given.
grep -qF -- "export RELEASE_GATE=''local''" "$SHIM_LOG" && ok "the Windows build is given the gate choice made here" || bad "the Windows build is given the gate choice made here"
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" RELEASE_OUT_DIR="$T/out-wr-none" RELEASE_GATE=none
expect_pass "the Windows build from here passes with RELEASE_GATE=none on the test channel"
grep -qF -- "export RELEASE_GATE=''none''" "$SHIM_LOG" && grep -qF -- "export RELEASE_CHANNEL=''test''" "$SHIM_LOG" \
  && ok "RELEASE_GATE=none and the channel go to the Windows build" || bad "RELEASE_GATE=none and the channel go to the Windows build"
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" RELEASE_GATE=none RELEASE_CHANNEL=real
expect_fail "RELEASE_GATE=none is refused for a real Windows build" "RELEASE_GATE=none is for the test channel only"
expect_no_calls "a refused Windows build sends nothing" 'BUILD_RAN'
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" "RELEASE_GATE=x;rm"
expect_fail "another RELEASE_GATE value is refused for the Windows build" "RELEASE_GATE must be evidence, local or none"

WR_OUT2="$T/out-wr2"; rm -rf "$WR_OUT2"
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" RELEASE_OUT_DIR="$WR_OUT2" SHIM_WIN_BUILD_FAIL=1
expect_fail "a failed Windows build fails the script" "the build on the Windows machine failed"
grep -qx KEY_REMOVED "$SHIM_LOG" && [ ! -e "$T/winkey" ] && ok "a failed Windows build still removes the key there" || bad "a failed Windows build still removes the key there"
[ ! -e "$WR_OUT2/ordin-1.2.3-windows-x86_64.setup.exe" ] && ok "a failed Windows build copies no file back" || bad "a failed Windows build copies no file back"

WR_OUT3="$T/out-wr3"; rm -rf "$WR_OUT3"
run build-windows-remote.sh VERSION=1.2.3 "${KEYENV[@]}" "${WR[@]}" RELEASE_OUT_DIR="$WR_OUT3" SHIM_WIN_OUT="$WINOUT_NOSIG"
expect_fail "an installer that comes back without its signature is refused" "came back without its signature"

# --- trying a build by hand on Windows (PB-REL-30)
LB_OUT="$T/out-lb"; rm -rf "$LB_OUT"
run local-build-windows-remote.sh "${WR[@]}" LOCAL_BUILD_OUT_DIR="$LB_OUT" AGENTS=bogus
expect_fail "an AGENTS value that is not commit, worktree or none is refused" "AGENTS is commit, worktree or none"
expect_no_calls "a refused AGENTS value sends nothing to the Windows machine" '^win '
run local-build-windows-remote.sh "${WR[@]}" LOCAL_BUILD_OUT_DIR="$LB_OUT"
expect_pass "the Windows build by hand passes"
git -C "$TESTBOX" rev-parse -q --verify refs/heads/local-build >/dev/null && ok "the build is pushed to the Windows machine's repository under local-build" || bad "the build is pushed to the Windows machine's repository under local-build"
grep -qx LOCAL_BUILD_RAN "$SHIM_LOG" && ok "the Windows machine runs make local-build" || bad "the Windows machine runs make local-build"
grep -qF -- "RELEASE_GITHUB_APP_CLIENT_ID='''' make local-build" "$SHIM_LOG" && ok "the Windows build by hand passes an empty GitHub App client id" || bad "the Windows build by hand passes an empty GitHub App client id"
grep -qE 'ordin-release-key|release-build|^BUILD_RAN$|KEY_REMOVED' "$SHIM_LOG" && bad "a build by hand sends no key, runs no release build and asks for no password" || ok "a build by hand sends no key, runs no release build and asks for no password"
grep -qE 'gate|journeys|playwright' "$SHIM_LOG" && bad "a build by hand runs no gate and no journeys" || ok "a build by hand runs no gate and no journeys"
[ -f "$LB_OUT/ordin-1.2.3-windows-x86_64.setup.exe" ] && ok "the installer comes back into the local build folder" || bad "the installer comes back into the local build folder"
printf '%s' "$OUTPUT" | grep -qF "not signed" && ok "the script says the installer is not signed" || bad "the script says the installer is not signed"
run local-build-windows-remote.sh "${WR[@]}" LOCAL_BUILD_OUT_DIR="$LB_OUT" RELEASE_GITHUB_APP_CLIENT_ID=Iv1.0a1b2c3d
grep -qF -- "RELEASE_GITHUB_APP_CLIENT_ID=''Iv1.0a1b2c3d'' make local-build" "$SHIM_LOG" && ok "the Windows build by hand passes the GitHub App client id" || bad "the Windows build by hand passes the GitHub App client id"
run local-build-windows-remote.sh "${WR[@]}" LOCAL_BUILD_OUT_DIR="$LB_OUT" "RELEASE_GITHUB_APP_CLIENT_ID=a'b"
expect_fail "the Windows build by hand refuses a GitHub App client id with other characters" "RELEASE_GITHUB_APP_CLIENT_ID may hold only"
expect_no_calls "a refused GitHub App client id sends nothing to the Windows machine" '^win '
LB_OUT2="$T/out-lb2"; rm -rf "$LB_OUT2"
run local-build-windows-remote.sh "${WR[@]}" LOCAL_BUILD_OUT_DIR="$LB_OUT2" SHIM_WIN_BUILD_FAIL=1
expect_fail "a failed build by hand fails the script" "the build on the Windows machine failed"
[ ! -e "$LB_OUT2/ordin-1.2.3-windows-x86_64.setup.exe" ] && ok "a failed build by hand brings nothing back" || bad "a failed build by hand brings nothing back"

printf '\n%s passed, %s failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
