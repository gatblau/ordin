# Releasing ordin

The maintainer builds, signs and publishes a version from their own computer. No CI job does it and no
CI service holds the update key. The rules are Playbook §6.5 and its January 2026 amendment
(`docs/spec/ordin-desktop/phase-5-playbook.md`, PB-REL-1 to PB-REL-23).

## Once: make the update key

1. `cargo install tauri-cli --locked`, `brew install minisign jq` (macOS), or the Windows equivalents. The scripts also use `curl`, `git`, `base64` and `shasum` or `sha256sum`.
2. `cargo tauri signer generate -w ~/ordin-update.key` — it asks for a password and writes
   `~/ordin-update.key` (private) and `~/ordin-update.key.pub`.
3. `base64 -d < ~/ordin-update.key.pub > release/ordin-update.pub` and commit **only** that file.
   Keep the private file and the password out of the repository (`*.key` is ignored).
4. Remove the `SPEC-GAP-33` comment in `apps/desktop/src/updater.rs`.

## Once: fill `release/release.conf`

`release/release.conf` already carries the feed address, the artefact address and the upload target for
both channels: GitHub releases at `gatblau/ordin-releases` (the `feed-test` and `feed` releases hold `latest.json`;
each version is the release `v<VERSION>`). Publishing needs your `gh` login to be able to write to that
repository. When the bundled agents repository exists, also write `RELEASE_BUNDLED_AGENTS_SOURCE` (a
local clone) and `RELEASE_BUNDLED_AGENTS_COMMIT` (the full hash).

## Once: the computer that runs the tests

The release build runs no test (PB-REL-21 to PB-REL-23). The full gate, `make check`, runs on a computer
of your choosing that can run it (a container runtime for the PostgreSQL tests, Rust, Node.js and `jq`).
There, `make gate-record` runs `make check` on the clean commit checked out and, only when it passes,
writes a git note on that commit in `refs/notes/ordin-gate`. It pushes nothing.

The release build then requires a clean working tree and that note on the commit it builds, fetched from
the git remote `RELEASE_GATE_REMOTE` (default `origin`). Either fetch it straight from the test computer:

```sh
git remote add testbox <user>@<test computer>:<path to its clone>   # once, in your clone
export RELEASE_GATE_REMOTE=testbox                                  # in your shell profile
```

or push the note from the test computer to `origin` (`git push origin refs/notes/ordin-gate`) and keep the
default. Which computer that is stays in your own git configuration and environment, never in the
repository. `RELEASE_GATE=local` runs `make check` on the release computer instead, as before.

## Each release

Before building, walk [`checklist.md`](checklist.md) by hand with the release candidate, on a Mac and on
a Windows PC, with your own Claude Code sign-in. It covers what the automatic checks cannot: what a
person sees, and the choices Ordin makes on the real model.

```sh
# on the test computer, at the commit you will release (the same hash):
git fetch origin && git checkout --detach <commit> && make gate-record
# on the release computer, at that commit:
export TAURI_SIGNING_PRIVATE_KEY=~/ordin-update.key        # a path
read -rs TAURI_SIGNING_PRIVATE_KEY_PASSWORD && export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
make release-check   VERSION=0.1.0-test.1                  # no key needed; checks the gate's record
make release-build   VERSION=0.1.0-test.1                  # on the Mac, then on the Windows computer
# gather both computers' target/release-out/0.1.0-test.1 into one folder; add notes.md (the /releasegen draft)
make release-publish VERSION=0.1.0-test.1                  # test channel; RELEASE_CHANNEL=real for a real one
```

`VERSION` must equal the `ordin-desktop` package version. A build stops, naming the commit, when the tree has
a change or the commit has no record that the gate passed; run `make gate-record` for that commit on the test
computer and try again. A first-time visitor can check a download with
`minisign -Vm <installer> -p release/ordin-update.pub` (the `.minisig` file sits beside the installer).

## Trying a build by hand, without the tests

`make local-build` on the Mac makes an unsigned `.app` and `.dmg` of the working tree (no gate, no key, nothing published).
`make local-build-windows-remote` does the same for Windows from the Mac (PB-REL-30): it sends the working tree to the
Windows machine, builds an unsigned installer there and brings it back into `target/local-build/windows/`. Use these to
smoke test and give early feedback; the release build and its records are only for what you publish. Set
`JOURNEYS_WINDOWS_SSH` as for the other Windows targets.

## Windows installer built on the Windows machine, started from the Mac

`make release-build-windows-remote VERSION=0.1.1` does the steps of building on the Windows computer from here (PB-REL-29):
it checks the key and password, sends the key to the Windows machine for the build only, builds the commit this
repository is at, removes the key there, and brings the signed installer back into `target/release-out/0.1.1`. Set
`JOURNEYS_WINDOWS_SSH` (or `RELEASE_WINDOWS_SSH`) to how the Mac reaches the Windows machine, for example
`"ssh -J andres@192.168.0.21 -p 2222 ordin@127.0.0.1"`, and push the commit to the repository that machine fetches from
(`RELEASE_WINDOWS_REMOTE`, default `server`) with its gate and journeys records, as `make release-check` needs. Then
`make publish VERSION=0.1.1`.

