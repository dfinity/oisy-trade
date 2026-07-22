---
id: DEFI-2942
title: Replay mainnet events and reconstruct state as a test
tags: [testing, back-compat]
---

# Replay mainnet events and reconstruct state as a test

## Motivation

The oisy-trade DEX persists an append-only event log and reconstructs state from it via `state::audit::replay_events`.
As new event types and fields are added, a decode/replay regression or a CBOR/Candid back-compatibility break could silently corrupt the reconstructed state on upgrade.
A companion test (DEFI-2942, `#227`) already loads the mainnet canister snapshot and upgrades it to the current wasm.
This spec adds the replay half: take the mainnet event log, replay it with the current code, and prove the reconstructed state matches the mainnet state captured in the snapshot.
This is analogous to the ckETH/ckBTC minter `should_replay_events_for_mainnet` tests (DEFI-2078).

## Requirements

R1: Given the publicly available mainnet snapshot, the test reconstructs a `State` by replaying the snapshot's event log through `state::audit::replay_events` and does not panic.
R2: The reconstructed persisted collections equal those stored in the snapshot: token balances, order history, per-user open orders, trades, trades-by-user, the user registry, and trading-account registries.
R3: The reconstructed in-heap order books match the live order-book depth the snapshot canister reports: for every trading pair, the depth derived from the reconstructed `State` equals `get_order_book_depth` from the snapshot loaded into PocketIC.
R4: The snapshot artifact is pinned by SHA-256 and rejected on mismatch (shared with `#227`).

## Non-goals

- No new production code path for event replay — `replay_events` already exists and is exercised only from tests.
- No comparison of transient/derived heap fields beyond order books (e.g. pending settling queues), which are not part of the persisted contract.
- No live-mainnet network dependency at test time beyond downloading the pinned snapshot artifact (same as `#227`).

## Design Decisions

- Reconstruct-vs-snapshot for the persisted collections is done by opening the snapshot's stable memory directly (a `MemoryManager` over `stable_memory.bin`) and comparing it, entry by entry, against fresh collections that `replay_events` writes into. This is the authoritative persisted state and needs no running canister.
- Order books are heap-only (the region-4 `StateSnapshot` is only rewritten at `pre_upgrade`, so it is stale on a live-capture snapshot). Their ground truth therefore comes from the snapshot loaded into PocketIC and queried via `get_order_book_depth`, cross-checked against the depth derived from the reconstructed `State::order_books()`.
- The internal `state::event::Event` (not the Candid `oisy_trade_types_internal::event::Event`) is read straight from the snapshot's stable event log, so `replay_events` consumes exactly what the canister persisted — no cross-type conversion.

## Implementation

### Constraints

- `state::audit::replay_events`, `State`, `storage`, and the collection types are `pub` in the `oisy_trade_canister` library target, so the test can depend on `oisy_trade_canister` (path dep) and drive replay directly.
- Stable-memory region IDs are the `MemoryId`s in `canister/src/storage/mod.rs`: event-log index `0`, event-log data `1`, order history `2`, balances `3`, state snapshot `4`, user registry `5`, user orders `6`, trades `7`, trades-by-user `8`, trading accounts `9`, trading-accounts-by-funding `10`.
- The snapshot download/extract/verify helpers currently live in `integration_tests/tests/mainnet_snapshot.rs`; hoist them (and the URL / SHA-256 / mainnet-id constants) into `integration_tests/src/lib.rs` and update `mainnet_snapshot.rs` to use them.

### `integration_tests`

- Add `oisy_trade_canister` and `ic-stable-structures` as dev-dependencies.
- New test file `integration_tests/tests/replay_mainnet.rs`:
  - Download + extract the pinned snapshot (shared helper); read `stable_memory.bin` into a `VectorMemory` and `MemoryManager::init` over it.
  - Open the snapshot's event log (`StableLog<state::event::Event, _, _>` over regions 0/1) and collect the events.
  - Call `replay_events` with fresh collections backed by a second `MemoryManager`, `StableMemoryOptions::Write`, to obtain the reconstructed `State`; re-open the fresh collections over their backing memory to read what replay wrote.
  - R2: assert each reconstructed collection equals the snapshot collection (compare by iterating sorted entries).
  - R3: load the snapshot into PocketIC (reuse the `#227` create-at-mainnet-id + `canister_snapshot_upload`/`load_canister_snapshot` flow), and for each trading pair assert `get_order_book_depth` equals the depth derived from the reconstructed `State`.

### canister

- Add minimal `pub` read accessors / iterators on the collection types only where needed to iterate entries for R2 (these are legitimate getters, not `#[cfg(test)]` helpers). Prefer reusing any existing `iter`/`PartialEq`.

### Delivery / PR sequence

Single PR (this one): `replay_mainnet.rs` plus the helper hoist and any minimal collection accessors. Covers R1–R4.

## Discussed Alternatives

- Compare the full reconstructed `State` against a `State` rebuilt from the snapshot (as the existing `assert_replay_matches` unit test does). Rejected: the snapshot's heap (order books, config) is not recoverable from stable memory alone, and the region-4 heap snapshot is stale on a live capture.
- Convert Candid `get_events` output into internal events for replay. Rejected: the two event models differ (e.g. `fee_rates` vs `maker_fee_bps`/`taker_fee_bps`); reading the internal event log from stable memory avoids a brittle conversion.
