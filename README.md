# DEX

Order book DEX on the Internet Computer.

## Architecture

See the [design document](docs/design.md) for the high-level architecture.

## Development :test_tube:

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [just](https://github.com/casey/just)
- [icp](https://cli.internetcomputer.org/)

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

### Deployment to staging :rocket:

Requires the [icp CLI](https://cli.internetcomputer.org/) with an identity that has deploy permissions.
By default the `hsm` identity is used; override with `just deploy <identity>`.

```bash
just deploy
```
