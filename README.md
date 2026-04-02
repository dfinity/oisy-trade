# DEX

Order book DEX on the Internet Computer.

## Architecture

See the [design document](docs/design.md) for the high-level architecture.

## Development :test_tube:

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [just](https://github.com/casey/just)

### Build

List all available recipes with `just`.

#### Lint

```bash
just lint
```

#### Build the canister WASM

```bash
just build
```

#### Run all checks, build, and tests

```bash
just ci
```
