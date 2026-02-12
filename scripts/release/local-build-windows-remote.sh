#!/usr/bin/env bash
# make local-build-windows-remote: an unsigned Windows installer of the working tree, built on the Windows
# build machine and started from this computer, to try ordin by hand. The Windows counterpart of
# `make local-build`: no gate, no journeys, no signing key, no update files; never published (PB-REL-30).
#
# The working tree is built as it is, uncommitted changes included: its tracked files as `git stash create`
# records them, or HEAD when nothing differs. That commit is pushed to the git repository the Windows
# machine fetches from, under a branch of its own (`local-build`), and built there with `make local-build`.
# The installer comes back into target/local-build/windows/ here.
#
#   RELEASE_WINDOWS_SSH     how to reach the Windows machine; default JOURNEYS_WINDOWS_SSH, else
#                           "ssh -p 2222 ordin@127.0.0.1".
#   RELEASE_WINDOWS_REPO    the build checkout there, in Git Bash's form; default /c/o.
#   RELEASE_WINDOWS_REMOTE  the git remote of that checkout to fetch from; default "server".
#   RELEASE_WINDOWS_PUSH    this repository's name for the same repository, to push to; default "testbox".
#   AGENTS, as for make local-build (commit, worktree or none); commit is the default.
# spec: ordin-desktop/phase-5-playbook#PB-REL-30
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf

read -r -a WIN <<< "${RELEASE_WINDOWS_SSH:-${JOURNEYS_WINDOWS_SSH:-ssh -p 2222 ordin@127.0.0.1}}"
REPO="${RELEASE_WINDOWS_REPO:-/c/o}"
REMOTE="${RELEASE_WINDOWS_REMOTE:-server}"
PUSH="${RELEASE_WINDOWS_PUSH:-testbox}"
OUT="${LOCAL_BUILD_OUT_DIR:-$ROOT/target/local-build/windows}"

win_bash() {
  "${WIN[@]}" "& 'C:\\Program Files\\Git\\bin\\bash.exe' -lc '$(printf %s "$1" | sed "s/'/''/g")'"
}

# What is built: the working tree's tracked files as a commit that no branch holds, or HEAD itself.
COMMIT="$(git -C "$ROOT" stash create 2>/dev/null || true)"
[ -n "$COMMIT" ] || COMMIT="$(git -C "$ROOT" rev-parse --verify "HEAD^{commit}")"
UNTRACKED="$(git -C "$ROOT" ls-files --others --exclude-standard | head -n 3 || true)"
[ -z "$UNTRACKED" ] || note "files not yet added to git are not in this build: $(printf '%s' "$UNTRACKED" | tr '\n' ' ')(git add them first)"
# PB-REL-31: the GitHub App client id goes with the build, checked here so it cannot break the quoting.
GITHUB_APP_ID="${RELEASE_GITHUB_APP_CLIENT_ID:-}"
[[ "$GITHUB_APP_ID" =~ ^[A-Za-z0-9._-]{0,128}$ ]] || die "RELEASE_GITHUB_APP_CLIENT_ID may hold only letters, digits, '.', '_' and '-' (at most 128)"
# PB-REL-36: the Microsoft app client id, checked as the GitHub one.
MICROSOFT_APP_ID="${RELEASE_MICROSOFT_CLIENT_ID:-}"
[[ "$MICROSOFT_APP_ID" =~ ^[A-Za-z0-9._-]{0,128}$ ]] || die "RELEASE_MICROSOFT_CLIENT_ID may hold only letters, digits, '.', '_' and '-' (at most 128)"
case "${AGENTS:-commit}" in
  commit|worktree|none) ;;
  *) die "AGENTS is commit, worktree or none" ;;
esac

note "windows: sending $(git -C "$ROOT" rev-parse --short "$COMMIT") to the Windows machine's repository"
git -C "$ROOT" push -q -f "$PUSH" "$COMMIT:refs/heads/local-build" \
  || die "could not push the build to the git remote '$PUSH'. Nothing was built."

note "windows: building an unsigned installer on the Windows machine (no gate, no journeys, no key)"
win_bash "set -euo pipefail
taskkill //F //IM ordin.exe >/dev/null 2>&1 || true
cd '$REPO'
git fetch -q '$REMOTE' '+refs/heads/*:refs/remotes/$REMOTE/*'
git checkout -q -f '$COMMIT'
git clean -fdq -e node_modules -e target
(cd apps/desktop/ui && npm ci --no-audit --no-fund >/dev/null)
rm -rf target/release/bundle/nsis
AGENTS='${AGENTS:-commit}' RELEASE_GITHUB_APP_CLIENT_ID='$GITHUB_APP_ID' RELEASE_MICROSOFT_CLIENT_ID='$MICROSOFT_APP_ID' make local-build" \
  || die "the build on the Windows machine failed; see its output above."

note "windows: bringing the installer into $OUT"
rm -rf "$OUT" && mkdir -p "$OUT"
win_bash "cd '$REPO/target/release/bundle/nsis' && tar -c *.exe" | tar -x -C "$OUT" \
  || die "the installer did not come back from the Windows machine."
INSTALLERS=("$OUT"/*.exe)
[ -f "${INSTALLERS[0]}" ] || die "no installer came back from the Windows machine."
win_bash "taskkill //F //IM ordin.exe >/dev/null 2>&1 || true" || true
note "windows: unsigned installer of $(git -C "$ROOT" rev-parse --short "$COMMIT"): ${INSTALLERS[0]}"
note "it is for trying ordin by hand: it is not signed and no record says it passed anything; nothing here is published"
