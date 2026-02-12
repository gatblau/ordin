#!/usr/bin/env bash
# Shared by the release targets (Playbook 6.5 amendment January 2026, PB-REL-1 to PB-REL-9).
# Sourced, never run. Written for bash 3.2 (macOS) and later. Never turn on `set -x` here or in a
# caller: the key's password must not reach a log.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PUBLIC_KEY_FILE="${RELEASE_PUBLIC_KEY_FILE:-$ROOT/release/ordin-update.pub}"

die() { printf 'release: %s\n' "$*" >&2; exit 1; }
note() { printf 'release: %s\n' "$*"; }
need_tool() { command -v "$1" >/dev/null 2>&1 || die "$1 is not installed${2:+ ($2)}"; }

# PB-REL-31: the GitHub App client id the desktop program compiles in (`option_env!("GITHUB_APP_CLIENT_ID")`).
# Empty: GITHUB_APP_CLIENT_ID is removed from the build's environment, so GitHub is connected with a token.
# spec: ordin-desktop/phase-5-playbook#PB-REL-31
github_app_env() {
  local id="${RELEASE_GITHUB_APP_CLIENT_ID:-}"
  [[ "$id" =~ ^[A-Za-z0-9._-]{0,128}$ ]] \
    || die "RELEASE_GITHUB_APP_CLIENT_ID may hold only letters, digits, '.', '_' and '-' (at most 128)"
  if [ -n "$id" ]; then
    export GITHUB_APP_CLIENT_ID="$id"
    note "GitHub sign-in: the GitHub App $id (device sign-in)"
  else
    unset GITHUB_APP_CLIENT_ID
    note "GitHub sign-in: no GitHub App (RELEASE_GITHUB_APP_CLIENT_ID is empty); people paste a personal access token"
  fi
}

# PB-REL-36: the Microsoft app client id the desktop program compiles in (`option_env!("MICROSOFT_CLIENT_ID")`).
# Empty: MICROSOFT_CLIENT_ID is removed from the build's environment, so Microsoft 365 needs a company profile.
# spec: ordin-desktop/phase-5-playbook#PB-REL-36
microsoft_app_env() {
  local id="${RELEASE_MICROSOFT_CLIENT_ID:-}"
  [[ "$id" =~ ^[A-Za-z0-9._-]{0,128}$ ]] \
    || die "RELEASE_MICROSOFT_CLIENT_ID may hold only letters, digits, '.', '_' and '-' (at most 128)"
  if [ -n "$id" ]; then
    export MICROSOFT_CLIENT_ID="$id"
    note "Microsoft sign-in: the Microsoft app $id (no company profile needed)"
  else
    unset MICROSOFT_CLIENT_ID
    note "Microsoft sign-in: no Microsoft app (RELEASE_MICROSOFT_CLIENT_ID is empty); Microsoft 365 needs a company profile"
  fi
}

# PB-REL-1: KEY=value lines; a value already in the environment wins.
load_conf() {
  local conf="${RELEASE_CONF:-$ROOT/release/release.conf}" line key value
  [ -f "$conf" ] || die "missing $conf"
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in ''|'#'*) continue ;; esac
    key="${line%%=*}"
    value="${line#*=}"
    [[ "$key" =~ ^RELEASE_[A-Z0-9_]+$ ]] || die "$conf: this line is not KEY=value: $line"
    if [ -z "${!key+x}" ]; then export "$key=$value"; fi
  done < "$conf"
}

