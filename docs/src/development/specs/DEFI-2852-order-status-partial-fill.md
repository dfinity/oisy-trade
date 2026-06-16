---
id: DEFI-2852
title: Expand order records with partial-fill information
tags: [orders, order-status, partial-fill, query-api]
---

# Expand order records with partial-fill information

## Motivation

Today a caller cannot tell how much of a resting order has been filled. `OrderRecord`
carries the *original* `quantity` and a coarse `status` (`Pending` / `Open` / `Filled` /
`Canceled`); the only fill-derived datum anywhere is `CanceledOrderInfo.remaining_quantity`,
captured at cancel time. A maker order that is partially consumed but stays on the book
reports `Open` with no indication that any of it traded — you only learn the remaining
amount if you cancel, or see `Filled` once it completes. Every major spot venue exposes
filled-so-far as a first-class field (Binance `executedQty`, Kraken `vol_exec`, Coinbase
`filled_size`); we want the same.

We also have two overlapping read endpoints: `get_order_status(OrderId)` (un-scoped, any
principal, returns a bare `OrderStatus`) and `get_my_orders` (caller-scoped, paginated,
returns full `OrderRecord`s — which already embed `status`). The fill information belongs
on `OrderRecord`, which makes `get_order_status` redundant once `get_my_orders` can also
fetch a single order by id.

## Requirements

- **R1 — Filled amount is reported.** `get_my_orders` returns each order's cumulative
  filled amount in base-token units via `OrderRecord.filled_quantity`. Remaining is
  derivable as `quantity − filled_quantity`.
- **R2 — Partial fill is visible.** A resting order that has been partially filled reports
  `0 < filled_quantity < quantity` and `status == Open`.
- **R3 — Full fill.** A fully filled order reports `filled_quantity == quantity` and
  `status == Filled`.
- **R4 — Pending.** An order not yet matched reports `filled_quantity == 0`.
- **R5 — Cancel retains the fill.** A canceled order reports the `filled_quantity`
  accumulated before cancel (unchanged by the cancel); `status` is the unit variant
  `Canceled`. Remaining-at-cancel is `quantity − filled_quantity`.
- **R6 — Point lookup, owner-scoped.** `get_my_orders` with the `ById(id)` filter returns
  the single matching `UserOrder` when the order belongs to the caller, and an empty result
  otherwise (unknown id, or an order owned by another principal). The two filter modes are
  mutually exclusive by construction (`ById` carries no cursor; `ByPage` carries
  `after`/`length`), so no page parameter is interpreted in lookup mode.
- **R7 — `get_order_status` removed.** The `get_order_status` endpoint no longer exists in
  the canister interface; `OrderStatus` no longer has a `NotFound` variant. Absence from a
  `get_my_orders` result is the sole signal that an order does not exist / is not the
  caller's.
- **R8 — One stable-memory write per order per batch.** Recording a matching event writes
  each affected order's record at most once, folding its status transition, its accumulated
  fill delta, and its `last_updated_at` into a single read-modify-write.
