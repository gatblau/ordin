#!/usr/bin/env bash
# make release-publish VERSION=<version>: Playbook 6.5 steps 5 to 7 over the folder the builds filled
# (PB-REL-8). The maintainer runs this; nothing else publishes.
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf
require_version
# spec: ordin-desktop/phase-5-playbook#PB-REL-3
require_key
# spec: ordin-desktop/phase-5-playbook#PB-REL-4
require_public_key
need_tool jq
need_tool curl
need_tool minisign "brew install minisign"

# spec: ordin-desktop/phase-5-playbook#PB-REL-5
channel
FEED_URL="$(require_chan_value FEED_URL)"
BASE="$(require_chan_value ARTEFACT_BASE_URL)"
BASE="${BASE%/}"
UPLOAD="$(require_chan_value UPLOAD)"
check_https "RELEASE_${CH}_FEED_URL" "$FEED_URL"
check_https "RELEASE_${CH}_ARTEFACT_BASE_URL" "$BASE"
case "$UPLOAD" in
  dir:?*) KIND=dir; DEST="${UPLOAD#dir:}" ;;
  github:?*:?*)
    KIND=github
    need_tool gh
    rest="${UPLOAD#github:}"
    REPO="${rest%%:*}"
    FEED_TAG="${rest#*:}"
    ;;
  *) die "RELEASE_${CH}_UPLOAD must be dir:<folder> or github:<owner>/<repo>:<feed-tag>" ;;
esac

OUT="${RELEASE_OUT_DIR:-$ROOT/target/release-out/$VERSION}"
[ -d "$OUT" ] || die "$OUT does not exist; run release-build on each computer first"
ls "$OUT"/platform-darwin-*.json >/dev/null 2>&1 || die "$OUT has no macOS build (platform-darwin-*.json)"
# spec: ordin-desktop/phase-5-playbook#PB-REL-8
if ! ls "$OUT"/platform-windows-*.json >/dev/null 2>&1; then
  if [ "$CHANNEL" = test ] && [ "${RELEASE_ALLOW_SINGLE_PLATFORM:-}" = 1 ]; then
    note "WARNING: $OUT has no Windows build; the test feed will carry only: $(cd "$OUT" && ls platform-*.json | sed 's/^platform-//;s/\.json$//' | tr '\n' ' ')"
  else
    die "$OUT has no Windows build (platform-windows-*.json); the real channel always needs both, and the test channel needs RELEASE_ALLOW_SINGLE_PLATFORM=1 to publish without it"
  fi
fi
# A Windows installer built on the Mac goes to the test feed only, until one has been checked on a clean
# Windows computer and this assertion is amended.
# spec: ordin-desktop/phase-5-playbook#PB-REL-20
if [ "$CHANNEL" = real ]; then
  for p in "$OUT"/platform-windows-*.json; do
    [ -f "$p" ] || continue
    if jq -e '.cross == true' "$p" >/dev/null 2>&1; then
      die "$(basename "$p") was built on the Mac; the real channel takes a Windows installer built on Windows until one built on the Mac has been checked on a clean Windows computer. Nothing was uploaded."
    fi
  done
elif ls "$OUT"/platform-windows-*.json >/dev/null 2>&1 && jq -e '.cross == true' "$OUT"/platform-windows-*.json >/dev/null 2>&1; then
  note "the Windows installer was built on the Mac (test channel)"
fi
HASHFILE="ordin-$VERSION-SHA256SUMS.txt"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# (1) Check what is about to go out.
note "checking the installers against their hashes and signatures"
for list in "$OUT"/SHA256SUMS.*; do
  while read -r hash name; do
    [ -f "$OUT/$name" ] || die "$name is listed in $(basename "$list") but missing"
    [ "$(sha256_of "$OUT/$name")" = "$hash" ] || die "$name does not match its hash; nothing was uploaded"
    minisign -Vm "$OUT/$name" -p "$PUBLIC_KEY_FILE" -x "$OUT/$name.minisig" >/dev/null \
      || die "$name does not verify against release/ordin-update.pub; nothing was uploaded"
  done < "$list"
done

# (2) The hash file for the whole version.
cat "$OUT"/SHA256SUMS.* | sort -k2 > "$OUT/$HASHFILE"

# spec: ordin-desktop/phase-5-playbook#PB-REL-9
# A feed file left by an earlier attempt is not checked: step (4) writes it again from public values only.
rm -f "$OUT/latest.json"
"$(dirname "$0")/leak-check.sh" "$OUT"

# (3) Notes: at most 4,000 characters, never cut.
NOTES="${RELEASE_NOTES_FILE:-$OUT/notes.md}"
[ -s "$NOTES" ] || die "no release notes at $NOTES (the /releasegen draft)"
chars="$(jq -Rs 'length' "$NOTES")"
[ "$chars" -le 4000 ] || die "the release notes are $chars characters; the limit is 4,000"

# (4) The feed, in Tauri's updater format.
jq -s --arg version "$VERSION" --rawfile notes "$NOTES" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg base "$BASE" '
  {version: $version, notes: ($notes | rtrimstr("\n")), pub_date: $date,
   platforms: (map({key: .platform, value: {signature: .signature, url: ($base + "/v" + $version + "/" + .file)}}) | from_entries)}' \
  "$OUT"/platform-*.json > "$OUT/latest.json"

