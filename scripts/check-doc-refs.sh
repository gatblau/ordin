#!/usr/bin/env bash
# Check that spec anchors in source and source paths in the specs and designs resolve.
#
#   // spec: ordin-desktop/components/HookRelay#SA-HOOK-02
#
# resolves against the `docs/spec` of the project holding the file: `memory/` and `sandbox/` keep
# their own spec trees, everything else uses the root one. The id must appear literally in that file.
# A path quoted in backticks in `docs/design` or `docs/spec` must exist, read from the root of the
# project holding the document. Only paths starting at a top-level folder of that project are read as
# paths: `text/plain` and `owner/repo` are not. Line and symbol suffixes are ignored.
#
# A design document (`docs/design`, or a sibling project's) may cite a path git ignores: the HLD's
# history names private working notes (plans, RFCs) that a clone does not carry. A spec may not, as a
# spec stands alone; an ignored path cited in `docs/spec` stays DANGLING.
#
# `scripts/testdata/doc-refs-allow.txt` lists paths deliberately ahead of the build: one
# `<path or folder/> | <reason and removal condition>` per line.
#
# Usage: scripts/check-doc-refs.sh [repository root]. Exit 0 when clean, 1 on any DANGLING line.
# bash 3.2 compatible: macOS ships it.

set -u

root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$root" || exit 2
allow="scripts/testdata/doc-refs-allow.txt"
this="scripts/check-doc-refs.sh"

project_of() {
  case "$1" in
    memory/*) echo memory ;;
    sandbox/*) echo sandbox ;;
    *) echo . ;;
  esac
}

allowed() {
  [ -f "$allow" ] || return 1
  while IFS='|' read -r entry _; do
    entry="$(echo "$entry" | sed -E 's/[[:space:]]+$//')"
    case "$entry" in ''|'#'*) continue ;; esac
    case "$1" in
      "$entry"|"$entry"*) case "$entry" in */|"$1") return 0 ;; esac ;;
    esac
  done <"$allow"
  return 1
}

failures=0
anchors=0
paths=0
dangling() {
  echo "DANGLING $1"
  failures=$((failures + 1))
}

files="$(mktemp)"
trap 'rm -f "$files"' EXIT
git ls-files -co --exclude-standard >"$files"

# ---- anchors in source

while IFS= read -r line; do
  file="${line%%:*}"
  rest="${line#*:}"
  number="${rest%%:*}"
  ref="$(echo "${rest#*:}" | sed -E 's/^.*(\/\/|--|#) spec: *//')"
  anchors=$((anchors + 1))
  case "$ref" in
    *#*[[:space:]]*|*,*) dangling "$file:$number: not one anchor alone: $ref"; continue ;;
    *#*) ;;
    *) dangling "$file:$number: no assertion id: $ref"; continue ;;
  esac
  spec="$(project_of "$file")/docs/spec/${ref%%#*}.md"
  spec="${spec#./}"
  id="${ref#*#}"
  if [ ! -f "$spec" ]; then
    dangling "$file:$number: no spec file $spec"
  elif ! grep -qF -- "$id" "$spec"; then
    dangling "$file:$number: $id is not in $spec"
  fi
done < <(grep -vE '^(docs/|memory/docs/|sandbox/docs/|testdata/|\.claude/)|\.(md|lock)$' "$files" |
  grep -vxF "$this" | while IFS= read -r f; do [ -f "$f" ] && printf '%s\0' "$f"; done |
  xargs -0 grep -HnIE '^[[:space:]]*(//|--|#) spec: |[^:/"'"'"'](//|--) spec: [^ ]+#' 2>/dev/null)

# ---- paths in documents

while IFS= read -r doc; do
  [ -f "$doc" ] || continue
  project="$(project_of "$doc")"
  case "$doc" in */design/*) design=yes ;; *) design=no ;; esac
  while IFS= read -r quoted; do
    path="$(echo "$quoted" | sed -E 's/(:[0-9].*|::.*)$//; s#/$##')"
    case "$path" in /*|./*|*'<'*|*'*'*|*'{'*|*'$'*|*'…'*|*...*) continue ;; esac
    case "$path" in
      ../*)
        paths=$((paths + 1))
        [ -e "$(dirname "$doc")/$path" ] || allowed "$path" || dangling "$doc: no such path $path"
        continue
        ;;
    esac
    top="${path%%/*}"
    [ -e "$project/$top" ] || continue
    paths=$((paths + 1))
    [ -e "$project/$path" ] && continue
    if [ "$project" != . ]; then
      [ -e "$path" ] && continue
      allowed "$project/$path" && continue
    fi
    allowed "$path" && continue
    if [ "$design" = yes ]; then
      git check-ignore -q -- "$project/$path" 2>/dev/null && continue
      [ "$project" != . ] && git check-ignore -q -- "$path" 2>/dev/null && continue
    fi
    dangling "$doc: no such path $path"
  done < <(grep -oE '`[^` ]+/[^` ]+`' "$doc" | tr -d '`' | sort -u)
done < <(grep -E '^((memory|sandbox)/)?docs/(design|spec)/.*\.md$' "$files")

echo "doc-ref-check: $anchors anchors, $paths paths, $failures dangling"
[ "$failures" -eq 0 ]
