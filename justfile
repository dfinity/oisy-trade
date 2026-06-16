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
    cargo clippy --locked --verbose --target wasm32-unknown-unknown -p oisy_trade_canister -- -D clippy::all -D warnings

# Build canister WASM (native; fast loop for development)
build:
    cargo build --locked --target wasm32-unknown-unknown --release --package oisy_trade_canister
    mkdir -p wasms
    cp target/wasm32-unknown-unknown/release/oisy_trade_canister.wasm wasms/oisy_trade_canister.wasm
    ic-wasm wasms/oisy_trade_canister.wasm -o wasms/oisy_trade_canister.wasm shrink
    ic-wasm wasms/oisy_trade_canister.wasm -o wasms/oisy_trade_canister.wasm metadata candid:service -f canister/oisy_trade.did -v public
    ic-wasm wasms/oisy_trade_canister.wasm -o wasms/oisy_trade_canister.wasm metadata candid:args -d '(OisyTradeArg)' -v public
    gzip -fckn9 wasms/oisy_trade_canister.wasm > wasms/oisy_trade_canister.wasm.gz
    rm wasms/oisy_trade_canister.wasm

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
    cargo test --locked --workspace --exclude oisy_trade_int_tests

# Download external WASMs for integration tests
download-external-wasms:
    bash scripts/download-external-wasms.sh

# Run integration tests
integration-tests: download-external-wasms _maybe-build
    cargo test --locked --package oisy_trade_int_tests -- --test-threads 2 --nocapture

_maybe-build:
    {{ if env("OISY_TRADE_CANISTER_WASM_PATH", "") == "" { "just build" } else { "true" } }}

# Build the documentation book
book:
    mise exec -- mdbook build docs

# Serve the documentation book locally and open it in the browser
book-serve:
    mise exec -- mdbook serve docs --open

# Run canbench benchmarks
bench:
    cd canister && canbench

# Run canbench and persist results for regression checks
bench-check:
    cd canister && canbench --persist

# Deploy to staging. Optionally pass a path to a file holding the identity's
# unlock secret (HSM PIN or encrypted-PEM password) to run non-interactively.
deploy identity='hsm' password_file='':
    icp deploy oisy_trade --identity "{{identity}}" {{ if password_file == '' { '' } else { '--identity-password-file "' + password_file + '"' } }} --environment staging