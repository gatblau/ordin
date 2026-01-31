#!/usr/bin/env bash
# leak-check.sh <path>...: stops with status 1 when a file under the paths holds a line of the key
# file or the key's password (PB-REL-9), or is one of Microsoft's files from the cargo-xwin cache
# (PB-REL-18). Prints file names only, never the matched text.
# spec: ordin-desktop/phase-5-playbook#PB-REL-9
set -euo pipefail
[ "$#" -ge 1 ] || { echo "usage: leak-check.sh <path>..." >&2; exit 2; }
key="${TAURI_SIGNING_PRIVATE_KEY:-}"
[ -f "$key" ] || { echo "leak-check: TAURI_SIGNING_PRIVATE_KEY does not name a file" >&2; exit 2; }
found=0
patterns="$(mktemp)"
trap 'rm -f "$patterns"' EXIT
grep -v '^untrusted comment:' "$key" | grep -v '^[[:space:]]*$' > "$patterns" || true
if [ -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]; then printf '%s\n' "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" >> "$patterns"; fi
if [ -s "$patterns" ]; then
  hits="$(grep -rlaF -f "$patterns" -- "$@" 2>/dev/null || true)"
  if [ -n "$hits" ]; then
    found=1
    printf 'leak-check: the key or its password appears in:\n%s\n' "$hits" >&2
  fi
fi

# The files cargo-xwin downloads from Microsoft are never published (the condition of the licence,
# PB-REL-17). A file is refused when it has a kind the cache holds and a release never does, when it lies
# inside the cache, or when the cache holds a file of the same name.
# spec: ordin-desktop/phase-5-playbook#PB-REL-18
cache="${RELEASE_XWIN_CACHE_DIR:-${HOME:-/nonexistent}/.cache/cargo-xwin}"
cache_real=""
if [ -d "$cache" ]; then cache_real="$(cd "$cache" && pwd -P)"; fi
xhits=""
while IFS= read -r -d '' f; do
  name="$(basename "$f")"
  lower="$(printf '%s' "$name" | tr 'A-Z' 'a-z')"
  hit=0
  case "$lower" in *.lib|*.h|*.hpp|*.idl|*.winmd|*.cab|*.msi|*.vsix) hit=1 ;; esac
  if [ "$hit" = 0 ] && [ -n "$cache_real" ]; then
    case "$(cd "$(dirname "$f")" && pwd -P)/" in "$cache_real"/*) hit=1 ;; esac
    if [ "$hit" = 0 ] && [ -n "$(find "$cache_real" -type f -name "$name" -print 2>/dev/null | head -1)" ]; then hit=1; fi
  fi
  if [ "$hit" = 1 ]; then xhits="$xhits$f
"; fi
done < <(find "$@" -type f -print0 2>/dev/null)
if [ -n "$xhits" ]; then
  found=1
  printf 'leak-check: files from the cargo-xwin cache (Microsoft'"'"'s, never published) are in:\n%s' "$xhits" >&2
fi
exit "$found"
