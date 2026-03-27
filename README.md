# DEX

Order book DEX on the Internet Computer.

## Architecture

See the [design document](docs/design.md) for the high-level architecture.

## Development :test_tube:

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)

### Build

#### Lint

```bash
cargo fmt --all -- --check
cargo clippy --locked --verbose --tests --benches --workspace -- -D clippy::all
cargo clippy --locked --verbose --target wasm32-unknown-unknown -p dex_canister -- -D clippy::all
```

#### Build the canister WASM

```bash
cargo build --locked --target wasm32-unknown-unknown --release --package dex_canister
gzip -fckn9 target/wasm32-unknown-unknown/release/dex_canister.wasm >./wasms/dex_canister.wasm.gz
```

#### Unit tests

```bash
cargo test --locked --workspace --exclude dex_int_tests
```

#### Integration tests

Requires the canister WASM built above and a running PocketIC server.

```bash
export DEX_CANISTER_WASM_PATH="$(pwd)/wasms/dex_canister.wasm.gz"
cargo test --package dex_int_tests -- --test-threads 2 --nocapture
```