# PB-REL-2.
require_version() {
  [ -n "${VERSION:-}" ] || die "give the version: VERSION=<version>"
  [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
    || die "VERSION must look like 1.2.3 or 1.2.3-test.1"
}

package_version() {
  local id
  id="$(cd "$ROOT" && cargo pkgid -p ordin-desktop)" || die "could not read the ordin-desktop version"
  id="${id##*[#@]}"
  printf '%s' "$id"
}

require_package_version() {
  local have
  have="$(package_version)"
  [ "$have" = "$VERSION" ] || die "VERSION is $VERSION but the ordin-desktop package is $have; change one"
}

# PB-REL-3: names the variable, never prints a value.
require_key() {
  [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] \
    || die "TAURI_SIGNING_PRIVATE_KEY is not set. Set it to the path of the update key file; nothing was built."
  [ -f "$TAURI_SIGNING_PRIVATE_KEY" ] && [ -r "$TAURI_SIGNING_PRIVATE_KEY" ] \
    || die "TAURI_SIGNING_PRIVATE_KEY does not name a readable file; nothing was built."
  [ -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+x}" ] \
    || die "TAURI_SIGNING_PRIVATE_KEY_PASSWORD is not set; nothing was built."
  export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
}

# PB-REL-4.
require_public_key() {
  [ -s "$PUBLIC_KEY_FILE" ] \
    || die "release/ordin-update.pub is missing or empty. The maintainer makes the key pair once; see Playbook 6.5 (PB-REL-4)."
}

# The string Tauri's configuration wants: base64 of the whole minisign file, on one line.
tauri_public_key() { base64 < "$PUBLIC_KEY_FILE" | tr -d '\n'; }

# PB-REL-5.
channel() {
  CHANNEL="${RELEASE_CHANNEL:-test}"
  case "$CHANNEL" in
    test) CH=TEST ;;
    real) CH=REAL ;;
    *) die "RELEASE_CHANNEL must be test or real" ;;
  esac
}

chan_value() { local name="RELEASE_${CH}_$1"; printf '%s' "${!name:-}"; }

require_chan_value() {
  local v
  v="$(chan_value "$1")"
  [ -n "$v" ] || die "RELEASE_${CH}_$1 is empty in release/release.conf"
  printf '%s' "$v"
}

check_https() {
  case "$2" in
    https://*) ;;
    file://*) [ "${RELEASE_ALLOW_FILE_URLS:-}" = 1 ] || die "$1 must begin with https://" ;;
    *) die "$1 must begin with https://" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# PB-REL-7: returns 1 when neither input is set; stops on a half-set or bad input.
bundled_inputs_set() {
  local src="${RELEASE_BUNDLED_AGENTS_SOURCE:-}" commit="${RELEASE_BUNDLED_AGENTS_COMMIT:-}"
  if [ -z "$src" ] && [ -z "$commit" ]; then return 1; fi
  [ -n "$src" ] && [ -n "$commit" ] \
    || die "set both RELEASE_BUNDLED_AGENTS_SOURCE and RELEASE_BUNDLED_AGENTS_COMMIT, or neither"
  [[ "$commit" =~ ^[0-9a-f]{40}$ ]] \
    || die "RELEASE_BUNDLED_AGENTS_COMMIT must be the full 40-character hash"
  git -C "$src" cat-file -e "${commit}^{commit}" 2>/dev/null \
    || die "commit $commit is not in $src"
  return 0
}

# PB-REL-11 and PB-REL-12: the memory models input. Stops, naming the setting or the line, on any fault;
# otherwise sets MODELS_MANIFEST to the ten "<path>=<sha256>" pairs joined by "," in the file's order
# (PB-REL-13). Reads no network address. There is no flag to build without the models.
# spec: ordin-desktop/phase-5-playbook#PB-REL-11
# spec: ordin-desktop/phase-5-playbook#PB-REL-12
MODEL_PATHS="embedding/model.onnx embedding/tokenizer.json embedding/config.json embedding/special_tokens_map.json embedding/tokenizer_config.json reranker/model.onnx reranker/tokenizer.json reranker/config.json reranker/special_tokens_map.json reranker/tokenizer_config.json"
require_models_input() {
  local files="$ROOT/release/memory-models/files.txt" name="release/memory-models/files.txt"
  local key line digest path rest count=0 number=0 seen=" " pairs=""
  for key in RELEASE_MEMORY_MODELS_URL RELEASE_MEMORY_MODELS_SHA256 RELEASE_MEMORY_MODELS_BYTES; do
    [ -n "${!key:-}" ] || die "$key is empty in release/release.conf"
  done
  case "$RELEASE_MEMORY_MODELS_URL" in https://*) ;; *) die "RELEASE_MEMORY_MODELS_URL must begin with https://" ;; esac
  [[ "$RELEASE_MEMORY_MODELS_SHA256" =~ ^[0-9a-f]{64}$ ]] \
    || die "RELEASE_MEMORY_MODELS_SHA256 must be 64 lowercase hexadecimal characters"
  [[ "$RELEASE_MEMORY_MODELS_BYTES" =~ ^[0-9]+$ ]] && [[ "$RELEASE_MEMORY_MODELS_BYTES" =~ [1-9] ]] \
    || die "RELEASE_MEMORY_MODELS_BYTES must be the zip's size in bytes, a whole number above 0 with no separators"
  [ -f "$files" ] || die "$name is missing"
  while IFS= read -r line || [ -n "$line" ]; do
    number=$((number + 1))
    case "$line" in ''|'#'*) continue ;; esac
    read -r digest path rest <<< "$line"
    [[ "$digest" =~ ^[0-9a-f]{64}$ ]] \
      || die "$name line $number: the digest is not 64 lowercase hexadecimal characters"
    case " $MODEL_PATHS " in *" $path "*) ;; *) die "$name line $number: $path is not one of the ten files the Memory Service checks" ;; esac
    case "$seen" in *" $path "*) die "$name line $number: $path appears twice" ;; esac
    seen="$seen$path "
    count=$((count + 1))
    pairs="${pairs:+$pairs,}$path=$digest"
  done < "$files"
  [ "$count" -gt 0 ] || die "$name holds no entry: the release needs the ten memory model files and their digests"
  [ "$count" -eq 10 ] || die "$name holds $count entries, not the ten files the Memory Service checks"
  MODELS_MANIFEST="$pairs"
}

