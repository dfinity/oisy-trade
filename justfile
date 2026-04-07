# List available recipes
default:
    @just --list

# Run all checks, build, and tests
ci: lint build test

# Format and lint checks
lint:
    cargo fmt --all -- --check
    cargo clippy --locked --verbose --tests --benches --workspace -- -D clippy::all -D warnings
    cargo clippy --locked --verbose --target wasm32-unknown-unknown -p dex_canister -- -D clippy::all -D warnings

# Build canister WASM
build:
    cargo build --locked --target wasm32-unknown-unknown --release --package dex_canister
    mkdir -p wasms
    gzip -fckn9 target/wasm32-unknown-unknown/release/dex_canister.wasm > wasms/dex_canister.wasm.gz

# Run all tests
test: unit-tests integration-tests

# Run unit tests
unit-tests:
    cargo test --locked --workspace --exclude dex_int_tests

# Download external WASMs for integration tests
download-external-wasms:
    bash scripts/download-external-wasms.sh

# Run integration tests
integration-tests: download-external-wasms
    cargo test --package dex_int_tests -- --test-threads 2 --nocapture

# Deploy to staging
deploy identity='hsm':
    icp deploy dex --identity {{identity}} --environment staging