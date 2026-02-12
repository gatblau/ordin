#!/usr/bin/env bash
# make release-build VERSION=<version>: Playbook 6.5 steps 1 and 2 for the computer it runs on
# (PB-REL-6, PB-REL-7). A macOS installer is built on macOS and a Windows installer on Windows.
# make release-build-windows VERSION=<version> RELEASE_ACCEPT_MSVC_LICENSE=1: the option to build the
# Windows installer on the Mac with cargo-xwin (RELEASE_CROSS=windows-x86_64, PB-REL-15 to PB-REL-19).
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf
require_version
# spec: ordin-desktop/phase-5-playbook#PB-REL-3
require_key
# spec: ordin-desktop/phase-5-playbook#PB-REL-4
require_public_key
need_tool jq
need_tool git
need_tool cargo
cargo tauri --version >/dev/null 2>&1 || die "cargo-tauri is not installed (cargo install tauri-cli --locked)"
require_package_version

detect_platform() {
  local os arch
  case "$(uname -s)" in
    Darwin) os=darwin ;;
    MINGW*|MSYS*|CYGWIN*) os=windows ;;
    *) die "installers are built on macOS or Windows only" ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64|ARM64) arch=aarch64 ;;
    x86_64|AMD64) arch=x86_64 ;;
    *) die "this computer's processor is not one ordin ships for" ;;
  esac
  printf '%s-%s' "$os" "$arch"
}
# spec: ordin-desktop/phase-5-playbook#PB-REL-15
CROSS=0
case "${RELEASE_CROSS:-}" in
  '') PLATFORM="${RELEASE_PLATFORM:-$(detect_platform)}" ;;
  windows-x86_64)
    [ "$(uname -s)" = Darwin ] || die "RELEASE_CROSS=windows-x86_64 builds the Windows installer on a Mac only; on Windows run make release-build"
    CROSS=1
    PLATFORM=windows-x86_64 ;;
  *) die "RELEASE_CROSS must be windows-x86_64 or unset (it is ${RELEASE_CROSS})" ;;
esac
case "$PLATFORM" in darwin-*) IS_MAC=1 ;; windows-*) IS_MAC=0 ;; *) die "unknown platform $PLATFORM" ;; esac
# spec: ordin-desktop/phase-5-playbook#PB-REL-25
if [ "$IS_MAC" = 0 ]; then require_windows_runtime_input "$PLATFORM"; fi

# spec: ordin-desktop/phase-5-playbook#PB-REL-7
HAVE_BUNDLED=0
if bundled_inputs_set; then
  HAVE_BUNDLED=1
elif [ "${RELEASE_ALLOW_NO_BUNDLED_AGENTS:-}" = 1 ]; then
  note "WARNING: building WITHOUT the bundled agents; the application will answer PKG_BUNDLED_AGENT_MISSING."
else
  die "no bundled agents: set RELEASE_BUNDLED_AGENTS_SOURCE and RELEASE_BUNDLED_AGENTS_COMMIT in release/release.conf (OQ-REC-13), or write RELEASE_ALLOW_NO_BUNDLED_AGENTS=1 to build without them"
fi

# spec: ordin-desktop/phase-5-playbook#PB-REL-12
require_models_input

OUT="${RELEASE_OUT_DIR:-$ROOT/target/release-out/$VERSION}"
if [ "$CROSS" = 1 ]; then
  # The licence, the cache and the tools are checked before the gate, so a missing one stops before
  # anything is built or downloaded.
  # spec: ordin-desktop/phase-5-playbook#PB-REL-17
  require_msvc_licence
  # spec: ordin-desktop/phase-5-playbook#PB-REL-18
  XWIN_CACHE="$(xwin_cache_dir)"
  require_cache_outside "$XWIN_CACHE" "$OUT"
  # spec: ordin-desktop/phase-5-playbook#PB-REL-16
  XPATH="$(windows_cross_path)"
  require_cross_tools "$XPATH"
  BUNDLE="${RELEASE_BUNDLE_DIR:-$ROOT/target/$WINDOWS_TRIPLE/release/bundle}"
  # The macOS build's build.log stays when both builds share the output folder.
  LOG_NAME="build-$PLATFORM.log"
else
  BUNDLE="${RELEASE_BUNDLE_DIR:-$ROOT/target/release/bundle}"
  # A build on the Windows machine names its log as the cross build does: the Mac brings it back beside
  # the macOS build's build.log (make release-build-windows-remote).
  if [ "$IS_MAC" = 1 ]; then LOG_NAME="build.log"; else LOG_NAME="build-$PLATFORM.log"; fi
fi
mkdir -p "$OUT"
LOG="$OUT/$LOG_NAME"
: > "$LOG"