# Signs one file with the update key. The signer reads TAURI_SIGNING_PRIVATE_KEY as the key's text, not
# as a path, so the key file goes in with -f and the variable is left out of the signer's environment;
# the password still reaches it through its environment variable only (PB-REL-3, PB-REL-6).
sign_file() { env -u TAURI_SIGNING_PRIVATE_KEY cargo tauri signer sign -f "$TAURI_SIGNING_PRIVATE_KEY" "$1"; }

# The key and its password sign a small file that verifies against the committed public key, so a wrong
# password, or a key of another pair, stops before anything leaves this computer or is built (PB-REL-3, PB-REL-29).
check_signing_key() {
  local dir out
  dir="$(mktemp -d)"
  printf 'ordin key check\n' > "$dir/probe"
  if ! out="$(sign_file "$dir/probe" 2>&1)" || [ ! -s "$dir/probe.sig" ]; then
    rm -rf "$dir"
    case "$out" in
      *"Wrong password"*) die "the key could not sign: the password is wrong. Nothing was built." ;;
      *) die "the key could not sign: $(printf '%s' "$out" | tail -n 1 | cut -c1-200). Nothing was built." ;;
    esac
  fi
  base64 -d < "$dir/probe.sig" > "$dir/probe.minisig"
  if ! minisign -Vm "$dir/probe" -p "$PUBLIC_KEY_FILE" -x "$dir/probe.minisig" >/dev/null 2>&1; then
    rm -rf "$dir"
    die "the key signs, but not as release/ordin-update.pub: this is not that key pair. Nothing was built."
  fi
  rm -rf "$dir"
  note "the key and the password are good"
}

# The gate's PostgreSQL tests need a Docker-compatible socket. With DOCKER_HOST unset and no
# /var/run/docker.sock, the active `docker context` (Colima keeps its socket elsewhere) supplies it.
use_container_socket() {
  [ -n "${DOCKER_HOST:-}" ] && return 0
  [ -S /var/run/docker.sock ] && return 0
  command -v docker >/dev/null 2>&1 || return 0
  local host
  host="$(docker context inspect --format '{{.Endpoints.docker.Host}}' 2>/dev/null || true)"
  case "$host" in
    unix://*) [ -S "${host#unix://}" ] || return 0
      export DOCKER_HOST="$host"
      export TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE="${TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE:-/var/run/docker.sock}"
      note "using the container socket $host" ;;
  esac
}

# The gate and the build use the Rust release that rust-toolchain.toml pins. A package manager's own
# Rust earlier on PATH (Homebrew's, say) is newer and lints differently, so when the running rustc is
# not the pinned one, rustup's folder goes first. Without a pin file, or without rustup, nothing changes.
use_pinned_toolchain() {
  local pin have
  [ -f "$ROOT/rust-toolchain.toml" ] || return 0
  pin="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml" | head -1)"
  [ -n "$pin" ] || return 0
  have="$(cd "$ROOT" && rustc --version 2>/dev/null | awk '{print $2}')"
  [ "$have" = "$pin" ] && return 0
  [ -x "$HOME/.cargo/bin/rustc" ] || return 0
  export PATH="$HOME/.cargo/bin:$PATH"
  note "using Rust $pin from rustup (this computer's default rustc is ${have:-missing})"
}

