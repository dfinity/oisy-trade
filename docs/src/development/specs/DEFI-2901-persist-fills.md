---
id: DEFI-2901
title: Persist individual fills and expose per-order / per-account trade feeds
tags: [orders, fills, trades, query-api, stable-memory]
---

# Persist individual fills and expose per-order / per-account trade feeds

## Motivation

There is no fills / user-trades endpoint. Today a caller can only *approximate* execution
from `get_my_orders`, which exposes the original limit `price` and the cumulative base
`filled_quantity` (added in DEFI-2852) — nothing about the price(s) actually traded. That
approximation is wrong whenever there is price improvement or level-sweeping:

- Fills execute at the **resting maker's** price, not the taker's limit (`OrderBook` records
  `maker_price` on every `Fill`).
- A single order can fill at **multiple different prices** — sweeping several levels in one
  match, or incremental fills over time.
- A buy taker that crosses below its limit is **refunded** the quote surplus at settlement
  (the `Unreserve` op in `compute_balance_operations`).

So the true execution price / VWAP is **not recoverable** from `get_my_orders`. The internal
`Fill` carries the maker price and quantity, but it is discarded after settlement; fees are
computed transiently during settlement and never persisted per fill; `get_events` is
debug-oriented and exposes neither per-fill prices nor fees.

