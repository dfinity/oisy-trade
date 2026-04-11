# DEX

> Order book DEX on the [Internet Computer](https://internetcomputer.org/).

## Table of Contents

- [Architecture](#architecture-building_construction)
- [Deployment](#deployment-rocket)
- [Development](#development-hammer_and_wrench)

## Architecture :building_construction:

- **Single canister**: all order book state, matching, and settlement live in one canister.
- **Synchronous matching engine**: token transfers only happen at the deposit/withdrawal edges; the matching engine operates entirely on internal balances, with no async complexity.
- **Event-sourced state**: every state change is recorded in an append-only log in stable memory and replayed on upgrade, providing full auditability and simpler upgrades.

See the [design document](docs/design.md) for the full architecture.

## Deployment :rocket:

| Environment | Canister ID                    | Dashboard                                                                            |
|-------------|--------------------------------|--------------------------------------------------------------------------------------|
| Staging     | `proc5-daaaa-aaaar-qb5va-cai` | [View](https://dashboard.internetcomputer.org/canister/proc5-daaaa-aaaar-qb5va-cai) |

## Development :hammer_and_wrench:

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [just](https://github.com/casey/just)
- [icp CLI](https://cli.internetcomputer.org/)

### Build

List all available recipes with `just`.

| Command      | Description                      |
|--------------|----------------------------------|
| `just lint`  | Run linter                       |
| `just build` | Build the canister WASM          |
| `just ci`    | Run all checks, build, and tests |

### Deploy to staging

Requires the [icp CLI](https://cli.internetcomputer.org/) with an identity that has deployment permissions.
By default the `hsm` identity is used; override with `just deploy <identity>`.

```bash
just deploy
```