# PB-REL-21 to PB-REL-23: the gate before a release. By default the release computer runs no test: it
# requires the evidence that `make check` passed on exactly the commit being built, a git note in
# GATE_NOTES_REF written by `make gate-record` on the computer that runs the tests. RELEASE_GATE=local
# runs `make check` here instead.
GATE_NOTES_REF=refs/notes/ordin-gate
GATE_PASSED_PREFIX="check passed "
# PB-REL-27 and PB-REL-28: the journeys' evidence, a second note written by `make journeys-windows`.
JOURNEYS_NOTES_REF=refs/notes/ordin-journeys
JOURNEYS_PASSED_PREFIX="journeys passed "

# Stops, naming the files, when the working tree has a change or an untracked file git does not ignore.
# spec: ordin-desktop/phase-5-playbook#PB-REL-21
require_clean_tree() { # require_clean_tree <what stops>
  local changes
  changes="$(git -C "$ROOT" status --porcelain 2>/dev/null)" \
    || die "$ROOT is not a git working tree, so the gate's evidence cannot be read. $1"
  [ -z "$changes" ] \
    || die "the working tree is not clean, so it is not the commit the gate checked. Commit or remove these, then try again: $(printf '%s' "$changes" | awk '{print $NF}' | head -5 | tr '\n' ' ')$1"
}

# spec: ordin-desktop/phase-5-playbook#PB-REL-21
require_gate_evidence() {
  local remote="${RELEASE_GATE_REMOTE:-origin}" head err record
  need_tool git
  require_clean_tree "Nothing was built."
  head="$(git -C "$ROOT" rev-parse HEAD)"
  if ! err="$(git -C "$ROOT" fetch -q "$remote" "+$GATE_NOTES_REF:$GATE_NOTES_REF" 2>&1)"; then
    die "could not fetch the gate's records ($GATE_NOTES_REF) from the git remote '$remote': $(printf '%s' "$err" | tail -n 1 | cut -c1-200). Check that the remote is reachable and that make gate-record has run there (or set RELEASE_GATE_REMOTE to the remote that holds them). Nothing was built."
  fi
  record="$(git -C "$ROOT" notes --ref="$GATE_NOTES_REF" show "$head" 2>/dev/null | head -n 1 || true)"
  case "$record" in
    "$GATE_PASSED_PREFIX"*) note "the gate passed on $head: $record (from '$remote')" ;;
    *) die "there is no record that the gate passed on commit $head in '$remote'. On the computer that runs the tests, check out $head and run make gate-record, then try again; or give RELEASE_GATE=local to run make check here. Nothing was built." ;;
  esac
  require_journeys_evidence "$remote" "$head"
}

# The journeys on the Windows build machine passed on exactly this commit (PB-REL-27).
# spec: ordin-desktop/phase-5-playbook#PB-REL-27
require_journeys_evidence() { # require_journeys_evidence <remote> <commit>
  local remote="$1" head="$2" err record
  if ! err="$(git -C "$ROOT" fetch -q "$remote" "+$JOURNEYS_NOTES_REF:$JOURNEYS_NOTES_REF" 2>&1)"; then
    die "could not fetch the journeys' records ($JOURNEYS_NOTES_REF) from the git remote '$remote': $(printf '%s' "$err" | tail -n 1 | cut -c1-200). On the computer that runs the tests, run make journeys-windows COMMIT=$head, then try again. Nothing was built."
  fi
  record="$(git -C "$ROOT" notes --ref="$JOURNEYS_NOTES_REF" show "$head" 2>/dev/null | head -n 1 || true)"
  case "$record" in
    "$JOURNEYS_PASSED_PREFIX"*) note "the journeys passed on $head: $record (from '$remote')" ;;
    *) die "there is no record that the journeys passed on commit $head in '$remote'. On the computer that runs the tests, run make journeys-windows COMMIT=$head, then try again. Nothing was built." ;;
  esac
}

