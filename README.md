# DEX

[![CI](https://github.com/dfinity/dex/actions/workflows/ci.yml/badge.svg)](https://github.com/dfinity/dex/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Internet Computer](https://img.shields.io/badge/Internet%20Computer-mainnet-blueviolet.svg)](https://internetcomputer.org/)

> Order book DEX on the [Internet Computer](https://internetcomputer.org/).

## Table of Contents

- [Key Features](#key-features)
- [Architecture](#architecture)
- [Deployment](#deployment)
- [Usage](#usage)
- [Development](#development)

<a id="key-features"></a>
## ✨ Key Features

- **CEX-like experience** — deposit once, trade as much as you want, withdraw anytime
- **Fully onchain order book** — central limit order book (CLOB) running entirely within a single canister
- **Permissionless trading** — any principal can trade on any active pair, no allowlisting required

<a id="architecture"></a>
## 🏗️ Architecture

- **Single canister**: all order book state, matching, and settlement live in one canister.
- **Synchronous matching engine**: token transfers only happen at the deposit/withdrawal edges; the matching engine operates entirely on internal balances, with no async complexity.
- **Event-sourced state**: every state change is recorded in an append-only log in stable memory and replayed on upgrade, providing full auditability and simpler upgrades.

See the [design document](docs/design.md) for the full architecture.

<a id="deployment"></a>
## 🚀 Deployment

| Environment         | Canister ID                                                                                                  | Listings          |
|---------------------|--------------------------------------------------------------------------------------------------------------|-------------------|
| 🧪 Staging | [`proc5-daaaa-aaaar-qb5va-cai`](https://dashboard.internetcomputer.org/canister/proc5-daaaa-aaaar-qb5va-cai) | Trade test tokens |

<a id="usage"></a>
## 📘 Usage

Walk through the main DEX flows against the staging canister using only the [`icp` CLI](https://cli.internetcomputer.org/): discover trading pairs, approve the DEX as an ICRC-2 spender, deposit, place a limit order, check its status, and withdraw.

See [`examples/getting_started.md`](examples/getting_started.md).


### 🤖Agents

Talk to your agent to interact with the DEX 😎.

> Buy 0.01 SOL at a limit price of 0.037 ETH per SOL

![Agent placing an order](examples/agent_place_order.png?raw=true "Agent placing an order")

See [`examples/agents.md`](examples/agents.md).

<a id="development"></a>
## 🛠️ Development

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
| `just test`  | Run unit and integration tests   |
| `just ci`    | Run all checks, build, and tests |

### Deploy to staging

Requires the [icp CLI](https://cli.internetcomputer.org/) with an identity that has deployment permissions.
By default the `hsm` identity is used; override with `just deploy <identity>`.

```bash
just deploy
```