DEFI-2852 deliberately deferred this: it added base `filled_quantity` and called out "average
fill price, quote-filled value, total fees, and fill count" as a later layer of `OrderRecord`
fields plus a dedicated feed. This spec is that layer. Every major spot venue exposes a
per-fill feed plus order-level cumulative figures as table stakes (see
[Cross-exchange comparison](#cross-exchange-comparison)).

This spec separates two distinct deliverables, which must not be conflated:

1. **Order-level scalars on `OrderRecord`** — cheap cumulative summaries (`filled_quote`,
   `filled_fee`) folded into the write that already updates `filled_quantity`.
2. **Per-fill records in their own stable-memory regions** — the granular feed, behind new
   `get_order_fills` and `get_my_trades` endpoints.

## Requirements

- **R1 — Cumulative quote on the order.** `OrderRecord` exposes `filled_quote`: the cumulative
  **realized** quote notional transacted, summed as `Σ (maker_price × fill_quantity)` over the
  order's fills. It is always **quote-denominated** and is the *realized* notional — for a buy
  taker that crossed below its limit, the refunded surplus is **excluded** (the maker price is
  used, not the taker limit). VWAP is derivable as `filled_quote / filled_quantity`.
- **R2 — Cumulative fee on the order.** `OrderRecord` exposes `filled_fee`: the cumulative
  **realized** fee charged to the order across its fills. It is denominated in the order's
  **receive token** — base for a buy, quote for a sell (the receive-side fee convention in
  `compute_balance_operations`). It is the sum of amounts actually charged, never reconstructed
  from a bps rate.
- **R3 — Per-fill persistence.** Every fill persists, side-projected, the information needed to
  audit an execution: the owning `order_id`, the execution `price` (the maker price), base
  `quantity`, quote `notional`, realized `fee`, `fee_token`, the `is_maker` role flag, the
  order's `side`, and a `timestamp`. The counterparty's identity is **not** stored or exposed.
- **R4 — Per-order fill feed, owner-scoped.** `get_order_fills(order_id, after, length)` returns
  the caller's fills for that order, newest-first, paginated by an `after` cursor. It returns an
  empty page when the order is unknown or is owned by another principal (same ownership rule as
  `get_my_orders { ById }`).
- **R5 — Account-wide trade feed, owner-scoped.** `get_my_trades(after, length)` returns the
  caller's fills across **all** their orders, newest-first, paginated. Each entry carries its
  `order_id` so a client can group by order.
- **R6 — Correct values under price improvement, sweeping, and refund.** For a fill, `price`
  equals the maker's execution price (never the taker's limit); `notional` equals
  `maker_price × quantity` (so a buy taker's refunded surplus is excluded); `fee` equals the
  amount actually withheld for that side; `is_maker` reflects that side's role on that fill. An
  order that fills partly as taker and partly as maker (crosses on entry, then rests and is hit)
  records each fill with its own role and rate — the role is **per fill**, not per order.
- **R7 — One order write per batch, extended.** Recording a matching event still writes each
  affected order's record **at most once** (DEFI-2852 R8). `filled_quote` and `filled_fee` are
  folded into that same read-modify-write alongside `filled_quantity`, via an extended
  `OrderUpdate`.
- **R8 — Write-gated, replay-safe, durable.** Fill persistence happens only under
  `StableMemoryOptions::Write`, so event-log replay at `post_upgrade` does not double-write
  fills. Fill records and the order-level scalars live in stable memory and survive upgrade. The
  fill sequence is canister-global and monotonic.
- **R9 — Monotonic invariants.** `filled_quote` and `filled_fee` are monotonic non-decreasing;
  each delta is applied with `checked_add` guarded by an **always-on** trap on overflow (a
  `BUG:` panic, matching the codebase convention — not a `debug_assert!`, which is compiled out
  of the release canister).
- **R10 — Bounded pages.** Both feeds cap `length` at `MAX_FILLS_PER_RESPONSE`; an absent /
  larger value is clamped. A malformed or unknown `after` cursor yields an empty page, exactly
  as `orders_after` handles its cursor.
- **R11 — Realized values computed once in settlement.** Per-fill `notional`, `fee`, and role
  are computed in the settlement path (`State`), where the fee schedule and base scale are in
  scope — **not** bolted onto the matching engine's `Fill` struct. The same computation feeds
  both the existing `BalanceOperation`s and the new fill records (single source of truth).
- **R12 — Measured hot-path cost.** The settlement instruction cost is measured with a
  canbench micro-benchmark, with vs. without fill persistence, to characterize the per-fill
  insert cost against the timer chunk's instruction budget. This is an acceptance gate on the
  persistence PR, not a hand-wave.

## Non-goals

- **A stored average / VWAP field.** VWAP is derived client-side as
  `filled_quote / filled_quantity`. An integer `Price` cannot represent a fractional average
  exactly; storing `filled_quote` (exact) and dividing on read avoids a lossy field. (Kraken /
  Coinbase expose a pre-divided average; we follow Binance — see
  [Cross-exchange comparison](#cross-exchange-comparison).)
- **Storing fee rates (bps) on records.** Only realized amounts are persisted. Rates are
  immutable per pair today but may change later; a stored amount stays correct regardless,
  whereas a stored rate (or a lazy bps re-derivation) would misreport historical fees the moment
  a rate changes. The current rates remain available via the pair's configuration, which answers
  a different question ("what will I pay next") than the fill feed ("what did I pay").
- **Exposing the counterparty.** Neither feed reveals the other side's principal, order id, or
  fee — consistent with every venue surveyed.
- **Cross-account / global trade lookup.** Both feeds are owner-scoped; there is no way to query
  another principal's fills.
- **A retention / pruning policy.** Fill storage grows unbounded (~120–150 B per side-projected
  record). Pagination bounds the *read*, not the *store*. A retention story (archival, TTL,
  caller-paid storage) is a follow-up; this spec only notes the growth and the
  `log`-it-if-capped discipline. The system is pre-launch, so there is no urgency yet.
- **Backfilling pre-existing orders.** Orders that filled before this ships have no persisted
  fills and report `filled_quote == 0` / `filled_fee == 0`. Pre-launch: start fresh, documented.
- **Normalized fill storage.** A single canonical record plus pointer entries was considered and
  rejected — see [Design Decisions](#design-decisions) and
  [Discussed Alternatives](#discussed-alternatives).

## Design Decisions

- **Two scalars on `OrderRecord`, the fill list in its own region — never embedded.** Embedding
  a growing `Vec<Fill>` in `OrderRecord` would re-serialize an ever-larger record on every fill
  (O(n²) write amplification through `apply_update`) and bloat the hot `get_my_orders` map.
  Instead: `filled_quote` and `filled_fee` are O(1) scalar adds folded into the existing
  per-order read-modify-write (≈ two extra `u128` adds on a write already paid for, R7), and the
  per-fill records live in dedicated stable regions read only by the new feeds.

- **Persist realized amounts, never rates (R1, R2).** A fill records the quote and fee that were
  *actually* transacted. This is robust to a future fee-schedule change and sidesteps any
  rate-versioning / timestamp-join needed to reconstruct historical fees. The `is_maker` flag is
  kept as a *descriptive* fact about the fill, not as an input to recompute the fee.

- **`filled_quote` is always quote; `filled_fee` is side-denominated.** A single order's side is
  fixed, so all its fees fall in one token (base for a buy, quote for a sell) and a scalar sum is
  coherent. The asymmetry vs. the always-quote `filled_quote` is real and is made explicit by the
  per-fill `fee_token` field, so a client never has to guess the denomination.

- **Denormalized, side-projected fill records (written twice), not a normalized canonical
  record.** A fill belongs to two orders (taker + maker); each owner must see their *own* price
  improvement, fee, role, and side, with the counterparty omitted. The two side views differ in
  most fields (`fee`, `is_maker`, `side`, `order_id`), so a normalized canonical record would
  have to carry both legs and be projected (and privacy-filtered) at read time. Denormalizing
  writes two self-contained records and makes `get_order_fills` a pure prefix range scan with no
  indirection. Chosen because the **settlement hot path** is the stated top risk: denormalized
  is fewer inserts than normalized (see Discussed Alternatives) and storage — the cost it trades
  for — has a separate, deferrable mitigation (retention). (Why not normalized — see Discussed
  Alternatives.)

- **Mirror `OrderHistory`'s storage shape.** The fill store reuses the exact pattern of
  `OrderHistory`: a primary `StableBTreeMap` keyed by an `OrderId`-prefixed key (so per-order
  reads are a range scan), plus a `(UserId, global_seq)` secondary index for the account-wide
  feed — identical to `by_user` / `orders_after`. New `MemoryManager` regions init fresh, so
  there is no upgrade-serialization cost (R8).

- **Realized values are computed in settlement, not in the matcher (R11).** The matcher's `Fill`
  stays fee-free; `compute_balance_operations` already derives `notional`, `quote_fee`,
  `base_fee`, and the maker/taker roles. That computation is the single source feeding both the
  balance ops and the fill records.

## Cross-exchange comparison

How the proposal lines up with the per-fill and order-level surfaces of the three reference
venues. The takeaway: the proposed feed matches the cross-venue baseline field-for-field; the
only deliberate divergence is deriving VWAP rather than storing it.

| Capability | Binance | Coinbase Advanced | Kraken | This spec |
|---|---|---|---|---|
| Per-fill feed | `myTrades` | List Fills | `TradesHistory` | `get_my_trades` |
| Filter fills by order | `orderId` param | `order_id` param | by `ordertxid` | `get_order_fills(order_id)` |
| Execution price / fill | `price` | `price` | `price` | `price` (maker price) |
| Base quantity / fill | `qty` | `size` | `vol` | `quantity` |
| Quote notional / fill | `quoteQty` | (price×size) | `cost` | `notional` |
| Realized fee / fill | `commission` | `commission` | `fee` | `fee` |
| Fee denomination | `commissionAsset` | quote | quote | `fee_token` (base/quote) |
| Maker/taker / fill | `isMaker` | `liquidity_indicator` | `maker` (in order) | `is_maker` |
| Side / fill | `isBuyer` | `side` | `type` | `side` |
| Counterparty exposed | no | no | no | no |
| Order cumulative base | `executedQty` | `filled_size` | `vol_exec` | `filled_quantity` (DEFI-2852) |
| Order cumulative quote | `cummulativeQuoteQty` | (derived) | `cost` | `filled_quote` |
| Order cumulative fee | (per-trade) | `total_fees` | `fee` | `filled_fee` |
| Order average price | derive | `average_filled_price` | `price` | **derive** (`filled_quote / filled_quantity`) |

Notes on the divergences:

- **VWAP is derived, not stored.** Coinbase and Kraken expose a pre-computed average fill price;
  they denominate prices as decimal strings, so a fractional average is representable. We use
  integer `Price`, where a fractional average is not exactly representable — so, like Binance, we
  expose the exact cumulative quote and let the client divide.
- **Fee denomination is explicit per fill.** Binance can charge commission in a *third* asset
  (BNB) and reports `commissionAsset`; Coinbase/Kraken charge in quote. Our receive-side
  convention charges the buyer in base and the seller in quote, so `fee_token` is carried on each
  record to make the denomination unambiguous rather than assumed.
- **No counterparty leakage**, matching all three venues.

## Implementation

### Constraints

The canister is event-sourced. `State::record_matching_event` applies a matching event to the
live `OrderBook` and, **only under `StableMemoryOptions::Write`**, reflects the result into
stable memory; post-upgrade replay runs with `Skip`. All new persistence must respect that
`Write` gate (R8). Order records live in `OrderHistory` (`canister/src/order/history`), backed by
two `StableBTreeMap`s — a primary `orders` map (`OrderId → SeqOrderRecord`) and a `by_user` index
(`(UserId, global_seq) → OrderId`); `orders_after` paginates the index as an O(length) reverse
range scan. The fill store mirrors this exactly. `MemoryId`s 0–6 are in use
(`canister/src/storage`); fills take the next free ids.

### Types — `libs/types` and `canister/oisy_trade.did`

`OrderRecord` gains two trailing fields (additive; extra fields in a returned record are a
backward-compatible Candid evolution, as `filled_quantity` was in DEFI-2852 — the repo's candid
equality / backward-compat check must pass):

```candid
type OrderRecord = record {
    owner : principal;
    side : Side;
    price : nat;
    quantity : nat;
    filled_quantity : nat;     // base, cumulative (DEFI-2852)
    filled_quote : nat;        // quote, cumulative realized notional (R1)
    filled_fee : nat;          // realized fee, in the order's receive token (R2)
    status : OrderStatus;
    created_at : nat64;
    last_updated_at : opt nat64;
};
```

A new per-fill record and the two feeds:

```candid
type PairToken = variant { Base; Quote };

// One side-projected fill. The counterparty is intentionally omitted.
type Trade = record {
    order_id : OrderId;     // the owning (caller's) order
    side : Side;            // this order's side
    price : nat;            // execution (maker) price
    quantity : nat;         // base filled
    notional : nat;         // quote transacted = price * quantity (realized)
    fee : nat;              // realized fee charged to this side
    fee_token : PairToken;  // base for a buy, quote for a sell
    is_maker : bool;        // this side's role on this fill
    timestamp : nat64;
};

// Owner-scoped, newest-first, paginated by `after` (a fill cursor). An absent
// `after` starts at the newest fill; `length` is capped at MAX_FILLS_PER_RESPONSE.
get_order_fills : (OrderId, opt FillCursor, nat32) -> (vec Trade) query;
get_my_trades   : (opt FillCursor, nat32) -> (vec Trade) query;
```

`FillCursor` is the opaque pagination cursor (a global fill sequence, encoded like `OrderId`);
the exact spelling is an implementation detail kept symmetric with the order cursor.
`MAX_FILLS_PER_RESPONSE` mirrors `MAX_ORDERS_PER_RESPONSE`.

### Internal fill store — `canister/src/order/fills` (new module)

Mirrors `OrderHistory`:

- `FillKey { order: OrderId, seq: u64 }` — fixed-width big-endian `Storable` (16 + 8 = 24 bytes,
  bounded), so a range over an `order` prefix yields that order's fills in `seq` order (R4).
- `FillRecord` — the side-projected record (the fields of `Trade` above, internal types),
  minicbor-encoded, `Bound::Unbounded`.
- `FillByUserKey { user: UserId, seq: u64 }` — identical layout to `UserOrderKey`; value is
  `FillKey` (R5). `seq` is the canister-global monotonic fill sequence (R8).
- `FillStore<M>` holds `fills: StableBTreeMap<FillKey, FillRecord>` and
  `by_user: StableBTreeMap<FillByUserKey, FillKey>` in **two distinct memory regions**.
- `append(taker_leg, maker_leg, now)` writes both side-projected records and both `by_user`
  entries (denormalized; 2 + 2 inserts per fill). `fills_after(order, after, length)` is a
  reverse prefix range scan over `fills` (no indirection). `trades_after(user, after, length)`
  reverse-scans `by_user` then `get`s each `FillKey` from `fills` — the exact shape of
  `orders_after`.

### Order-level scalars — `canister/src/order/history`

- Internal `OrderRecord` gains `filled_quote: Quantity` and `filled_fee: Quantity` as new
  trailing minicbor fields (append-only indices; never reused).
- `OrderUpdate` gains `quote_delta: Quantity` and `fee_delta: Quantity`. `OrderUpdate::apply`
  adds them to `filled_quote` / `filled_fee` with `checked_add` and the always-on overflow trap
  (R9), within the same single read-modify-write that already handles `filled_delta` and
  `status` (R7). A no-op update still writes nothing.

### Matching write path — `canister/src/state` (`record_matching_event` / settlement)

Under the existing `Write` gate, and reusing the per-fill computation already in
`compute_balance_operations` (`notional`, `quote_fee`, `base_fee`, and the buyer/seller =
maker/taker roles), for each `fill`:

- Extend the per-order `OrderUpdate` map: the taker order gets `quote_delta += notional` and
  `fee_delta += <taker-side fee>`; the maker order gets `quote_delta += notional` and
  `fee_delta += <maker-side fee>`. (`filled_delta` is already accumulated per DEFI-2852.) Both
  legs share the same `notional`; the `fee_delta` differs by side (R1, R2, R6).
- Build the two side-projected `FillRecord`s (taker leg, maker leg) — each with its own
  `order_id`, `side`, `fee`, `fee_token`, `is_maker` — and call `FillStore::append`, stamped with
  the matching `Event`'s timestamp (R3, R6, R11).

This keeps a single computation of the realized values feeding both balance ops and fills. The
matcher's `Fill` struct is unchanged.

### Storage & lifecycle — `canister/src/storage`, `canister/src/lifecycle`

- Add `FILLS_MEMORY_ID = MemoryId::new(7)` and `FILLS_BY_USER_MEMORY_ID = MemoryId::new(8)` with
  accessors mirroring `order_history_memory` / `user_orders_memory`.
- `init` and `post_upgrade` construct `FillStore::new(fills_memory(), fills_by_user_memory())`
  alongside `OrderHistory`; the regions init fresh and auto-load on upgrade — no
  upgrade-serialization cost (R8).

### Endpoints — `canister/src/lib.rs`, `canister/src/main.rs`

- `get_order_fills(order_id, after, length)`: resolve the caller's `UserId`; if `order_id` is the
  caller's (same ownership check as `get_my_orders { ById }`), return `fills_after`, else an empty
  vec (R4). `length` clamped to `MAX_FILLS_PER_RESPONSE` (R10).
- `get_my_trades(after, length)`: resolve the caller's `UserId`, return `trades_after` (R5, R10).
- Each is a `#[ic_cdk::query]` wrapper in `main.rs` over a business fn in `lib.rs`.

### Test plan

Unit (`*/tests.rs`, helpers/fixtures per repo convention):

- `order/history/tests.rs`: `OrderUpdate::apply` adds `quote_delta` / `fee_delta` in the same
  single write as `filled_delta` and `status` (R7); the monotonic invariant traps on overflow in
  **release config** (always-on, not a compiled-out `debug_assert!`) (R9).
- `order/fills/tests.rs`: `append` writes two side-projected records + two `by_user` entries per
  fill (R3); `fills_after` prefix range scan returns one order's fills newest-first and excludes
  another order's (R4); `trades_after` returns a user's fills across orders newest-first (R5);
  unknown / malformed cursor → empty page; `length` clamped (R10); counterparty fields absent
  from the record (R3).
- `state/tests.rs`: a taker sweeping several maker levels records one fill per level, each at its
  own maker price, with `filled_quote = Σ price×qty` and the buy-taker surplus excluded (R1, R6);
  an order that crosses then rests-and-is-hit records a taker fill (`is_maker = false`) and a
  maker fill (`is_maker = true`) with the respective rates (R6); `filled_fee` denominated base for
  a buy and quote for a sell (R2); replay under `Skip` writes no fills and no scalar deltas (R8).

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Place a maker, hit it with a price-improving taker; `get_order_fills` on each order returns the
  fill at the maker price with correct `notional`/`fee`/`is_maker`/`side`, counterparty absent
  (R3, R4, R6). `get_my_orders` shows `filled_quote` / `filled_fee` consistent with the fills, and
  `filled_quote / filled_quantity` is the expected VWAP (R1, R2).
- `get_order_fills` for an unknown id and for an id owned by another principal → empty (R4).
- `get_my_trades` returns fills across multiple orders newest-first, paginates by `after`, clamps
  `length` (R5, R10).

canbench (R12):

- A settlement-path bench (a taker sweeping N maker levels) measured with and without
  `FillStore::append`, reported as instructions/fill, to size the per-fill insert cost against the
  timer chunk budget. Landed and recorded on the persistence PR.

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
cargo canbench            # settlement path, R12
# + the repo's candid equality / backward-compat check (see justfile / CI)
```

### Delivery / PR sequence

Three stacked PRs, each independently mergeable / compilable / testable.

1. **Order-level scalars.** `OrderRecord += filled_quote, filled_fee` (internal + `libs/types` +
   `.did`); `OrderUpdate += quote_delta, fee_delta` and `apply`; settlement refactored to compute
   the per-fill realized values once and feed the extended `OrderUpdate`. Ships order-level VWAP &
   fees through the existing `get_my_orders` immediately. **Acceptance: R1, R2, R6 (order-level),
   R7, R9, R11.**
2. **Fill store + per-order feed.** New `canister/src/order/fills` module, `FILLS_MEMORY_ID`,
   `FillRecord` / `Trade` types, settlement writes the two side-projected records (Write-gated),
   `get_order_fills` endpoint, and the canbench measurement. **Acceptance: R3, R4, R6 (per-fill),
   R8, R10, R11, R12.**
3. **Account-wide trade feed.** `FILLS_BY_USER_MEMORY_ID` index, `trades_after`, `get_my_trades`
   endpoint. **Acceptance: R5.**

## Discussed Alternatives

- **Normalized storage: one canonical fill record + pointer entries.** Store each fill once under
  a global `FillSeq` and add `by_order` / `by_user` pointer entries (canonical + 4 pointers = 5
  inserts/fill) instead of denormalizing (2 records + 2 user-index entries = 4 inserts/fill).
  Rejected: it is *more* inserts on the settlement hot path (the stated top risk), it forces a
  read indirection (`get` per pointer) even on the common per-order scan, and the canonical record
  must hold both legs and be projected + privacy-filtered per requester. Its only win is ~half the
  storage — and storage has a separate, deferrable mitigation (retention), whereas hot-path
  instructions do not. Denormalized is simpler and cheaper where it matters.

- **Embed fills in `OrderRecord` (a `Vec<Fill>`).** Rejected: O(n²) write amplification — each
  fill re-serializes an ever-growing record through `apply_update` — and it bloats the hot
  `get_my_orders` map with data most reads don't want. The whole point of a separate region is to
  keep the order write O(1).

- **Store a pre-computed average price on the order.** Rejected: not exactly representable as an
  integer `Price`. Exposing exact `filled_quote` and deriving VWAP on read is lossless; the client
  divides (as Binance integrators do).

- **Store the fee rate (bps) per fill and derive the amount.** Rejected: correct only while rates
  never change. The realized amount stays correct across any future rate change without
  rate-versioning or a timestamp join. The flat bps on the pair config still answers "what will I
  pay next."

- **Fold `get_order_fills` into `get_my_orders`.** Rejected: fills are a separate, higher-cardinality
  resource with their own cursor and their own account-wide feed (`get_my_trades`); overloading the
  orders endpoint's filter variant would conflate two pagination domains. A dedicated endpoint per
  the ticket is clearer.