# The gate, as RELEASE_GATE chooses it (PB-REL-6 and PB-REL-19's step 1).
# spec: ordin-desktop/phase-5-playbook#PB-REL-23
release_gate() {
  case "${RELEASE_GATE:-evidence}" in
    evidence) require_gate_evidence ;;
    local)
      make -C "$ROOT" check
      note "WARNING: RELEASE_GATE=local runs make check only; nothing here shows that the journeys passed on this commit (PB-REL-27)." ;;
    # PB-REL-32: the test channel may build with no gate, to try a fix quickly; the real channel never.
    # spec: ordin-desktop/phase-5-playbook#PB-REL-32
    none)
      [ "${RELEASE_CHANNEL:-test}" = test ] \
        || die "RELEASE_GATE=none is for the test channel only; a real release needs the gate and the journeys. Nothing was built."
      need_tool git
      require_clean_tree "Nothing was built."
      note "WARNING: RELEASE_GATE=none: no test was run or checked for commit $(git -C "$ROOT" rev-parse HEAD). This build is for the test channel only." ;;
    *) die "RELEASE_GATE must be evidence, local or none (it is ${RELEASE_GATE}). Nothing was built." ;;
  esac
}

# PB-REL-15 to PB-REL-18: the option to build the Windows installer on the Mac with cargo-xwin.
WINDOWS_TRIPLE=x86_64-pc-windows-msvc
MSVC_LICENSE_URL="https://go.microsoft.com/fwlink/?LinkId=2086102"

# PB-REL-17: the maintainer accepts Microsoft's licence on the command line, every time. The answer is
# never defaulted, so a committed release.conf cannot give it for whoever clones the repository.
# spec: ordin-desktop/phase-5-playbook#PB-REL-17
require_msvc_licence() {
  local conf="${RELEASE_CONF:-$ROOT/release/release.conf}"
  if grep -q '^RELEASE_ACCEPT_MSVC_LICENSE=' "$conf" 2>/dev/null; then
    die "RELEASE_ACCEPT_MSVC_LICENSE must not be written in $(basename "$conf"); give it on the command line. Nothing was built."
  fi
  [ "${RELEASE_ACCEPT_MSVC_LICENSE:-}" = 1 ] \
    || die "building the Windows installer on the Mac downloads Microsoft's Windows SDK and C runtime under the Visual Studio Build Tools licence ($MSVC_LICENSE_URL). Read it, then add RELEASE_ACCEPT_MSVC_LICENSE=1 to accept it. Nothing was built or downloaded."
}

# PB-REL-18: where cargo-xwin keeps Microsoft's files.
xwin_cache_dir() { printf '%s' "${RELEASE_XWIN_CACHE_DIR:-$HOME/.cache/cargo-xwin}"; }