note "step 1: the gate for $VERSION"
# spec: ordin-desktop/phase-5-playbook#PB-REL-21
release_gate

CATALOGUE="$ROOT/apps/desktop/catalogue"
rm -rf "$CATALOGUE"
if [ "$HAVE_BUNDLED" = 1 ]; then
  mkdir -p "$CATALOGUE"
  git -C "$RELEASE_BUNDLED_AGENTS_SOURCE" archive "$RELEASE_BUNDLED_AGENTS_COMMIT" | tar -x -C "$CATALOGUE"
  [ -n "$(find "$CATALOGUE" -type f | head -1)" ] || die "commit $RELEASE_BUNDLED_AGENTS_COMMIT holds no file"
  note "bundled agents: commit $RELEASE_BUNDLED_AGENTS_COMMIT copied into apps/desktop/catalogue"
fi

# spec: ordin-desktop/phase-5-playbook#PB-REL-6
CONFIG="$(mktemp)"
trap 'rm -f "$CONFIG"' EXIT
# On Windows DirectML.dll and the Visual C++ runtime go beside ordin.exe (PB-REL-25).
jq -n --arg pk "$(tauri_public_key)" --arg feed "${RELEASE_REAL_FEED_URL:-}" \
  --argjson catalogue "$HAVE_BUNDLED" --argjson mac "$IS_MAC" \
  --arg runtime "${WINDOWS_RUNTIME_DIR#apps/desktop/}" --arg names "DirectML.dll $VC_RUNTIME_FILES" '
  ((if $catalogue == 1 then {"catalogue/": "catalogue/"} else {} end)
   + (if $mac == 0 then ($names | split(" ") | map({key: "\($runtime)/\(.)", value: .}) | from_entries) else {} end)) as $resources
  | {bundle: ({createUpdaterArtifacts: true}
            + (if $resources != {} then {resources: $resources} else {} end)
            + (if $mac == 1 then {macOS: {signingIdentity: "-"}} else {} end)),
   plugins: {updater: ({pubkey: $pk} + (if $feed != "" then {endpoints: [$feed]} else {} end))}}' > "$CONFIG"

if [ -n "${RELEASE_REAL_FEED_URL:-}" ]; then
  check_https RELEASE_REAL_FEED_URL "$RELEASE_REAL_FEED_URL"
  export ORDIN_UPDATE_FEED_URL="$RELEASE_REAL_FEED_URL"
fi

# The memory models are a build input, not a resource: the build is told the zip's address, size and
# fingerprint and the ten file digests, and copies no model file (PB-REL-13, DSK-MD-2).
# spec: ordin-desktop/phase-5-playbook#PB-REL-13
export ORDIN_MEMORY_MODELS_URL="$RELEASE_MEMORY_MODELS_URL"
export ORDIN_MEMORY_MODELS_SHA256="$RELEASE_MEMORY_MODELS_SHA256"
export ORDIN_MEMORY_MODELS_BYTES="$RELEASE_MEMORY_MODELS_BYTES"
export ORDIN_MEMORY_MODELS_MANIFEST="$MODELS_MANIFEST"
github_app_env
microsoft_app_env

note "step 2: building the installers for $PLATFORM"
# spec: ordin-desktop/phase-5-playbook#PB-REL-25
if [ "$IS_MAC" = 0 ]; then stage_windows_runtime 2>&1 | tee -a "$LOG"; fi
if [ "$CROSS" = 1 ]; then
  # The licence answer, the cache folder and Homebrew's LLVM reach the Windows steps only, never the gate.
  # spec: ordin-desktop/phase-5-playbook#PB-REL-19
  note "building the Windows installer on this Mac with cargo-xwin (Microsoft's files are cached in $XWIN_CACHE)"
  env PATH="$XPATH" XWIN_ACCEPT_LICENSE=1 XWIN_CACHE_DIR="$XWIN_CACHE" \
    make -C "$ROOT" desktop-build-inputs-windows 2>&1 | tee -a "$LOG"
  (cd "$ROOT/apps/desktop" && env PATH="$XPATH" XWIN_ACCEPT_LICENSE=1 XWIN_CACHE_DIR="$XWIN_CACHE" \
    cargo tauri build --config "$CONFIG" --runner cargo-xwin --target "$WINDOWS_TRIPLE" --bundles nsis) 2>&1 | tee -a "$LOG"
