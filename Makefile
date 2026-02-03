# ordin — build and check targets.
#
# `make check` is the full gate and the pre-commit hook runs it, so the tests run on the developer's
# computer before every commit. The CI job runs `make ci`, the same list without the tests (they need
# a container runtime for PostgreSQL). Both stop at the first failure.
# Entries are added by the step that creates the files they need (Playbook Step 0, item 8):
#   Step 3  — the Service API generator check (`ordin-service-api-gen -- check apps/desktop/ui`)
#   Step 71 — `npx svelte-check` and `npx vitest run` in apps/desktop/ui

SCREENS := apps/desktop/ui

# The PostgreSQL tests reach the container runtime through DOCKER_HOST. When it is not set, take the
# socket of the active docker context (Colima's, on the maintainer's computer), so a fresh shell
# runs the gate with no setup. A value already in the environment wins.
DOCKER_HOST ?= $(shell docker context inspect --format '{{.Endpoints.docker.Host}}' 2>/dev/null)
export DOCKER_HOST

# On Windows, the cmake crate replaces CMake's Release compiler flags with its own and drops every
# optimisation flag, so whisper.cpp (the Transcription Service's speech engine) was built unoptimised
# and ran about six times slower (plan 07 FU-020). whisper-rs-sys hands any CMAKE_ variable to CMake,
# and the cmake crate keeps one already set. A whisper build made before this line keeps its old flags:
# remove target/*/build/whisper-rs-sys-* once. Dashes, not slashes: Git Bash rewrites /MD as a path.
ifeq ($(OS),Windows_NT)
WHISPER_MSVC_FLAGS := -MD -O2 -Ob2 -DNDEBUG
export CMAKE_C_FLAGS_RELEASE := $(WHISPER_MSVC_FLAGS)
export CMAKE_CXX_FLAGS_RELEASE := $(WHISPER_MSVC_FLAGS)
endif

.DEFAULT_GOAL := check

.PHONY: check
## Run the whole gate: everything CI runs, then the tests.
check:
	$(MAKE) ci
	$(MAKE) test
	$(MAKE) first-start-check

.PHONY: ci
## Run every check except the tests, in order, stopping at the first failure. CI runs this.
ci:
	cargo deny check licenses advisories sources
	cd $(SCREENS) && npx license-checker-rseidelsohn --production --excludePrivatePackages --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC;0BSD;Apache-2.0 OR MIT;(MPL-2.0 OR Apache-2.0)"
	cd $(SCREENS) && npm audit --omit=dev
	cargo fmt --check
	$(MAKE) doc-ref-check
	$(MAKE) release-test
	$(MAKE) desktop-build-inputs
	cargo clippy --workspace -- -D warnings
	# The same with the minutes switch on: whisper.cpp needs CMake and Clang (plan 07 Phase 9, FU-007).
	cargo clippy --workspace --features ordin-desktop/minutes -- -D warnings
	cargo run -p ordin-service-api --bin ordin-service-api-gen -- check $(SCREENS)
	# The minutes build writes the same three files plus minutes.gen.ts and minutes.schema.gen.json (plan 07 FU-015).
	cargo run -p ordin-service-api --features minutes --bin ordin-service-api-gen -- check $(SCREENS)
	cd $(SCREENS) && npx svelte-check
	$(MAKE) ui-contract-check

.PHONY: ui-contract-check
## Run the screens' tests, recording every request they send, then check each request against the
## backend's own input rules (plan 04, safety net A). A request the backend would refuse fails the gate
## unless tests/support/refused-on-purpose.txt names it with a reason.
UI_REQUESTS := $(CURDIR)/target/ui-requests
ui-contract-check:
	rm -rf $(UI_REQUESTS)
	cd $(SCREENS) && ORDIN_UI_REQUESTS_DIR=$(UI_REQUESTS) npx vitest run
	# With the minutes rows: the screens send them to a backend that answers the group (plan 07 FU-015).
	cargo run -q -p ordin-service-api --features minutes --bin ordin-service-api-check-requests -- $(UI_REQUESTS) $(SCREENS)/tests/support/refused-on-purpose.txt

