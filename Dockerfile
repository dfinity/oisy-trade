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
# The base image ships rustc/cargo 1.93.0 + gcc + ca-certificates, matching
# rust-toolchain.toml exactly. Bump the Rust version in BOTH this Dockerfile
# and rust-toolchain.toml together.

FROM --platform=linux/amd64 rust:1.93.0-slim-bookworm@sha256:776861219cd851131c1cec3bbd7cbeb16b99a794048097eb69ad9682a8ed0d57 AS builder

# Locale-independent ordering for any string-handling build tool.
ENV LC_ALL=C
# Defensive: tools that embed build timestamps see a fixed epoch.
ENV SOURCE_DATE_EPOCH=1
# Strip the container-internal source path from panic-message strings baked
# into the WASM. `/src` is already canonical inside the container, but this
# also covers the case of running outside Docker.
ENV RUSTFLAGS="--remap-path-prefix=/src=/"

# The slim image ships the host (x86_64-unknown-linux-gnu) toolchain; the
# wasm32 target is the one extra component we need.
RUN rustup target add wasm32-unknown-unknown

WORKDIR /src
COPY . .

# Build the canister and gzip the result. Flags mirror `just build`:
#   --locked          : refuse to update Cargo.lock
#   gzip -fckn9       : -n strips the timestamp; -9 max compression
RUN cargo build --locked --target wasm32-unknown-unknown --release --package dex_canister \
 && mkdir -p /out \
 && gzip -fckn9 target/wasm32-unknown-unknown/release/dex_canister.wasm > /out/dex_canister.wasm.gz

# Export-only stage. With `--target export --output type=local,dest=./wasms`,
# buildx drops just the gzipped WASM into the host's wasms/ directory.
FROM scratch AS export
COPY --from=builder /out/dex_canister.wasm.gz /
