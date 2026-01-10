# memory — build targets.
#
# The container image is the shipping unit: one multi-stage image (see `Dockerfile`) carrying BOTH
# binaries, `recall` (REST edge) and `memory-mcp` (MCP edge), selected by the container command.

# Image coordinates. The default tag `memory:ci` is the one `docker-compose.yml` and the container
# smoke (`cargo test --test container`) reference, so a plain `make image` produces the image the
# local stack and the smoke expect. Override for a registry build:
#   make image IMAGE=ghcr.io/gatblau/memory TAG=v0.1.0
IMAGE ?= memory
TAG   ?= ci

# Extra flags passed through to `docker build`, e.g. `make image DOCKER_BUILD_ARGS=--no-cache`.
DOCKER_BUILD_ARGS ?=

.DEFAULT_GOAL := image

.PHONY: image
## Build the memory container image, tagged $(IMAGE):$(TAG).
image:
	docker build $(DOCKER_BUILD_ARGS) -t $(IMAGE):$(TAG) .