- **R9 — Invariant and durability.** `filled_quantity` is monotonic non-decreasing and never
  exceeds `quantity`. This is enforced by an **always-on** check that traps on violation (a
  `BUG:` panic, per the canister's existing convention) — not a `debug_assert!`, since the
  canister ships in release where `debug_assert!` is compiled out — in addition to
  `checked_add` for overflow. `filled_quantity` is persisted in the stable-memory order
  history, survives canister upgrade, and the matching write path stays `Write`-gated so
  event-log replay does not double-count fills.
- **R10 — Order timestamps.** `OrderRecord` exposes `created_at` (renamed from `timestamp`,
  set once at placement) and `last_updated_at: Option<Timestamp>`. `last_updated_at` is
  `None` until the order is first modified, then carries the timestamp of the most recent
  modifying event (a fill, a status transition, or a cancel).

## Non-goals

- **Richer fill analytics.** Average fill price, quote-filled value, total fees, and fill
  count (the Coinbase/Kraken-style fields) are out of scope; only base `filled_quantity` is
  added. They can layer on later as further `OrderRecord` fields without disturbing this work.
- **A `PartiallyFilled` status variant.** Partialness is expressed by the
  `filled_quantity` field against `quantity`, not by splitting the resting state across
  `Open` / `PartiallyFilled` — see Design Decisions.
- **Cross-account / global order lookup.** Removing `get_order_status` makes lookup-by-id
  owner-scoped; querying an order you do not own is no longer possible (accepted).
- **A stored `remaining_quantity`.** Remaining is always derived (`quantity −
  filled_quantity`), never persisted.

## Design Decisions

- **Filled amount is a flat field on `OrderRecord`, not an `OrderStatus` variant.**
  `status` stays a pure lifecycle enum; how much has traded is orthogonal data that applies
  in several states (a resting order, a canceled order). This mirrors Kraken (`vol_exec`)
  and Coinbase (`filled_size`) and keeps the engine's existing `Open`/`Filled` transitions
  unchanged. (Why not a `PartiallyFilled` variant — see Discussed Alternatives.)

- **Persist `filled_quantity`; do not compute it by joining the live book at query time.**
  The system is pre-launch, so the breaking record-format change carries no migration cost,
  and persisting keeps the read path a pure history scan — `get_my_orders` never has to
  reach into order-book internals to reconstruct a number. (Why not the query-time join —
  see Discussed Alternatives.)

- **Consolidate point lookup into `get_my_orders`; remove `get_order_status`.** Fill
  information lives on `OrderRecord`, which `get_my_orders` already returns, so a single
  caller-scoped endpoint subsumes the bare status query. The argument becomes an optional
  `ById | ByPage` filter variant rather than a flat field set, so the two modes are mutually
  exclusive by construction (no ambiguous "id + cursor" combinations). Lookup-by-id becomes
  owner-scoped as a consequence (accepted; see Non-goals).

- **Drop `NotFound` and `CanceledOrderInfo`.** With lookup folded into `get_my_orders`,
  not-found is signalled by absence from the result vector, so `OrderStatus::NotFound`
  disappears. With `filled_quantity` persisted, remaining-at-cancel is derivable, so
  `CanceledOrderInfo` disappears and `Canceled` becomes a unit variant.

- **Aggregate fill deltas in the heap, then write each order once.** A single batch can
  fill one order across many `Fill`s (a taker sweeping several makers; a maker hit
  repeatedly), and an order can both change status and accrue fills in the same batch.
  Summing per-order deltas in plain memory first, then doing one read-modify-write per
  touched order, minimizes stable-memory roundtrips (R8).

## Implementation

### Constraints

The canister is event-sourced. Order records live in `OrderHistory`, backed by a
stable-memory `StableBTreeMap` (`canister/src/order/history`). `State::record_matching_event`
applies a matching event to the live `OrderBook` and, **only under
`StableMemoryOptions::Write`**, reflects the result into `OrderHistory`; post-upgrade replay
runs with `Skip`, since the stable map is preserved across upgrades. All new persistence
must respect that `Write` gate so replay does not re-apply it.

### Types — `libs/types` and `canister/oisy_trade.did`

- `OrderRecord` gains `filled_quantity: Nat` (cumulative base-token amount filled), renames
  `timestamp` to `created_at`, and gains `last_updated_at: Option<Timestamp>` (R10).
- `OrderStatus` drops `NotFound`; `Canceled` becomes a unit variant. Resulting set:
  `Pending`, `Open`, `Filled`, `Canceled`.
- `CanceledOrderInfo` is removed.
- `GetMyOrdersArgs` carries a single non-optional `filter` that is either a point lookup or
  a page, replacing the flat `after`/`length` pair. The endpoint takes `opt GetMyOrdersArgs`;
  an absent argument is the default (first page, newest first).
- `get_order_status` is removed from the canister interface.

The candid surface (the repo's candid equality check must pass):

```candid
// The endpoint arg is optional: `get_my_orders : (opt GetMyOrdersArgs) -> ...`.
// An absent argument defaults to the first page, newest first.
type GetMyOrdersArgs = record {
    filter : GetMyOrdersFilter;
};

type GetMyOrdersFilter = variant {
    ById  : OrderId;
    ByPage : record { after : opt OrderId; length : nat32 };
};

type OrderRecord = record {
    owner : principal;
    side : Side;
    price : nat;
    quantity : nat;
    filled_quantity : nat;
    status : OrderStatus;
    created_at : nat64;
    last_updated_at : opt nat64;
};

type OrderStatus = variant { Pending; Open; Filled; Canceled };
```

`length` is capped at `MAX_ORDERS_PER_RESPONSE` as today; an absent argument is equivalent
to `ByPage { after = null; length = MAX_ORDERS_PER_RESPONSE }`. A malformed `OrderId` (in
either `ById` or `ByPage.after`) is rejected exactly as the current id/cursor parsing does
(trap), so behavior is consistent across the endpoint.

### Internal order record — `canister/src/order`

- The internal `OrderRecord` (`order/history`) gains `filled_quantity: Quantity` and
  `last_updated_at: Option<Timestamp>` as new trailing minicbor fields (append-only indices;
  never reuse), and renames `timestamp` to `created_at`.
- The internal `OrderStatus` (`order/mod.rs`) `Canceled` variant becomes unit; the
  `CanceledOrderInfo` struct is removed.
- `OrderHistory` replaces the `set_status`-only writer with a single combined writer,
  `apply_update(&OrderId, OrderUpdate, now: Timestamp)`, where `OrderUpdate { status:
  Option<OrderStatus>, filled_delta: Quantity }`. It does one `get` + one `insert`, applying
  the status (if present), adding the delta via `checked_add`, and setting
  `last_updated_at = Some(now)`. It enforces `filled_quantity <= quantity` with an
  **always-on** check that traps on violation (a `BUG:` panic, matching the codebase's
  `expect("BUG: …")` convention) — *not* a `debug_assert!`, which is compiled out of the
  release canister and would let a corrupted value persist silently (R9).

### Matching write path — `canister/src/state` (`record_matching_event`)

Under the existing `Write` gate, replace the `compute_order_status_transitions` +
`set_status` loop with: build a heap `BTreeMap<OrderSeq, OrderUpdate>` from the batch
output — for each `fill` in `output.fills`, add `fill.quantity` to both
`fill.taker_order_seq` and `fill.maker_order_seq`; for each seq in `output.resting_orders`
set `status = Open`; for each in `output.filled_orders` set `status = Filled` — then call
`apply_update(.., now)` once per entry, where `now` is the matching `Event`'s timestamp.
`output.fills` already carries every maker/taker pair and per-fill quantity, so no
order-book/`MatchingOutput` changes are needed. This is the only site that catches a maker
partially filled while it stays open (which produces no status transition today). The event
timestamp is already available to `apply_state_transition` (`state/audit`) and is threaded
into `record_matching_event` (and `record_cancel_limit_order`) for `last_updated_at`.

### Cancel path — `canister/src/state` (`record_cancel_limit_order`)

Writes the unit `Canceled` status (with `last_updated_at` set to the cancel event's
timestamp). The cancel flow **still reads** `remaining_quantity` from the book removal
(`book.remove_order`) — it is required to compute the unreserve/refund amount, unchanged.
What goes away is only its *persistence* in history: with `CanceledOrderInfo` removed,
`remaining_quantity` is no longer stored on the record (remaining is derived from `quantity
− filled_quantity`).

### Endpoint — `canister/src/lib.rs`, `canister/src/main.rs`

- `get_my_orders`: an absent argument defaults to `GetMyOrdersArgs::default()`; then match
  `args.filter`. `ById(id)` → resolve the caller's `UserId` and return the single owned
  record as a one-element `vec` (empty if the id is unknown or owned by another principal).
  `ByPage { after, length }` → the existing newest-first cursor scan. The default filter is
  `ByPage { after = None, length = MAX_ORDERS_PER_RESPONSE }`.
- Remove `get_order_status` (business fn in `lib.rs`, the `#[ic_cdk::query]` wrapper in
  `main.rs`, and `state::get_order_status` if otherwise unused).

### Test plan

Unit (`*/tests.rs`, helpers/fixtures per repo convention):

- `order/history/tests.rs`: `apply_update` applies status-only, delta-only, and
  status+delta in a single write and sets `last_updated_at` to `now` (R8, R10); the
  `filled_quantity > quantity` invariant traps **in release build config** (an always-on
  check, not a compiled-out `debug_assert!`) (R9).
- `state/tests.rs`: a batch that partially fills a maker advances its `filled_quantity`
  without a status transition (R2); a fully filled order reaches `filled_quantity ==
  quantity` + `Filled` (R3); cancel-after-partial keeps `filled_quantity` and writes unit
  `Canceled` (R5); a fill spanning multiple `Fill`s for one order writes that order once
  (R8); `created_at` is unchanged after fills while `last_updated_at` advances (R10). Replay
  under `Skip` leaves `filled_quantity` untouched (R9).

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Place a maker, partially fill it with a crossing taker, then `get_my_orders` shows
  `0 < filled_quantity < quantity`, `Open` (R2); complete the fill → `filled_quantity ==
  quantity`, `Filled` (R3).
- `get_my_orders` with `ById(id)` returns the single owned order; returns empty for an
  unknown id and for an id owned by a different principal; `ByPage`/absent filter pages as
  before (R6).
- Cancel a partially filled order → `Canceled`, `filled_quantity` preserved (R5).
- Existing tests that called `get_order_status` are migrated to `get_my_orders` (R7).

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
# + the repo's candid equality check (see justfile / CI)
```

### Delivery / PR sequence

A single PR. The `filled_quantity` field and the write path that populates it are
inseparable — shipping the field without the write path would expose an always-zero value,
and the endpoint consolidation depends on the field existing. Acceptance: all of R1–R10.

(If review size warrants, this could split into "types + endpoint consolidation +
always-zero field" then "matching/cancel write path", but the field is vacuous until the
second lands, so one PR is preferred.)

## Discussed Alternatives

- **A `PartiallyFilled` OrderStatus variant (Binance style).** Rejected: it splits the
  single "resting on the book" concept across `Open` and `PartiallyFilled`, while the
  matching engine marks every rester `Open` — the distinction would have to be recomputed
  at the response boundary anyway. A flat field expresses partialness without touching the
  lifecycle enum and generalizes to the canceled case.

- **Compute filled at query time by joining the live order book.** The book's
  `resting_orders` index makes a resting order's remaining quantity reachable by id, so
  filled could be derived as `quantity − live_remaining` without persisting anything. This
  was the simpler option *when migration was a concern* — but the system is pre-launch, so
  there is no migration to avoid, and the join couples `get_my_orders` to order-book
  internals and only works while the order is still live. Persisting keeps reads a pure
  history scan and a single source of truth on the record.

- **Keep `get_order_status` for un-scoped lookup.** Rejected: nothing requires looking up an
  order you do not own, and folding the lookup into `get_my_orders` removes a redundant
  endpoint and the `NotFound` variant. Owner-scoped lookup is the accepted consequence.
