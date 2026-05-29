# syntax=docker/dockerfile:1.7

# Reproducible build of the DEX canister WASM.
#
# Invoke via `just docker-build`, or directly:
#   docker buildx build --platform linux/amd64 \
#       --target export --output type=local,dest=./wasms .
#
# The output is `wasms/dex_canister.wasm.gz`, byte-identical regardless of
# host platform (Apple Silicon runs the linux/amd64 image via emulation).
#
# Post-build `ic-wasm` embeds the Candid interface and init-arg type into
# the WASM as custom metadata sections, then shrinks the binary.
#
# The base image ships rustc/cargo 1.93.0 + gcc + curl/xz + ca-certificates,
# matching rust-toolchain.toml exactly. Bump the Rust version in BOTH this
# Dockerfile and rust-toolchain.toml together.

FROM --platform=linux/amd64 rust:1.93.0-bookworm@sha256:d0a4aa3ca2e1088ac0c81690914a0d810f2eee188197034edf366ed010a2b382 AS builder

# Locale-independent ordering for any string-handling build tool.
ENV LC_ALL=C
# Defensive: tools that embed build timestamps see a fixed epoch.
ENV SOURCE_DATE_EPOCH=1
# Strip the container-internal source path from panic-message strings baked
# into the WASM. `/src` is already canonical inside the container, but this
# also covers the case of running outside Docker.
ENV RUSTFLAGS="--remap-path-prefix=/src=/"

# The base ships the host (x86_64-unknown-linux-gnu) toolchain; the wasm32
# target is the one extra component we need.
RUN rustup target add wasm32-unknown-unknown

# Install ic-wasm: pinned binary + SHA-256 verification. Used post-build for
# `shrink` and to embed `candid:service` / `candid:args` metadata sections.
# Bump IC_WASM_VERSION + IC_WASM_SHA256 together when upgrading.
ARG IC_WASM_VERSION=0.9.10
ARG IC_WASM_SHA256=54f7a100273b2cfbb993b4de1358523c453936d6f80b0cb340ec35e6fd0b5703
RUN curl --proto '=https' --tlsv1.2 -fsSL \
        "https://github.com/dfinity/ic-wasm/releases/download/${IC_WASM_VERSION}/ic-wasm-x86_64-unknown-linux-gnu.tar.xz" \
        -o /tmp/ic-wasm.tar.xz \
 && echo "${IC_WASM_SHA256}  /tmp/ic-wasm.tar.xz" | sha256sum -c - \
 && tar -xJf /tmp/ic-wasm.tar.xz -C /tmp \
 && mv /tmp/ic-wasm-x86_64-unknown-linux-gnu/ic-wasm /usr/local/bin/ic-wasm \
 && rm -rf /tmp/ic-wasm.tar.xz /tmp/ic-wasm-x86_64-unknown-linux-gnu

WORKDIR /src
COPY . .

# Build the canister, embed Candid metadata, shrink, and gzip. Each ic-wasm
# step writes back to the same file; this matches the conventional pattern
# used across the dfinity canister ecosystem.
RUN cargo build --locked --target wasm32-unknown-unknown --release --package dex_canister \
 && mkdir -p /out \
 && cp target/wasm32-unknown-unknown/release/dex_canister.wasm /out/dex_canister.wasm \
 && ic-wasm /out/dex_canister.wasm -o /out/dex_canister.wasm shrink \
 && ic-wasm /out/dex_canister.wasm -o /out/dex_canister.wasm metadata candid:service -f canister/dex.did -v public \
 && ic-wasm /out/dex_canister.wasm -o /out/dex_canister.wasm metadata candid:args -d '(DexArg)' -v public \
 && gzip -fckn9 /out/dex_canister.wasm > /out/dex_canister.wasm.gz \
 && rm /out/dex_canister.wasm

# Export-only stage. With `--target export --output type=local,dest=./wasms`,
# buildx drops just the gzipped WASM into the host's wasms/ directory.
FROM scratch AS export
COPY --from=builder /out/dex_canister.wasm.gz /