else
  make -C "$ROOT" desktop-build-inputs 2>&1 | tee -a "$LOG"
  # CI=true makes Tauri's DMG step skip the Finder icon layout, which fails intermittently
  # (AppleScript error -1728) and is unavailable on headless machines.
  # PB-REL-37: on macOS Tauri builds the app (and its update archive) only; the disk image is made after it.
  ONLY_APP=()
  if [ "$IS_MAC" = 1 ]; then ONLY_APP=(--bundles app); fi
  (cd "$ROOT/apps/desktop" && CI=true cargo tauri build --config "$CONFIG" ${ONLY_APP[@]+"${ONLY_APP[@]}"}) 2>&1 | tee -a "$LOG"
  if [ "$IS_MAC" = 1 ]; then make_dmg "$BUNDLE" "$VERSION" 2>&1 | tee -a "$LOG"; fi
fi

# The Windows programs need no Visual C++ runtime the PC may lack (PB-REL-24), checked before the
# installer is kept: the hook imports none; the program and the shipped DLLs import only those shipped.
# spec: ordin-desktop/phase-5-playbook#PB-REL-26
if [ "$IS_MAC" = 0 ]; then
  if [ "$CROSS" = 1 ]; then IMPORTS_PATH="$XPATH"; else IMPORTS_PATH="$PATH"; fi
  check_windows_imports "$IMPORTS_PATH" "" "$ROOT"/apps/desktop/binaries/ordin-hook-*.exe 2>&1 | tee -a "$LOG"
  check_windows_imports "$IMPORTS_PATH" "$(windows_runtime_names)" "$(dirname "$BUNDLE")/ordin.exe" \
    "$ROOT/$WINDOWS_RUNTIME_DIR"/*.dll 2>&1 | tee -a "$LOG"
  note "the Windows programs need no Visual C++ runtime beyond the files beside ordin.exe" | tee -a "$LOG"
fi

pick() { # pick <dir> <glob>: the one file that matches
  local found
  found="$(find "$1" -maxdepth 1 -type f -name "$2" 2>/dev/null | head -1)"
  [ -n "$found" ] || die "the build produced no $2 in $1"
  printf '%s' "$found"
}

FILES=""
add_file() { # add_file <source> <extension>
  local dest="$OUT/ordin-$VERSION-$PLATFORM.$2"
  cp "$1" "$dest"
  FILES="$FILES
$dest"
}
if [ "$IS_MAC" = 1 ]; then
  add_file "$(pick "$BUNDLE/dmg" '*.dmg')" dmg
  add_file "$(pick "$BUNDLE/macos" '*.app.tar.gz')" app.tar.gz
  UPDATER="$OUT/ordin-$VERSION-$PLATFORM.app.tar.gz"
else
  add_file "$(pick "$BUNDLE/nsis" '*-setup.exe')" setup.exe
  UPDATER="$OUT/ordin-$VERSION-$PLATFORM.setup.exe"
fi

: > "$OUT/SHA256SUMS.$PLATFORM"
for f in $FILES; do
  # The key and password reach the signer through its environment only (PB-REL-3).
  sign_file "$f" >> "$LOG" 2>&1
  [ -s "$f.sig" ] || die "the signer wrote no signature for $(basename "$f")"
  base64 -d < "$f.sig" > "$f.minisig"
  printf '%s  %s\n' "$(sha256_of "$f")" "$(basename "$f")" >> "$OUT/SHA256SUMS.$PLATFORM"
done
# A Windows installer built on the Mac says so, for PB-REL-20.
# spec: ordin-desktop/phase-5-playbook#PB-REL-15
jq -n --arg platform "$PLATFORM" --arg file "$(basename "$UPDATER")" --rawfile sig "$UPDATER.sig" --argjson cross "$CROSS" \
  '{platform: $platform, file: $file, signature: ($sig | rtrimstr("\n"))} + (if $cross == 1 then {cross: true} else {} end)' \
  > "$OUT/platform-$PLATFORM.json"

# spec: ordin-desktop/phase-5-playbook#PB-REL-7
if [ "$HAVE_BUNDLED" = 1 ]; then
  if [ "$IS_MAC" = 1 ] && [ -z "${RELEASE_SKIP_APP_CHECK:-}" ]; then
    APP_CAT="$BUNDLE/macos/ordin.app/Contents/Resources/catalogue"
  else
    APP_CAT="$CATALOGUE"
  fi
  [ -n "$(find "$APP_CAT" -type f 2>/dev/null | head -1)" ] \
    || die "the built application's catalogue folder holds no file ($APP_CAT)"
  note "the application's catalogue folder holds the bundled agents"
fi

# spec: ordin-desktop/phase-5-playbook#PB-REL-9
# spec: ordin-desktop/phase-5-playbook#PB-REL-18
if [ "$CROSS" = 1 ]; then
  RELEASE_XWIN_CACHE_DIR="$XWIN_CACHE" "$(dirname "$0")/leak-check.sh" "$OUT"
else
  "$(dirname "$0")/leak-check.sh" "$OUT"
fi
note "release-build passed: $PLATFORM installers are in $OUT"