# An absolute path with "." and ".." resolved, for a folder that need not exist yet.
abs_path() {
  local p="$1" out="" part
  case "$p" in /*) ;; *) p="$PWD/$p" ;; esac
  local IFS=/
  for part in $p; do
    case "$part" in ''|.) ;; ..) out="${out%/*}" ;; *) out="$out/$part" ;; esac
  done
  printf '%s' "${out:-/}"
}

# PB-REL-18: the cache stays outside the repository and the output folder, and the output folder outside it.
# spec: ordin-desktop/phase-5-playbook#PB-REL-18
require_cache_outside() { # require_cache_outside <cache> <out>
  local cache out root
  cache="$(abs_path "$1")"; out="$(abs_path "$2")"; root="$(abs_path "$ROOT")"
  case "$cache/" in "$root"/*) die "the cargo-xwin cache ($cache) is inside the repository; set RELEASE_XWIN_CACHE_DIR to a folder outside it. Nothing was built." ;; esac
  case "$cache/" in "$out"/*) die "the cargo-xwin cache ($cache) is inside the release output folder; set RELEASE_XWIN_CACHE_DIR elsewhere. Nothing was built." ;; esac
  case "$out/" in "$cache"/*) die "the release output folder ($out) is inside the cargo-xwin cache; set RELEASE_OUT_DIR elsewhere. Nothing was built." ;; esac
}

# PB-REL-16: Homebrew keeps its llvm out of PATH (macOS has its own clang), so the Windows steps get the
# bin folders of Homebrew's llvm and lld first. The gate keeps the PATH it had.
windows_cross_path() {
  local p="$PATH" f prefix
  if command -v brew >/dev/null 2>&1; then
    for f in lld llvm; do
      prefix="$(brew --prefix "$f" 2>/dev/null || true)"
      if [ -n "$prefix" ] && [ -d "$prefix/bin" ]; then p="$prefix/bin:$p"; fi
    done
  fi
  printf '%s' "$p"
}

# PB-REL-16: stops, naming the first missing prerequisite and how to install it.
# spec: ordin-desktop/phase-5-playbook#PB-REL-16
require_cross_tools() { # require_cross_tools <PATH for the Windows steps>
  local xpath="$1" have
  (
    PATH="$xpath"
    need_tool clang "brew install llvm"
    need_tool llvm-rc "brew install llvm"
    need_tool lld-link "brew install lld"
    need_tool makensis "brew install nsis"
    need_tool npm "brew install node"
    need_tool cargo-xwin "cargo install --locked cargo-xwin"
    need_tool rustup "https://rustup.rs"
  ) || exit 1
  # Read whole before matching: `grep -q` closing the pipe early made rustup fail under pipefail.
  have="$(cd "$ROOT" && PATH="$xpath" rustup target list --installed 2>/dev/null || true)"
  case "
$have
" in *"
$WINDOWS_TRIPLE
"*) ;; *) false ;; esac \
    || die "the Rust target $WINDOWS_TRIPLE is not installed for the pinned toolchain; run: rustup target add $WINDOWS_TRIPLE (in the repository folder)"
  have="$(cargo tauri --version 2>/dev/null | awk '{print $2}')"
  [ "$have" = 2.12.4 ] \
    || die "the Windows build on the Mac needs tauri-cli 2.12.4 (this computer has ${have:-none}); run: cargo install tauri-cli --version 2.12.4 --locked"
}

# PB-REL-24 to PB-REL-26: a Windows installer that starts on a PC with nothing but Windows on it.
# Beside ordin.exe go DirectML.dll (Microsoft's DirectML package at a pinned address, checked against the
# pinned fingerprint of the DLL) and the four Visual C++ runtime DLLs ONNX Runtime's prebuilt library needs
# (the newest Visual C++ redistributable package of the Visual Studio channel, checked against the
# fingerprint the channel's manifest gives for it).
# The folder the build fills for Tauri (ignored by git); the --config maps each file in it beside the program.
WINDOWS_RUNTIME_DIR="apps/desktop/windows-runtime"
DIRECTML_MEMBER="bin/x64-win/DirectML.dll"
VC_RUNTIME_FILES="msvcp140.dll msvcp140_1.dll vcruntime140.dll vcruntime140_1.dll"

# spec: ordin-desktop/phase-5-playbook#PB-REL-25
require_windows_runtime_input() { # require_windows_runtime_input <platform>
  local key
  [ "$1" = windows-x86_64 ] \
    || die "the Windows build for $1 is not supported: the files shipped beside ordin.exe are chosen for windows-x86_64 only. Nothing was built."
  for key in RELEASE_WINDOWS_DIRECTML_URL RELEASE_WINDOWS_DIRECTML_SHA256 RELEASE_WINDOWS_VC_CHANNEL_URL; do
    [ -n "${!key:-}" ] || die "$key is empty in release/release.conf"
  done
  check_https RELEASE_WINDOWS_DIRECTML_URL "$RELEASE_WINDOWS_DIRECTML_URL"
  check_https RELEASE_WINDOWS_VC_CHANNEL_URL "$RELEASE_WINDOWS_VC_CHANNEL_URL"
  [[ "$RELEASE_WINDOWS_DIRECTML_SHA256" =~ ^[0-9a-f]{64}$ ]] \
    || die "RELEASE_WINDOWS_DIRECTML_SHA256 must be 64 lowercase hexadecimal characters"
  need_tool curl
  need_tool unzip
}

# download_checked <url> <sha256> <kept file>: downloads the file once; a kept copy with that fingerprint is
# reused. Stops, keeping nothing, when the download fails or the fingerprint differs.
download_checked() {
  local url="$1" want="$2" kept="$3" tmp have
  if [ -f "$kept" ] && [ "$(sha256_of "$kept")" = "$want" ]; then return 0; fi
  mkdir -p "$(dirname "$kept")"
  tmp="$(mktemp "$kept.XXXXXX")"
  note "downloading $url"
  if ! curl -fsSL -o "$tmp" "$url"; then rm -f "$tmp"; die "could not download $url. Nothing was built."; fi
  have="$(sha256_of "$tmp")"
  if [ "$have" != "$want" ]; then
    rm -f "$tmp"; die "the file at $url has the fingerprint $have, not $want. Nothing was built."
  fi
  mv "$tmp" "$kept"
}

# Fills WINDOWS_RUNTIME_DIR, emptied first. Downloads are kept in RELEASE_DOWNLOAD_CACHE_DIR (default
# ~/.cache/ordin-release) under their fingerprint, so each package is downloaded once.
# spec: ordin-desktop/phase-5-playbook#PB-REL-25
stage_windows_runtime() {
  local cache="${RELEASE_DOWNLOAD_CACHE_DIR:-$HOME/.cache/ordin-release}" dir="$ROOT/$WINDOWS_RUNTIME_DIR"
  local want="$RELEASE_WINDOWS_DIRECTML_SHA256" kept tmp have manifest_url package name member
  rm -rf "$dir"
  mkdir -p "$dir" "$cache"

  # DirectML.dll: the pin is the DLL's own fingerprint, so the package is checked after extraction.
  kept="$cache/DirectML-$want.dll"
  if [ ! -f "$kept" ] || [ "$(sha256_of "$kept")" != "$want" ]; then
    tmp="$(mktemp "$cache/directml.XXXXXX")"
    note "downloading $RELEASE_WINDOWS_DIRECTML_URL"
    if ! curl -fsSL -o "$tmp" "$RELEASE_WINDOWS_DIRECTML_URL"; then
      rm -f "$tmp"; die "could not download DirectML from $RELEASE_WINDOWS_DIRECTML_URL. Nothing was built."
    fi
    if ! unzip -p "$tmp" "$DIRECTML_MEMBER" > "$tmp.dll" 2>/dev/null || [ ! -s "$tmp.dll" ]; then
      rm -f "$tmp" "$tmp.dll"; die "the package at $RELEASE_WINDOWS_DIRECTML_URL holds no $DIRECTML_MEMBER. Nothing was built."
    fi
    rm -f "$tmp"
    have="$(sha256_of "$tmp.dll")"
    if [ "$have" != "$want" ]; then
      rm -f "$tmp.dll"
      die "DirectML.dll from $RELEASE_WINDOWS_DIRECTML_URL has the fingerprint $have, not RELEASE_WINDOWS_DIRECTML_SHA256 ($want). Nothing was built."
    fi
    mv "$tmp.dll" "$kept"
  fi
  cp "$kept" "$dir/DirectML.dll"
  note "DirectML.dll ($want) goes beside ordin.exe"

  # The Visual C++ runtime: the newest x64 redistributable package the channel's manifest lists.
  tmp="$(mktemp -d "$cache/channel.XXXXXX")"
  if ! curl -fsSL -o "$tmp/channel.json" "$RELEASE_WINDOWS_VC_CHANNEL_URL"; then
    rm -rf "$tmp"; die "could not download the Visual Studio channel from $RELEASE_WINDOWS_VC_CHANNEL_URL. Nothing was built."
  fi
  manifest_url="$(jq -r 'first(.channelItems[]? | select(.type == "Manifest") | .payloads[0].url) // empty' "$tmp/channel.json" 2>/dev/null || true)"
  [ -n "$manifest_url" ] || { rm -rf "$tmp"; die "the Visual Studio channel at $RELEASE_WINDOWS_VC_CHANNEL_URL names no manifest. Nothing was built."; }
  check_https "the channel's manifest address" "$manifest_url"
  if ! curl -fsSL -o "$tmp/manifest.json" "$manifest_url"; then
    rm -rf "$tmp"; die "could not download the Visual Studio manifest from $manifest_url. Nothing was built."
  fi
  package="$(jq -r '[.packages[]? | select(.id | test("^Microsoft\\.VC\\.[0-9.]+\\.CRT\\.Redist\\.X64\\.base$"))
                      | select((.payloads | length) == 1)]
                     | sort_by(.version | split(".") | map(tonumber? // 0)) | last
                     | if . == null then empty else "\(.version) \(.payloads[0].sha256 | ascii_downcase) \(.payloads[0].url)" end' \
               "$tmp/manifest.json" 2>/dev/null || true)"
  rm -rf "$tmp"
  [ -n "$package" ] || die "the Visual Studio manifest lists no x64 Visual C++ redistributable package. Nothing was built."
  set -- $package
  [[ "$2" =~ ^[0-9a-f]{64}$ ]] || die "the Visual Studio manifest gives no fingerprint for the Visual C++ redistributable $1. Nothing was built."
  check_https "the Visual C++ redistributable's address" "$3"
  kept="$cache/vc-redist-$2.zip"
  download_checked "$3" "$2" "$kept"
  for name in $VC_RUNTIME_FILES; do
    member="$(unzip -Z1 "$kept" 2>/dev/null | grep -E "/x64/Microsoft\.VC[0-9]+\.CRT/$name\$" | head -1 || true)"
    [ -n "$member" ] || die "the Visual C++ redistributable $1 holds no x64 $name. Nothing was built."
    unzip -p "$kept" "$member" > "$dir/$name" 2>/dev/null && [ -s "$dir/$name" ] \
      || die "could not take $name from the Visual C++ redistributable $1. Nothing was built."
  done
  note "the Visual C++ runtime $1 ($VC_RUNTIME_FILES, package $2) goes beside ordin.exe"
}

# The files staged beside ordin.exe, one name per line.
windows_runtime_names() { (cd "$ROOT/$WINDOWS_RUNTIME_DIR" 2>/dev/null && ls -1); }

# A tool that lists a Windows program's imported DLLs (`-p` prints "DLL Name: <name>" lines): llvm-objdump
# or objdump on the given PATH, else the llvm-objdump of the pinned Rust toolchain's llvm-tools component.
pe_imports_tool() { # pe_imports_tool <PATH to search>
  local t sysroot
  for t in llvm-objdump objdump; do
    if (PATH="$1"; command -v "$t" >/dev/null 2>&1); then (PATH="$1"; command -v "$t"); return 0; fi
  done
  sysroot="$(cd "$ROOT" && rustc --print sysroot 2>/dev/null)" || return 1
  t="$(find "$sysroot/lib/rustlib" -path '*/bin/llvm-objdump*' -type f 2>/dev/null | head -1)"
  [ -n "$t" ] || return 1
  printf '%s' "$t"
}