.PHONY: first-start-check
## Start the real core on an empty data root with the agents the release bundles
## (RELEASE_BUNDLED_AGENTS_SOURCE at RELEASE_BUNDLED_AGENTS_COMMIT, release/release.conf): both bundled
## agents must be ready and nothing may be logged at WARN or above outside
## apps/desktop/tests/fixtures/first-start-expected.txt (plan 04, safety net C).
first-start-check:
	cargo test -p ordin-desktop --test first_start -- --ignored --nocapture

.PHONY: journeys-windows
## Build COMMIT (default HEAD) on the Windows build machine with the test-only journeys switch and run
## the journeys against the real window (apps/desktop/journeys, plan 04 Phase 6). From the computer that
## runs the tests; the report lands in target/journeys/<commit>/. See scripts/journeys-windows.sh.
journeys-windows:
	scripts/journeys-windows.sh

.PHONY: local-build
## An unsigned installer of the working tree for this computer, to try ordin by hand: a .app and a .dmg
## on macOS, an NSIS installer on Windows, in target/release/bundle/. No gate, no signing, no update
## files; never published. AGENTS=commit (default) | worktree | none; JOURNEYS=1 adds the test-only
## journeys switch; MINUTES=1 adds meeting minutes. See scripts/release/local-build.sh.
local-build:
	scripts/release/local-build.sh

.PHONY: local-build-windows-remote
## An unsigned Windows installer of this working tree, built on the Windows build machine and brought back
## into target/local-build/windows/, to try ordin by hand. No gate, no journeys, no key; never published.
## See scripts/release/local-build-windows-remote.sh.
# spec: ordin-desktop/phase-5-playbook#PB-REL-30
local-build-windows-remote:
	scripts/release/local-build-windows-remote.sh

.PHONY: desktop-build-inputs
## On macOS and Windows, build what the desktop program's Tauri build step reads: the screens
## (`apps/desktop/ui/dist`) and the ordin hook beside the program
## (`apps/desktop/binaries/ordin-hook-<target triple>`, `bundle.externalBin`). Elsewhere the program
## is not built and nothing is needed.
HOST_TRIPLE := $(shell rustc -vV 2>/dev/null | sed -n 's/^host: //p')
EXE_SUFFIX := $(if $(findstring windows,$(HOST_TRIPLE)),.exe,)
desktop-build-inputs:
ifneq ($(filter %apple-darwin %windows-msvc %windows-gnu,$(HOST_TRIPLE)),)
	cd $(SCREENS) && npm run build
	cargo build --release -p ordin-hook
	mkdir -p apps/desktop/binaries
	cp target/release/ordin-hook$(EXE_SUFFIX) apps/desktop/binaries/ordin-hook-$(HOST_TRIPLE)$(EXE_SUFFIX)
endif

# spec: ordin-desktop/phase-5-playbook#PB-REL-19
.PHONY: desktop-build-inputs-windows
## The same inputs for the Windows installer built on the Mac with cargo-xwin (Playbook PB-REL-19).
## Called by release-build-windows, which sets XWIN_ACCEPT_LICENSE only after the maintainer accepted the licence.
desktop-build-inputs-windows:
	cd $(SCREENS) && npm run build
	cargo xwin build --release -p ordin-hook --target x86_64-pc-windows-msvc
	mkdir -p apps/desktop/binaries
	cp target/x86_64-pc-windows-msvc/release/ordin-hook.exe apps/desktop/binaries/ordin-hook-x86_64-pc-windows-msvc.exe

.PHONY: doc-ref-check
## Check that every spec anchor in source and every source path in the specs and designs resolves.
doc-ref-check:
	scripts/check-doc-refs.sh

.PHONY: doc-date-check
## Check that no document in docs/ or README.md is dated later than the commit that holds it, in every commit.
doc-date-check:
	scripts/check-doc-dates.sh

.PHONY: test
## Run the workspace tests. Local only: CI does not run them.
test:
	cargo test --workspace
	# The crates the minutes switch changes, built with it on (plan 07 Phase 9).
	cargo test -p ordin-desktop -p ordin-host -p ordin-service-api -p ordin-dictation -p ordin-minutes -p ordin-obs --features ordin-desktop/minutes

