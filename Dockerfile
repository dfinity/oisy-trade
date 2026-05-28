# syntax=docker/dockerfile:1.7

# Reproducible build of the DEX canister WASM.
#
# Invoke via `just docker-build`, or directly:
#   docker buildx build --platform linux/amd64 \
#       --target export --output type=local,dest=./wasms .
#
# The output is `wasms/dex_canister.wasm.gz`, byte-identical regardless of
# host platform (Apple Silicon runs the linux/amd64 image via emulation).

FROM --platform=linux/amd64 ubuntu:26.04@sha256:f3d28607ddd78734bb7f71f117f3c6706c666b8b76cbff7c9ff6e5718d46ff64 AS builder

# Locale-independent ordering for any string-handling build tool.
ENV LC_ALL=C
# Defensive: tools that embed build timestamps see a fixed epoch.
ENV SOURCE_DATE_EPOCH=1
# Strip the container-internal source path from panic-message strings baked
# into the WASM. `/src` is already canonical inside the container, but this
# also covers the case of running outside Docker.
ENV RUSTFLAGS="--remap-path-prefix=/src=/"

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        build-essential \
        pkg-config \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

# Install rustup with no default toolchain; cargo will auto-install the
# version pinned in rust-toolchain.toml (plus the wasm32-unknown-unknown
# target) on first invocation. Single source of truth.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --default-toolchain none --profile minimal

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
