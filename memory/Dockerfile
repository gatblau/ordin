# syntax=docker/dockerfile:1
#
# Multi-stage image carrying BOTH memory binaries (Phase 1, plan 03).
#
# Builder stage compiles the crate in release mode, producing `target/release/memory` and
# `target/release/memory-mcp`. Runtime stage is a slim Debian image carrying only the two
# binaries, the CA bundle (TLS for OIDC discovery / JWKS fetch), a non-root user, and a
# writable store directory. The default command is `recall`; deployments override it to
# `memory-mcp` for the MCP edge.

# ---- Builder ----------------------------------------------------------------
FROM rust:1-bookworm AS builder

WORKDIR /usr/src/memory

# Dependency-cache layer: copy the manifests first and build a throwaway crate so the
# (slow) dependency compile is cached independently of the source. surrealdb + aws-lc-rs
# pull in C/asm toolchains already present in the rust:1-bookworm base.
#
# The manifest declares a `[[test]] name = "bdd"` target whose file (`tests/bdd.rs`) is
# excluded from the build context (see .dockerignore), so cargo would refuse to parse the
# manifest without it. We create an empty placeholder so the manifest is valid; building
# only the two `--bin` targets never compiles the test target.

# Cap codegen parallelism (plan 07 FU-001 / plan 06 FU-007). The image build was OOM-killed on an
# 8 GiB Docker VM while compiling `surrealdb-core` in release — not because any single rustc needs
# that much, but because the default job count is the host's CPU count (6 here) and each concurrent
# release-mode rustc on that crate holds well over a gigabyte. Two jobs trades build time for a peak
# that fits, and it fits on a small CI runner for the same reason. Raise it only alongside the VM.
ENV CARGO_BUILD_JOBS=2

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src/mcp tests \
    && echo 'fn main() {}' > src/main.rs \
    && echo 'fn main() {}' > src/mcp/main.rs \
    && echo '' > src/lib.rs \
    && echo '' > tests/bdd.rs \
    && cargo build --release --locked --bin memory --bin memory-mcp 2>/dev/null; rm -rf src

# Real source. Touch the entry points so cargo recompiles them over the cached deps; keep
# the placeholder `tests/bdd.rs` in place so the manifest still parses.
COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs src/mcp/main.rs src/lib.rs \
    && cargo build --release --locked --bin memory --bin memory-mcp

# ---- Runtime ----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# TLS trust roots for OIDC discovery and the JWKS fetch performed at boot.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root runtime user owning the writable store directory.
RUN groupadd --system --gid 10001 memory \
    && useradd --system --uid 10001 --gid memory --home-dir /var/lib/memory --shell /usr/sbin/nologin memory \
    && mkdir -p /var/lib/memory \
    && chown -R memory:memory /var/lib/memory

COPY --from=builder /usr/src/memory/target/release/memory /usr/local/bin/memory
COPY --from=builder /usr/src/memory/target/release/memory-mcp /usr/local/bin/memory-mcp

ENV MEMORY_STORE_PATH=/var/lib/memory \
    MEMORY_HTTP_ADDR=0.0.0.0:8080 \
    MEMORY_MCP_HTTP_ADDR=0.0.0.0:8081

EXPOSE 8080 8081

USER memory

# Default to the REST binary; deployments override the command to `memory-mcp`.
CMD ["memory"]