.PHONY: harness-test
## Run the harness test job against the real Claude Code at the pinned version. Not in `check`:
## Claude Code updates itself, and that must not break every commit.
## The Copilot half needs the Copilot CLI at the pinned version on PATH, signed in with a
## Copilot licence (plan 08 Phase 8).
harness-test:
	cargo test -p ordin-claude-adapter --test harness -- --ignored
	cargo test -p ordin-copilot-adapter --test harness -- --ignored

.PHONY: e2e-test
## Run the R1 scripted end-to-end job (plan 01 Phase 13): the desktop core driven through the Service API with the
## real Claude Code at the pinned version (model stood in locally) and memory's models in ORDIN_TEST_MODEL_DIR.
## Not in `check`, for the same reasons as harness-test and memory-model-test; it fails if either is missing.
e2e-test:
	ORDIN_TEST_MODEL_DIR="$(ORDIN_TEST_MODEL_DIR)" cargo test -p ordin-desktop --test end_to_end -- --ignored --nocapture

.PHONY: memory-model-test
## Run the memory service against the real int8 models. Not in `check`: the models are not in the
## repository and cost time to load. Download them once to ORDIN_TEST_MODEL_DIR (`embedding/` and
## `reranker/`, each an int8 `model.onnx` with its tokenizer files). The test fails if they are absent.
## A release build: the recall limit is about what ships, and a debug build's hashing alone takes 8 s.
ORDIN_TEST_MODEL_DIR ?= $(HOME)/.cache/ordin-models
memory-model-test:
	ORDIN_TEST_MODEL_DIR="$(ORDIN_TEST_MODEL_DIR)" cargo test --release -p ordin-memory-service --test model_job -- --ignored --nocapture

# The release pipeline (Playbook 6.5, plan 01 Phase 12). The maintainer runs these on their own
# computer; no CI job builds, signs or publishes. Each takes VERSION=<version>. release-build and
# release-publish need TAURI_SIGNING_PRIVATE_KEY (a file path) and TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# (see release/README.md).
.PHONY: release publish release-check release-build release-build-windows release-build-windows-remote release-publish release-test memory-models gate-record
## Asks once for the update key's password, checks it, then release-build and release-publish. PUBLISH=0 stops after the build.
release:
	scripts/release/release.sh

## Publish what release-build made: asks for the update key's password, checks it, then release-publish.
## Builds nothing. Needs VERSION=<version>; RELEASE_CHANNEL=test is the default.
publish:
	BUILD=0 scripts/release/release.sh

## Step 1: the gate. Builds nothing, so it needs no key. By default it checks the record that `make check`
## passed on this commit (gate-record); RELEASE_GATE=local runs `make check` here instead.
release-check:
	scripts/release/check.sh

## Steps 1 and 2 for this computer's platform: signed installers in target/release-out/<VERSION>.
release-build:
	scripts/release/build.sh

## An option, on the Mac only: the Windows installer built here with cargo-xwin, into the same folder.
## Needs RELEASE_ACCEPT_MSVC_LICENSE=1 on the command line (release/README.md, "Windows installer from the Mac").
# spec: ordin-desktop/phase-5-playbook#PB-REL-15
release-build-windows:
	RELEASE_CROSS=windows-x86_64 scripts/release/build.sh

## The Windows installer built and signed on the Windows build machine, started from this computer: the key goes
## there for the build only and is removed after; the files come back here and are checked. Needs VERSION=<version>.
# spec: ordin-desktop/phase-5-playbook#PB-REL-29
release-build-windows-remote:
	scripts/release/build-windows-remote.sh

## Steps 5 to 7 over the folder both computers filled. The maintainer's action.
release-publish:
	scripts/release/publish.sh

## On the computer that runs the tests: run `make check` on the clean commit checked out and, only when it
## passes, record that in a git note (refs/notes/ordin-gate) the release build reads (Playbook PB-REL-22).
# spec: ordin-desktop/phase-5-playbook#PB-REL-22
gate-record:
	scripts/release/gate-record.sh

## Test the release scripts against stand-ins for the signer, minisign and make.
release-test:
	scripts/release/test.sh

## Download the memory models, check them, add their licences and zip them: target/memory-models/ordin-memory-models-<VERSION>.zip.
memory-models:
	scripts/release/memory-models.sh
