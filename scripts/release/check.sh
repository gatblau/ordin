#!/usr/bin/env bash
# make release-check VERSION=<version>: Playbook 6.5 step 1. Builds nothing, so it needs no key. By default
# it checks the record that the gate passed on this commit (PB-REL-21); RELEASE_GATE=local runs make check.
# spec: ordin-desktop/phase-5-playbook#PB-REL-3
set -euo pipefail
. "$(dirname "$0")/lib.sh"
load_conf
require_version
require_package_version
if bundled_inputs_set; then note "the bundled agents commit is present"; fi
# spec: ordin-desktop/phase-5-playbook#PB-REL-12
require_models_input
note "the memory models input is complete"
note "step 1: the gate for $VERSION"
# spec: ordin-desktop/phase-5-playbook#PB-REL-21
release_gate
note "release-check passed for $VERSION"
