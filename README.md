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

Install [`mise`](https://mise.jdx.dev/) and then run `mise install` from the repository root to set up Rust, `just`, and other pinned tools defined in [`mise.toml`](mise.toml).

Additionally:

- [icp CLI](https://cli.internetcomputer.org/)
- [Docker](https://docs.docker.com/get-docker/) with `buildx` (only for `just docker-build`)

### Build

List all available recipes with `just`.

| Command             | Description                                |
|---------------------|--------------------------------------------|
| `just lint`         | Run linter                                 |
| `just build`        | Build the canister WASM (native)           |
| `just docker-build` | Build the canister WASM reproducibly       |
| `just test`         | Run unit and integration tests             |
| `just ci`           | Run all checks, build, and tests           |

### Reproducible build

`just docker-build` produces `wasms/dex_canister.wasm.gz` that is byte-identical regardless of host platform, as long as Docker is available. The build runs inside a `linux/amd64` container with a digest-pinned base image and a pinned Rust toolchain; on Apple Silicon it transparently runs under Rosetta/QEMU.

CI verifies reproducibility by building the same commit twice on different runner images (`ubuntu-22.04` and `ubuntu-24.04`) and asserting the SHA-256 hashes match.

To verify a tagged release against the canister deployed on the IC, check out the tag and run:

```bash
just docker-build
sha256sum wasms/dex_canister.wasm.gz
```

The resulting hash should match both the SHA-256 published in the GitHub Release notes and the canister's module hash on the IC.

### Deploy to staging

Requires the [icp CLI](https://cli.internetcomputer.org/) with an identity that has deployment permissions.
By default the `hsm` identity is used; override with `just deploy <identity>`. Pass an optional second argument to read the identity's unlock secret (HSM PIN or encrypted-PEM password) from a file and run non-interactively:

```bash
just deploy                                   # default: hsm, interactive unlock
just deploy dev                               # non-hsm identity, interactive unlock
just deploy hsm ~/.config/icp/hsm.pin         # non-interactive with PIN file
```

### Releasing

Releases are cut from `main` with [release-plz](https://release-plz.dev/) in two manual steps:

1. **Open the release PR.** Run the [`Release`](.github/workflows/release.yml) workflow (Actions → Release → *Run workflow*). It opens a PR that bumps the crate versions and updates the changelogs from the [conventional commits](https://www.conventionalcommits.org/) made since the last release.
2. **Merge the release PR.** Review and merge it. Merging triggers the [`Publish`](.github/workflows/publish.yml) workflow, which:
   - tags the released crates,
   - builds `dex_canister.wasm.gz` reproducibly from the tagged commit and publishes a GitHub Release with the WASM, its SHA-256, the candid interface, and the deployment status, and
   - publishes `dex_types` to [crates.io](https://crates.io/).

The pipeline does not deploy — releasing and deploying are separate steps.
