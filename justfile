# List available recipes
default:
    @just --list

# Run all checks, build, and tests
ci: lint build test

# Format and lint checks
lint:
    cargo fmt --all -- --check
    cargo sort --workspace --check
    cargo clippy --locked --verbose --tests --benches --workspace -- -D clippy::all -D warnings
    cargo clippy --locked --verbose --target wasm32-unknown-unknown -p dex_canister -- -D clippy::all -D warnings

# Build canister WASM (native; fast loop for development)
build:
    cargo build --locked --target wasm32-unknown-unknown --release --package dex_canister
    mkdir -p wasms
    cp target/wasm32-unknown-unknown/release/dex_canister.wasm wasms/dex_canister.wasm
    ic-wasm wasms/dex_canister.wasm -o wasms/dex_canister.wasm shrink
    ic-wasm wasms/dex_canister.wasm -o wasms/dex_canister.wasm metadata candid:service -f canister/dex.did -v public
    ic-wasm wasms/dex_canister.wasm -o wasms/dex_canister.wasm metadata candid:args -d '(DexArg)' -v public
    gzip -fckn9 wasms/dex_canister.wasm > wasms/dex_canister.wasm.gz
    rm wasms/dex_canister.wasm

# Build canister WASM reproducibly via Docker (bit-identical across hosts).
# Platform is pinned to linux/amd64 by the Dockerfile's `FROM --platform=...`
# directive, so no `--platform` flag is needed here — handy for setups
# (e.g., `brew install docker` without the docker-buildx plugin) where the
# CLI doesn't accept that flag.
docker-build:
    docker buildx build --target export --output type=local,dest=./wasms .

# Run all tests
test: unit-tests integration-tests

# Run unit tests
unit-tests:
    cargo test --locked --workspace --exclude dex_int_tests

# Download external WASMs for integration tests
download-external-wasms:
    bash scripts/download-external-wasms.sh

# Run integration tests
integration-tests: download-external-wasms _maybe-build
    cargo test --locked --package dex_int_tests -- --test-threads 2 --nocapture

_maybe-build:
    {{ if env("DEX_CANISTER_WASM_PATH", "") == "" { "just build" } else { "true" } }}

# Run canbench benchmarks
bench:
    cd canister && canbench

# Run canbench and persist results for regression checks
bench-check:
    cd canister && canbench --persist

# Deploy to staging. Optionally pass a path to a file holding the identity's
# unlock secret (HSM PIN or encrypted-PEM password) to run non-interactively.
deploy identity='hsm' password_file='':
    icp deploy dex --identity "{{identity}}" {{ if password_file == '' { '' } else { '--identity-password-file "' + password_file + '"' } }} --environment staging