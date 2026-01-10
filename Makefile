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

.DEFAULT_GOAL := check

.PHONY: check
## Run the whole gate: everything CI runs, then the tests. This is what the pre-commit hook runs.
check: hooks-path-warning
	$(MAKE) ci
	$(MAKE) test

.PHONY: ci
## Run every check except the tests, in order, stopping at the first failure. CI runs this.
ci:
	cargo deny check licenses advisories sources
	cd $(SCREENS) && npx license-checker-rseidelsohn --production --excludePrivatePackages --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC;0BSD"
	cd $(SCREENS) && npm audit --omit=dev
	cargo fmt --check
	cargo clippy --workspace -- -D warnings
	cargo run -p ordin-service-api --bin ordin-service-api-gen -- check $(SCREENS)

.PHONY: test
## Run the workspace tests. Local only: CI does not run them.
test:
	cargo test --workspace

.PHONY: hooks
## Point git at the committed hooks, once on a new clone.
hooks:
	git config core.hooksPath .githooks

.PHONY: hooks-path-warning
hooks-path-warning:
	@if [ "$$(git config core.hooksPath 2>/dev/null)" != ".githooks" ]; then \
	  echo "warning: the pre-commit hook is not active; run 'make hooks' once on this clone" >&2; \
	fi
