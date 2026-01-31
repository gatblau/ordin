#!/usr/bin/env bash
# make gate-record: run on the computer that runs the tests. Runs the full gate (`make check`) on the
# clean commit checked out here and, only when it passes, records that in a git note on the commit
# (refs/notes/ordin-gate). The release build reads that note instead of running the tests itself
# (PB-REL-21). It pushes nothing: the release computer fetches the note from this repository, or the
# maintainer pushes refs/notes/ordin-gate to the remote the release build reads.
# spec: ordin-desktop/phase-5-playbook#PB-REL-22
set -euo pipefail
. "$(dirname "$0")/lib.sh"
need_tool git
need_tool make

require_clean_tree "Nothing was recorded."
HEAD_BEFORE="$(git -C "$ROOT" rev-parse HEAD)"
note "running the gate (make check) on $HEAD_BEFORE"
make -C "$ROOT" check || die "the gate failed on $HEAD_BEFORE. Nothing was recorded."

# The record says what was tested: the same commit, its tracked files unchanged.
[ "$(git -C "$ROOT" rev-parse HEAD)" = "$HEAD_BEFORE" ] \
  || die "the checked-out commit changed while the gate ran. Nothing was recorded."
git -C "$ROOT" diff --quiet HEAD -- \
  || die "the gate changed tracked files, so it did not test $HEAD_BEFORE as committed. Nothing was recorded."

RUST="$(cd "$ROOT" && rustc --version 2>/dev/null || echo 'rustc unknown')"
RECORD="${GATE_PASSED_PREFIX}$(date -u +%Y-%m-%dT%H:%M:%SZ) $(uname -s) $(uname -m), $RUST"
# A notes commit needs an author; a computer with no git identity gets a generic one.
IDENTITY=()
if [ -z "$(git -C "$ROOT" config user.email 2>/dev/null || true)" ]; then
  IDENTITY=(-c user.name=ordin-gate -c user.email=ordin-gate@localhost)
fi
git -C "$ROOT" "${IDENTITY[@]+"${IDENTITY[@]}"}" notes --ref="$GATE_NOTES_REF" add -f -m "$RECORD" "$HEAD_BEFORE" \
  || die "could not write the note on $HEAD_BEFORE."
note "recorded on $HEAD_BEFORE: $RECORD"
