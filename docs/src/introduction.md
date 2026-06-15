# Introduction

> **OISY TRADE** is an order-book DEX on the [Internet Computer](https://internetcomputer.org/).

## Key features

- **CEX-like experience** — deposit once, trade as much as you want, withdraw anytime.
- **Fully onchain order book** — central limit order book (CLOB) running entirely within a single canister.
- **Permissionless trading** — any principal can trade on any active pair, no allowlisting required.

## Architecture at a glance

- **Single canister.** All order-book state, matching, and settlement live in one canister.
- **Synchronous matching engine.** Token transfers only happen at the deposit and withdrawal edges; the matching engine operates entirely on internal balances, with no async complexity.
- **Event-sourced state.** Every state change is recorded in an append-only log in stable memory and replayed on upgrade, providing full auditability and simpler upgrades.

For the full design, see [Development → Architecture → Design](development/architecture/design.md).

## Deployment

| Environment   | Canister ID                                                                                                  | Listings          |
|---------------|--------------------------------------------------------------------------------------------------------------|-------------------|
| Production    | [`sy2xe-miaaa-aaaar-qb7sq-cai`](https://dashboard.internetcomputer.org/canister/sy2xe-miaaa-aaaar-qb7sq-cai) | *Coming soon!*    |
| Staging       | [`proc5-daaaa-aaaar-qb5va-cai`](https://dashboard.internetcomputer.org/canister/proc5-daaaa-aaaar-qb5va-cai) | Trade test tokens |

## Where to next?

- New to OISY TRADE? Start with [For Users](usage/for-users.md).
- Want your agent to place orders? See [For Agents](usage/for-agents.md).
- Operating the canister? See [For Admins](usage/for-admins.md).
- Contributing or reviewing? See [Architecture](development/architecture/index.md) and [Specs](development/specs/index.md).