ARTEFACTS=()
for f in "$OUT"/ordin-"$VERSION"-*; do ARTEFACTS+=("$f"); done

fetch() { curl -fsSL --max-time 300 "$1" -o "$2"; }

put_artefacts() {
  case "$KIND" in
    dir) mkdir -p "$DEST/v$VERSION" && cp "${ARTEFACTS[@]}" "$DEST/v$VERSION/" ;;
    # The tag is made in the releases repository, on its own default branch: the source commit is not
    # there, so it is named in the release's description instead (the feed's notes stay as written).
    github) { cat "$NOTES"; printf '\nBuilt from commit %s.\n' "$(git -C "$ROOT" rev-parse HEAD)"; } > "$TMP/release-notes.md"
            gh release create "v$VERSION" "${ARTEFACTS[@]}" --repo "$REPO" --title "ordin $VERSION" \
              --notes-file "$TMP/release-notes.md" ;;
  esac
}

HAD_PREVIOUS=0
keep_previous_feed() {
  mkdir -p "$TMP/previous"
  case "$KIND" in
    dir) if [ -f "$DEST/latest.json" ]; then cp "$DEST/latest.json" "$TMP/previous/latest.json"; HAD_PREVIOUS=1; fi ;;
    github) if gh release download "$FEED_TAG" --repo "$REPO" --pattern latest.json --dir "$TMP/previous" --clobber >/dev/null 2>&1; then HAD_PREVIOUS=1; fi ;;
  esac
}

put_feed() {
  case "$KIND" in
    dir) cp "$OUT/latest.json" "$DEST/latest.json.tmp" && mv "$DEST/latest.json.tmp" "$DEST/latest.json" ;;
    github)
      gh release view "$FEED_TAG" --repo "$REPO" >/dev/null 2>&1 \
        || gh release create "$FEED_TAG" --repo "$REPO" --title "ordin update feed" \
             --notes "The feed file the installed programs read."
      gh release upload "$FEED_TAG" "$OUT/latest.json" --repo "$REPO" --clobber ;;
  esac
}

restore_feed() {
  case "$KIND" in
    dir) if [ "$HAD_PREVIOUS" = 1 ]; then cp "$TMP/previous/latest.json" "$DEST/latest.json"; else rm -f "$DEST/latest.json"; fi ;;
    github)
      if [ "$HAD_PREVIOUS" = 1 ]; then gh release upload "$FEED_TAG" "$TMP/previous/latest.json" --repo "$REPO" --clobber
      else gh release delete-asset "$FEED_TAG" latest.json --repo "$REPO" --yes; fi ;;
  esac
}

# (7) Read it back from the published address. Every command is checked by hand: set -e is off here.
read_back() {
  local rb="$TMP/readback" hash name key url sig
  mkdir -p "$rb"
  fetch "$FEED_URL" "$rb/latest.json" || { note "the feed cannot be read at its address"; return 1; }
  [ "$(jq -r '.version' "$rb/latest.json")" = "$VERSION" ] || { note "the feed does not name version $VERSION"; return 1; }
  fetch "$BASE/v$VERSION/$HASHFILE" "$rb/$HASHFILE" || { note "the hash file cannot be downloaded"; return 1; }
  while read -r hash name; do
    fetch "$BASE/v$VERSION/$name" "$rb/$name" || { note "$name cannot be downloaded"; return 1; }
    [ "$(sha256_of "$rb/$name")" = "$hash" ] || { note "$name does not match the hash file"; return 1; }
    fetch "$BASE/v$VERSION/$name.minisig" "$rb/$name.minisig" || { note "$name.minisig cannot be downloaded"; return 1; }
    minisign -Vm "$rb/$name" -p "$PUBLIC_KEY_FILE" -x "$rb/$name.minisig" >/dev/null \
      || { note "$name does not verify against the public key"; return 1; }
  done < "$rb/$HASHFILE"
  while IFS="$(printf '\t')" read -r key url sig; do
    name="${url##*/}"
    [ -f "$rb/$name" ] || { note "the feed names $name, which the hash file does not list"; return 1; }
    printf '%s' "$sig" | base64 -d > "$rb/feed-$key.minisig" || { note "the feed's signature for $key is not base64"; return 1; }
    minisign -Vm "$rb/$name" -p "$PUBLIC_KEY_FILE" -x "$rb/feed-$key.minisig" >/dev/null \
      || { note "the feed's signature for $key does not verify"; return 1; }
  done < <(jq -r '.platforms | to_entries[] | [.key, .value.url, .value.signature] | @tsv' "$rb/latest.json")
  return 0
}

# spec: ordin-desktop/phase-5-playbook#PB-REL-8
note "publishing $VERSION to the $CHANNEL channel"
keep_previous_feed
put_artefacts
put_feed
if ! read_back; then
  restore_feed
  die "the read-back failed; the previous feed file was restored. The uploaded files stay; delete a test release and tag yourself."
fi
note "release-publish passed: $FEED_URL now offers $VERSION"