## Windows installer from the Mac

An option (Playbook §6.5, PB-REL-15 to PB-REL-20). The Windows computer stays the default: the real feed
refuses a Windows installer built on the Mac until one has been installed and checked on a clean Windows
computer. The test feed takes it.

It uses cargo-xwin, which downloads Microsoft's Windows SDK and C runtime under the Visual Studio Build Tools
licence (https://go.microsoft.com/fwlink/?LinkId=2086102). You accept that licence by writing
`RELEASE_ACCEPT_MSVC_LICENSE=1` on the command line each time; it is refused in `release/release.conf`.
Microsoft's files stay in `~/.cache/cargo-xwin` (or `RELEASE_XWIN_CACHE_DIR`, outside the repository) and are
never published: the leak check fails the build or the publish when one is in the release folder.

Once:

```sh
brew install llvm lld nsis node jq
rustup target add x86_64-pc-windows-msvc          # in the repository folder, so the pinned Rust gets it
cargo install --locked cargo-xwin
cargo install tauri-cli --version 2.12.4 --locked
```

Each release, after the macOS build and into the same folder:

```sh
export TAURI_SIGNING_PRIVATE_KEY=~/ordin-update.key
read -rs TAURI_SIGNING_PRIVATE_KEY_PASSWORD && export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
make release-build         VERSION=0.1.1-test.1                                # the macOS installers
make release-build-windows VERSION=0.1.1-test.1 RELEASE_ACCEPT_MSVC_LICENSE=1  # the Windows installer, on the Mac
make release-publish       VERSION=0.1.1-test.1                                # test channel
```

The first Windows build downloads Microsoft's files once and later builds reuse them. Each build checks the gate's record again; nothing is tested on the Mac. The
Windows build's log is `build-windows-x86_64.log`, beside the macOS `build.log`. The installer is signed
for the updater exactly as a Windows-built one is.

Every Windows installer, built on the Mac or on Windows, carries five files beside `ordin.exe` so the
program starts on a PC with nothing but Windows on it (PB-REL-24 to PB-REL-26): `DirectML.dll`, pinned
in `release/release.conf`, and the four Visual C++ runtime DLLs ONNX Runtime needs, taken from the newest
redistributable on the Visual Studio channel at each build. The build needs `curl` and `unzip`, keeps the
downloads in `~/.cache/ordin-release`, names the runtime's version in its log, and refuses an installer
whose programs would need a Visual C++ runtime DLL it does not carry.

## The short way: `make release`

```sh
make release
```

With no arguments it asks, one question at a time: the version (it offers the package's), what to do
(1 build only, 2 build and publish to the test feed, 3 build and publish to the real feed), whether to publish
macOS-only when there is no Windows build, whether to build without the bundled agents, one line of release
notes when publishing, and finally the update key's password (typed without echo). Press Enter to take the
offered answer. Choice 3 asks for the version to be typed again.

It signs a small file with the key and checks it against `release/ordin-update.pub`, so a wrong password or
the wrong key stops at once and not after the gate. The gate is the record of PB-REL-21, as for `make release-build`. The key is `~/ordin-update.key` unless
`TAURI_SIGNING_PRIVATE_KEY` names another file. It needs a terminal window to ask on.

To skip the questions: `make release VERSION=0.1.1-test.1 PUBLISH=0` (build only) or
`make release VERSION=0.1.1-test.1` (build, then publish); it then asks only for the password.
`RELEASE_CHANNEL=real`, `RELEASE_ALLOW_SINGLE_PLATFORM=1` (test feed only) and
`RELEASE_ALLOW_NO_BUNDLED_AGENTS=1` pass through.

To publish installers already built (for example the Mac and Windows builds collected in one folder):
`make publish VERSION=0.1.1-test.1`. It asks only for the password, checks the key the same way, then runs
`release-publish`; it builds nothing.

`make local-build` makes an unsigned installer of the working tree for the computer it runs on (a `.app`
and a `.dmg` on macOS, an NSIS installer on Windows), to try ordin by hand. It runs no gate, uses no
key and makes no update files, so nothing it makes is ever published. It is told where the memory
models are, as a release is, so Ordin offers their download the first time it remembers or recalls
something. Its version is the one in `release/release.conf`, so an installation that already holds
that version keeps the agents it read before: start it with a fresh data folder to see changed
agents. `AGENTS=worktree` bundles the
agents' working folder as it is, committed or not; `AGENTS=none` bundles none; `JOURNEYS=1` adds the
test-only journeys switch, which opens a debugging port and stays on the test machine.