# Stops, naming the file and the DLLs, when a Windows program or shipped DLL imports a Visual C++ runtime
# DLL (MSVCP140*.dll or VCRUNTIME140*.dll, any case) that is not among the allowed names, which a PC
# without the redistributable lacks.
# spec: ordin-desktop/phase-5-playbook#PB-REL-26
check_windows_imports() { # check_windows_imports <PATH to search> "<allowed names>" <file>...
  local tool allowed f dlls d hits found=""
  tool="$(pe_imports_tool "$1")" \
    || die "no llvm-objdump or objdump to read the Windows programs' imports; run: rustup component add llvm-tools-preview (in the repository folder)"
  allowed=" $(printf '%s' "$2" | tr 'A-Z' 'a-z' | tr '\n' ' ') "
  shift 2
  for f in "$@"; do
    [ -f "$f" ] || die "the build produced no $f"
    dlls="$("$tool" -p "$f" 2>/dev/null | sed -n 's/^[[:space:]]*DLL Name:[[:space:]]*//p' | tr -d '\r')"
    [ -n "$dlls" ] || die "could not read the imported DLLs of $f with $tool"
    hits=""
    for d in $(printf '%s\n' "$dlls" | grep -iE '^(msvcp140|vcruntime140)[^/]*\.dll$' || true); do
      case "$allowed" in *" $(printf '%s' "$d" | tr 'A-Z' 'a-z') "*) ;; *) hits="$hits $d" ;; esac
    done
    if [ -n "$hits" ]; then found="$found $(basename "$f") imports$hits;"; fi
  done
  [ -z "$found" ] \
    || die "a Windows program needs Microsoft's Visual C++ runtime, which a PC without the redistributable lacks and the installer does not carry:$found the installer is not kept."
}
