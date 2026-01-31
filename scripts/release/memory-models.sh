#!/usr/bin/env bash
# make memory-models: download the memory models from the repositories that publish them, check every file
# against the fingerprints in release/memory-models/files.txt, add the licences and zip the result.
#
#   make memory-models VERSION=1       writes target/memory-models/ordin-memory-models-1.zip and SHA256SUMS
#
# Nothing is signed or uploaded here; publishing the zip is a separate, deliberate step. A file already in
# target/memory-models/cache with the right fingerprint is reused, so a rerun downloads nothing.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

VERSION="${VERSION:-}"
[[ "$VERSION" =~ ^[0-9]+$ ]] || die "give VERSION=<whole number>, the models release number (VERSION=1 makes models-1)"
need_tool curl
need_tool zip

DIR="$ROOT/release/memory-models"
OUT="$ROOT/target/memory-models"
CACHE="$OUT/cache"
STAGE="$OUT/stage"
ZIP="ordin-memory-models-$VERSION.zip"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

rm -rf "$STAGE" "$OUT/$ZIP" "$OUT/SHA256SUMS"
mkdir -p "$CACHE" "$STAGE"

while read -r want path url; do
  case "$want" in ''|'#'*) continue ;; esac
  cached="$CACHE/$want"
  if [ ! -f "$cached" ] || [ "$(sha256_of "$cached")" != "$want" ]; then
    note "downloading $path"
    curl --fail --silent --show-error --location --retry 3 --connect-timeout 20 --output "$cached.part" "$url" \
      || die "could not download $path from $url"
    got="$(sha256_of "$cached.part")"
    [ "$got" = "$want" ] || { rm -f "$cached.part"; die "$path: expected $want but the download is $got. Nothing was packed."; }
    mv "$cached.part" "$cached"
  fi
  mkdir -p "$STAGE/$(dirname "$path")"
  cp "$cached" "$STAGE/$path"
done < "$DIR/files.txt"

cp "$DIR/LICENSES.md" "$STAGE/LICENSES.md"
( cd "$STAGE" && find embedding reranker -type f | LC_ALL=C sort | while read -r f; do printf '%s  %s\n' "$(sha256_of "$f")" "$f"; done > SHA256SUMS )

# A fixed file date and order, so the same inputs always give the same zip and the same fingerprint.
find "$STAGE" -exec touch -t 202601010000 {} +
( cd "$STAGE" && { printf '%s\n' LICENSES.md SHA256SUMS; find embedding reranker -type f | LC_ALL=C sort; } | zip -q -X -@ "$OUT/$ZIP" )

printf '%s  %s\n' "$(sha256_of "$OUT/$ZIP")" "$ZIP" > "$OUT/SHA256SUMS"
note "wrote $OUT/$ZIP"
cat "$OUT/SHA256SUMS"
