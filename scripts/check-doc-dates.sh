#!/usr/bin/env bash
# Check that no document is dated later than the commit that holds it.
#
# A date in `docs/` or `README.md` is read in two forms: `YYYY-MM-DD` and `<Month> YYYY`
# (`January 2026`). A commit made on date A must not contain a date B with B > A; a month-year form
# is compared by month. The commit's date is its author date.
#
# `scripts/testdata/doc-dates-allow.txt` lists dates that are not document dates, such as a
# certificate's expiry in a test scenario: one `<path> | <date as written> | <reason>` per line.
#
# Usage:
#   scripts/check-doc-dates.sh                    every commit reachable from HEAD, each against its own date
#   scripts/check-doc-dates.sh --as-of YYYY-MM-DD the working tree's committed files (HEAD) against that date
#   scripts/check-doc-dates.sh --tree YYYY-MM-DD  the tracked files on disk, for a commit about to be made on that date
# Exit 0 when clean, 1 on any LATE line. bash 3.2 compatible: macOS ships it.

set -u

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root" || exit 2
allow="scripts/testdata/doc-dates-allow.txt"
pattern='[0-9]{4}-[0-9]{2}-[0-9]{2}|(January|February|March|April|May|June|July|August|September|October|November|December) [0-9]{4}'
paths="docs README.md"
fail=0

# Read one stream of `<label>:<path>:<line>:<token>` lines, report each token later than $1.
report() {
  awk -F: -v limit="$1" -v allow="$allow" '
    BEGIN {
      split("January February March April May June July August September October November December", m, " ")
      for (i = 1; i <= 12; i++) mon[m[i]] = sprintf("%02d", i)
      while ((getline l < allow) > 0) {
        if (l ~ /^#/ || l == "") continue
        n = split(l, f, " \\| ")
        if (n >= 2) ok[f[1] SUBSEP f[2]] = 1
      }
    }
    {
      label = $1; path = $2; line = $3
      tok = $4; for (i = 5; i <= NF; i++) tok = tok ":" $i
      if (ok[path SUBSEP tok]) next
      if (tok ~ /^[0-9]/) { key = tok; cmp = substr(limit, 1, 10) }
      else { split(tok, p, " "); key = p[2] "-" mon[p[1]]; cmp = substr(limit, 1, 7) }
      if (key > cmp) { printf "LATE %s %s:%s dated %s, after %s\n", label, path, line, tok, cmp; bad = 1 }
    }
    END { exit bad }'
}

case "${1:-}" in
  --tree)
    [ -n "${2:-}" ] || { echo "usage: $0 --tree YYYY-MM-DD" >&2; exit 2; }
    # shellcheck disable=SC2086
    git ls-files -z $paths | xargs -0 grep -nIEo "$pattern" | sed 's/^\([^:]*\):\([0-9]*\):/disk:\1:\2:/' | report "$2" || fail=1
    ;;
  --as-of)
    [ -n "${2:-}" ] || { echo "usage: $0 --as-of YYYY-MM-DD" >&2; exit 2; }
    # shellcheck disable=SC2086
    git grep -nIEo "$pattern" HEAD -- $paths | report "$2" || fail=1
    ;;
  "")
    for c in $(git rev-list HEAD); do
      d="$(git log -1 --format=%ad --date=format:%Y-%m-%d "$c")"
      # shellcheck disable=SC2086
      git grep -nIEo "$pattern" "$c" -- $paths | sed "s/^$c:/$(echo "$c" | cut -c1-7):/" | report "$d" || fail=1
    done
    ;;
  *) echo "usage: $0 [--as-of DATE | --tree DATE]" >&2; exit 2 ;;
esac

if [ "$fail" -ne 0 ]; then echo "doc-date-check: FAILED"; exit 1; fi
echo "doc-date-check: clean"
