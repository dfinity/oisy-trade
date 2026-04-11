# DEX

> Order book DEX on the [Internet Computer](https://internetcomputer.org/).

## Table of Contents

- [Key Features](#key-features)
- [Architecture](#architecture)
- [Deployment](#deployment)
- [Development](#development)

<a id="key-features"></a>
## :sparkles: Key Features

- **CEX-like experience** — deposit once, trade as much as you want, withdraw anytime
- **Fully onchain order book** — central limit order book (CLOB) running entirely within a single canister
- **Permissionless trading** — any principal can trade on any active pair, no allowlisting required

<a id="architecture"></a>
## :building_construction: Architecture

- **Single canister**: all order book state, matching, and settlement live in one canister.
- **Synchronous matching engine**: token transfers only happen at the deposit/withdrawal edges; the matching engine operates entirely on internal balances, with no async complexity.
- **Event-sourced state**: every state change is recorded in an append-only log in stable memory and replayed on upgrade, providing full auditability and simpler upgrades.

See the [design document](docs/design.md) for the full architecture.

<a id="deployment"></a>
## :rocket: Deployment

| Environment         | Canister ID                                                                                                  | Listings          |
|---------------------|--------------------------------------------------------------------------------------------------------------|-------------------|
| :test_tube: Staging | [`proc5-daaaa-aaaar-qb5va-cai`](https://dashboard.internetcomputer.org/canister/proc5-daaaa-aaaar-qb5va-cai) | Trade test tokens |

<a id="development"></a>
## :hammer_and_wrench: Development

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
