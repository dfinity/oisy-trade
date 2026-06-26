---
id: DEFI-2901
title: Persist individual fills and expose a trades / fills feed
tags: [orders, fills, trades, query-api, stable-memory]
---

# Persist individual fills and expose a trades / fills feed

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

So the true execution price — and the volume-weighted average price (VWAP, the average fill
price weighted by filled quantity) — is **not recoverable** from `get_my_orders`. The internal
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
2. **Per-fill records in their own stable-memory regions** — the granular feed, behind one new
   `get_my_trades` endpoint that filters either by order or account-wide.

## Requirements

- **R1 — Cumulative quote on the order.** `OrderRecord` exposes `filled_quote`: the cumulative
  **realized** quote notional transacted, summed as `Σ (maker_price × fill_quantity / base_scale)`
  over the order's fills (`base_scale = 10^base_decimals`; the division converts base
  smallest-units to whole base, matching the engine's `quote_amount`). It is always
  **quote-denominated** and is the *realized* notional: the price the trade actually executed at,
  `maker_price × quantity / base_scale`. (A buy taker that crossed below its
  limit reserved quote at its *limit* price; when it fills cheaper, the difference between that
  reservation and the executed notional is released back to its balance — a reservation
  artifact that was never part of the trade value, not a figure deducted from it. Recording a
  trade at its execution price is universal — see
  [Cross-exchange comparison](#cross-exchange-comparison).) VWAP is derivable as
  `filled_quote × base_scale / filled_quantity`.
- **R2 — Cumulative fee on the order.** `OrderRecord` exposes `filled_fee`: the cumulative
  **realized** fee charged to the order across its fills. It is denominated in the order's
  **receive token** — base for a buy, quote for a sell (the receive-side fee convention in
  `compute_balance_operations`). It is the sum of amounts actually charged, never reconstructed
  from a bps (basis-point) rate. `filled_quantity` is the **gross** matched base amount and does
  *not* net out fees; for a buy, the base-denominated fee is withheld separately, so net base
  received is `filled_quantity − filled_fee` (see [Worked example](#worked-example)).
- **R3 — Per-fill persistence.** Every fill persists, side-projected, the information needed to
  audit an execution: the owning `order_id`, the execution `price` (the maker price), base
  `quantity`, quote `notional`, realized `fee`, `fee_token` (the token the fee is charged in —
  base for a buy, quote for a sell), the `is_maker` role flag, the order's `side`, and a
  `timestamp`. The counterparty's identity is **not** stored or exposed.
- **R4 — One owner-scoped feed, two filter modes.** A single `get_my_trades(filter)` endpoint
  serves both use-cases via a filter:
  - `ByOrder { order_id, after, length }` — the caller's fills for that one order;
  - `ByAccount { after, length }` — the caller's fills across **all** their orders.
  Both are owner-scoped, newest-first, paginated by an `after` cursor, and every entry carries
  its `order_id` so a client can group by order. `ByOrder` for an order owned by another
  principal (or an unknown id) returns an empty page.
- **R5 — Non-trapping, error-enveloped.** `get_my_trades` never traps; it returns the
  DEFI-2801 error envelope (`docs/src/development/specs/DEFI-2801-error-envelope.md`, R8). A malformed `order_id`
  or `after` cursor returns `Err(RequestError(...))`; a well-formed but unknown / not-owned id,
  or an unknown cursor, returns `Ok([])`; otherwise `Ok(<trades>)`.
- **R6 — Correct values under price improvement, sweeping, and refund.** For a fill, `price`
  equals the maker's execution price (never the taker's limit); `notional` equals
  `maker_price × quantity / base_scale` — the executed price, so a buy taker's reservation
  surplus is not part of it; `fee` equals the
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
  fill sequence is canister-global and monotonic (see [Internal fill store](#internal-fill-store)).
- **R9 — Monotonic invariants.** `filled_quote` and `filled_fee` are monotonic non-decreasing;
  each delta is applied with `checked_add` guarded by an **always-on** trap on overflow (a
  `BUG:` panic, matching the codebase convention — not a `debug_assert!`, which is compiled out
  of the release canister).
- **R10 — Bounded pages.** `length` is mandatory in the filter; a value above
  `MAX_FILLS_PER_RESPONSE` is clamped down to it. An unknown `after` cursor yields an empty page
  (malformed cursors are rejected per R5).
- **R11 — Fills and balance ops share one computed source.** The per-fill `notional`, `fee`, and
  role are computed **once** and feed both the existing `BalanceOperation`s and the new fill
  records, so the two can never diverge. They are computed in the settlement path (`State`)
  because that is where the base-token scale lives — see
  [Design Decisions](#design-decisions).
- **R12 — Measured hot-path cost.** The settlement instruction cost is measured with a canbench
  micro-benchmark, with vs. without fill persistence, to characterize the per-fill insert cost
  against the timer chunk's instruction budget.

## Worked example

Pair **ICP / ckUSDT** — base **ICP** (8 decimals), quote **ckUSDT** (6 decimals). Fee rates:
**taker 10 bps (0.10%)**, **maker 5 bps (0.05%)**.

The engine stores everything in **smallest units**; the human-readable value and the stored
value (Rust `_` digit grouping) are given side by side throughout, as human (`stored`):

- `Price` is quote-smallest-units per **whole** base: `10 ckUSDT/ICP` → `10 × 10⁶ = 10_000_000`.
- `quantity` is base-smallest-units: `2 ICP` → `2 × 10⁸ = 200_000_000`.
- `base_scale = 10^base_decimals = 100_000_000`.
- `notional = price × quantity / base_scale`. Fill 1: `10_000_000 × 200_000_000 / 100_000_000 =
  20_000_000` (`= 20 ckUSDT`).

Two resting asks: **Maker A** sells **2 ICP @ 10**, **Maker B** sells **3 ICP @ 11**. A taker
submits a **buy of 5 ICP with limit 12** — it crosses and **sweeps both levels**, producing two
fills, each writing a taker-leg and a maker-leg record (counterparty never named):

| Fill | Taker leg (the buy order) | Maker leg (the resting sell) |
|---|---|---|
| **Fill 1**<br>2 ICP @ 10 | • `side`: Buy<br>• `is_maker`: false<br>• `price`: 10 (`10_000_000`)<br>• `quantity`: 2 ICP (`200_000_000`)<br>• `notional`: 20 ckUSDT (`20_000_000`)<br>• `fee`: 0.002 ICP (`200_000`) *(10 bps × qty)*<br>• `fee_token`: ICP (Base) | • `side`: Sell<br>• `is_maker`: true<br>• `price`: 10 (`10_000_000`)<br>• `quantity`: 2 ICP (`200_000_000`)<br>• `notional`: 20 ckUSDT (`20_000_000`)<br>• `fee`: 0.01 ckUSDT (`10_000`) *(5 bps × notional)*<br>• `fee_token`: ckUSDT (Quote) |
| **Fill 2**<br>3 ICP @ 11 | • `side`: Buy<br>• `is_maker`: false<br>• `price`: 11 (`11_000_000`)<br>• `quantity`: 3 ICP (`300_000_000`)<br>• `notional`: 33 ckUSDT (`33_000_000`)<br>• `fee`: 0.003 ICP (`300_000`) *(10 bps × qty)*<br>• `fee_token`: ICP (Base) | • `side`: Sell<br>• `is_maker`: true<br>• `price`: 11 (`11_000_000`)<br>• `quantity`: 3 ICP (`300_000_000`)<br>• `notional`: 33 ckUSDT (`33_000_000`)<br>• `fee`: 0.0165 ckUSDT (`16_500`) *(5 bps × notional)*<br>• `fee_token`: ckUSDT (Quote) |

Order-level rollups (`OrderRecord` scalars), human (`stored`):

- **Taker buy order** (both fills): `filled_quantity` = 5 ICP (`500_000_000`), `filled_quote` =
  20 + 33 = 53 ckUSDT (`53_000_000`), `filled_fee` = 0.002 + 0.003 = 0.005 ICP (`500_000`,
  `fee_token` ICP). VWAP `= filled_quote × base_scale / filled_quantity = 53_000_000 ×
  100_000_000 / 500_000_000 = 10_600_000` (`= 10.6 ckUSDT/ICP`, between the two maker prices). It
  reserved 60 ckUSDT (`60_000_000`) at its limit (12 × 5); only 53 ckUSDT is spent, so **7 ckUSDT
  (`7_000_000`) is released** back to its balance (`Unreserve`) — never part of `filled_quote`.
  Net ICP received = `filled_quantity − filled_fee` = 4.995 ICP (`499_500_000`).
- **Maker A sell order** (Fill 1): `filled_quantity` = 2 ICP (`200_000_000`), `filled_quote` = 20
  ckUSDT (`20_000_000`), `filled_fee` = 0.01 ckUSDT (`10_000`). Net ckUSDT received = 19.99
  (`19_990_000`).
- **Maker B sell order** (Fill 2): `filled_quantity` = 3 ICP (`300_000_000`), `filled_quote` = 33
  ckUSDT (`33_000_000`), `filled_fee` = 0.0165 ckUSDT (`16_500`). Net ckUSDT received = 32.9835
  (`32_983_500`).

## Non-goals

- **A stored average / VWAP field.** VWAP is derived client-side as
  `filled_quote × base_scale / filled_quantity`. An integer `Price` cannot represent a fractional average
  exactly; storing `filled_quote` (exact) and dividing on read avoids a lossy field. (Kraken /
  Coinbase expose a pre-divided average; we follow Binance — see
  [Cross-exchange comparison](#cross-exchange-comparison).)
- **Storing fee rates (bps) on records, or recomputing fees from the live rate.** The current
  rate is reachable (it lives on `OrderBook`), so a fill's fee *could* be recomputed on read —
  but that is correct only while the rate never changes. Persisting the **realized amount**
  keeps historical fills correct across any future rate change, with no rate-versioning or
  timestamp-join. The flat bps on the pair config still answers "what will I pay next" — a
  different question from the feed's "what did I pay."
- **Exposing the counterparty.** Neither filter reveals the other side's principal, order id, or
  fee — consistent with every venue surveyed.
- **Cross-account / global trade lookup.** The feed is owner-scoped; there is no way to query
  another principal's fills.
- **A retention / pruning policy.** Fill storage grows unbounded (~120–150 B per side-projected
  record). Pagination bounds the *read*, not the *store*. A retention story (archival, TTL,
  caller-paid storage) is a follow-up; this spec only notes the growth. The system is pre-launch,
  so there is no urgency yet.
- **Backfilling pre-existing orders.** Orders that filled before this ships have no persisted
  fills and report `filled_quote == 0` / `filled_fee == 0`. Pre-launch: start fresh, documented.
- **Normalized fill storage.** A single canonical record plus pointer entries was considered and
  rejected — see [Design Decisions](#design-decisions) and
  [Discussed Alternatives](#discussed-alternatives).

## Design Decisions

- **Two scalars on `OrderRecord`, the fill list in its own region — never embedded.** Embedding
  a growing `Vec<Fill>` in `OrderRecord` would re-serialize an ever-larger record on every fill:
  with `n` = the number of fills accumulated against a single order, appending fill `n` rewrites
  all `n` already there, so the total write cost over the order's life is `O(n²)` through
  `apply_update`. It would also bloat the hot `get_my_orders` map. Instead: `filled_quote` and
  `filled_fee` are `O(1)` scalar adds folded into the existing per-order read-modify-write (≈ two
  extra `u128` adds on a write already paid for, R7), and the per-fill records live in dedicated
  stable regions read only by the new feed.

- **Persist realized amounts, never rates (R1, R2).** A fill records the quote and fee that were
  *actually* transacted. This is robust to a future fee-schedule change and sidesteps any
  rate-versioning / timestamp-join needed to reconstruct historical fees. The `is_maker` flag is
  kept as a *descriptive* fact about the fill, not as an input to recompute the fee.

- **`filled_quote` is always quote; `filled_fee` is side-denominated.** A single order's side is
  fixed, so all its fees fall in one token (base for a buy, quote for a sell) and a scalar sum is
  coherent. The asymmetry vs. the always-quote `filled_quote` is real and is made explicit by the
  per-fill `fee_token` field, so a client never has to guess the denomination.

- **One endpoint with a filter, not two endpoints.** Both reads are owner-scoped, return the same
  `Trade`, and paginate the same way; only the scan domain differs (one order's prefix vs. the
  account-wide index). A `ByOrder | ByAccount` filter expresses that with a single ownership
  guard and one cursor convention — mirroring how DEFI-2852 folded point-lookup and paging into
  `get_my_orders` via `ById | ByPage`. (Two separate endpoints — see Discussed Alternatives.)

- **Denormalized, side-projected fill records (written twice), not a normalized canonical
  record.** A fill belongs to two orders (taker + maker); each owner must see their *own* price
  improvement, fee, role, and side, with the counterparty omitted. The two side views differ in
  most fields (`fee`, `is_maker`, `side`, `order_id`), so a normalized canonical record would
  have to carry both legs and be projected (and privacy-filtered) at read time. Denormalizing
  writes two self-contained records and makes the `ByOrder` read a pure prefix range scan with no
  indirection. Chosen because the **settlement hot path** is the stated top risk: denormalized
  is fewer inserts than normalized (see Discussed Alternatives) and storage — the cost it trades
  for — has a separate, deferrable mitigation (retention). (Why not normalized — see Discussed
  Alternatives.)

- **Mirror `OrderHistory`'s storage shape.** The fill store reuses the exact pattern of
  `OrderHistory`: a primary `StableBTreeMap` keyed by an `OrderId`-prefixed key (so `ByOrder`
  reads are a range scan), plus a `(UserId, global_seq)` secondary index for the account-wide
  read — identical to `by_user` / `orders_after`. New `MemoryManager` regions init fresh, so
  there is no upgrade-serialization cost (R8).

- **Compute the per-fill values in settlement, not in the matcher (R11).** The matcher
  (`OrderBook::match_order`) has the `fee_rates` but **not** the base-token scale: `OrderBook` is
  deliberately token-scale-agnostic, and `notional = maker_price × quantity / base_scale` needs
  that scale, which lives in `State`'s token registry (`base_scale_for_book`).
  Settlement (`compute_balance_operations`) is the first point where both `fee_rates` and
  `base_scale` are in scope; it already derives `notional`, `quote_fee`, `base_fee`, and the
  maker/taker roles. Computing the fill values there reuses that single computation and avoids
  threading token metadata into the matcher just to keep `Fill` self-describing. (Why not move it
  into the matcher — see Discussed Alternatives.)

## Cross-exchange comparison

How the proposal lines up with the per-fill and order-level surfaces of the three reference
venues. The takeaway: the proposed feed matches the cross-venue baseline field-for-field; the
only deliberate divergence is deriving VWAP rather than storing it.

| Capability | Binance | Coinbase Advanced | Kraken | This spec |
|---|---|---|---|---|
| Per-fill feed | `myTrades` | List Fills | `TradesHistory` | `get_my_trades { ByAccount }` |
| Filter fills by order | `orderId` param | `order_id` param | by `ordertxid` | `get_my_trades { ByOrder }` |
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
| Order average price | derive | `average_filled_price` | `price` | **derive** (`filled_quote × base_scale / filled_quantity`) |

Notes on the divergences:

- **Reporting at the execution price (excluding the surplus) is universal.** A limit order fills
  at the maker price or better; all three venues record the trade's notional at the *execution*
  price (`quoteQty` / `cost` / `price × size`), never at the submitting order's limit. Our
  "refunded surplus" is just the accounting artifact of reserving quote at the taker's limit and
  releasing the unused part; the reported `notional` / `filled_quote` is the executed notional
  (`maker_price × quantity / base_scale`), exactly as on Binance / Coinbase / Kraken.
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
equality / backward-compat check must pass). The fields are also appended to the minicbor
stable-memory layout at the next free indices (`#[n(9)]` / `#[n(10)]`); they are non-optional and
carry no `#[cbor(default)]`, so they would break decoding of records written by a prior version —
acceptable only because the system is pre-launch with no persisted records (see the *Backfilling
pre-existing orders* non-goal and the `OrderRecord` schema docstring). Post-launch, the same
addition would require an `Option<T>` / `#[cbor(default)]` fallback.

```candid
type OrderRecord = record {
    owner : principal;
    side : Side;
    price : nat;
    quantity : nat;
    filled_quantity : nat;     // base, cumulative, gross of fees (DEFI-2852)
    status : OrderStatus;
    created_at : nat64;        // nanoseconds since the Unix epoch
    last_updated_at : opt nat64;
    time_in_force : TimeInForce;  // existing field (DEFI-2853) — keep it
    // the two new trailing fields appended after the existing ones:
    filled_quote : nat;        // quote, cumulative realized notional (R1)
    filled_fee : nat;          // realized fee, in the order's receive token (R2)
};
```

A new per-fill record and the single feed. `PairToken` is the **existing** type already declared
in `oisy_trade.did` (reused, not redefined):

```candid
// One side-projected fill. The counterparty is intentionally omitted.
// `fee_token` reuses the existing PairToken type { Base; Quote }.
type Trade = record {
    id : FillId;            // this fill's identity; pass the last one back as `after` to paginate
    order_id : OrderId;     // the owning (caller's) order
    side : Side;            // this order's side
    price : nat;            // execution (maker) price
    quantity : nat;         // base filled
    notional : nat;         // quote transacted = price × quantity / base_scale (realized)
    fee : nat;              // realized fee charged to this side
    fee_token : PairToken;  // base for a buy, quote for a sell
    is_maker : bool;        // this side's role on this fill
    timestamp : nat64;      // settlement time, nanoseconds since the Unix epoch
};

// Opaque identity of one side-projected fill — a text token like `OrderId`
// (composite of the owning OrderId and the global fill sequence; see below),
// NOT a number. Treating it as opaque text lets `get_my_trades` distinguish a
// malformed token (Err) from a well-formed-but-unknown one (Ok []) per R5.
type FillId = text;

type TradesFilter = variant {
    ByOrder   : record { order_id : OrderId; after : opt FillId; length : nat32 };
    ByAccount : record { after : opt FillId; length : nat32 };
};

// Owner-scoped, newest-first. Non-trapping: returns the DEFI-2801 error
// envelope (R5). `length` is capped at MAX_FILLS_PER_RESPONSE.
get_my_trades : (TradesFilter) -> (variant { Ok : vec Trade; Err : GetMyTradesError }) query;
```

`FillId` is the opaque fill identity and pagination cursor — an opaque `text` token like `OrderId`,
composing the owning `OrderId` with the global fill sequence (see below); callers pass back the
last value they received and never parse it. Each `Trade` carries its own `id`, so a client
paginates by passing the last entry's `id` as the next `after` — mirroring `get_my_orders
{ ByPage }`, where each `UserOrder` exposes the `OrderId` that doubles as the page cursor.
`GetMyTradesError` is an instantiation of the DEFI-2801 generic error envelope.
`MAX_FILLS_PER_RESPONSE` mirrors `MAX_ORDERS_PER_RESPONSE`.

### Internal fill store

A new module, `canister/src/order/fills`, mirroring `OrderHistory`:

- **The fill sequence (`FillSeq`).** A canister-global, monotonic `u64` assigned to each
  side-projected record as it is appended — so the two legs of one fill get two consecutive
  values, every record is globally ordered, and records within one order's prefix are ordered too.
  `u64` is ample: it matches the existing `OrderSeq` width, is never reused (fills are
  append-only, never deleted), and at any realistic fill rate will not wrap for longer than the
  canister can exist. It is persisted (a stable counter, mirroring `OrderHistory`'s insertion seq)
  so it stays monotonic across upgrades.
- `FillId { order: OrderId, seq: FillSeq }` — the fill's identity and the `after` cursor for both
  filter modes; modelled on `OrderId` (opaque `text` on Candid, fixed-width big-endian `Storable`,
  16 + 8 = 24 bytes, bounded), so a range over an `order` prefix yields that order's fills in `seq`
  order (R4 `ByOrder`). A fill is addressable by its `FillId` directly via `fills.get`.
- `FillRecord` — the side-projected record (the fields of `Trade` above, internal types),
  minicbor-encoded, `Bound::Unbounded`.
- `FillByUserKey { user: UserId, seq: FillSeq }` — identical layout to `UserOrderKey`; value is
  `FillId` (R4 `ByAccount`).
- `FillStore<M>` holds `fills: StableBTreeMap<FillId, FillRecord>` and
  `by_user: StableBTreeMap<FillByUserKey, FillId>` in **two distinct memory regions**, plus the
  `FillSeq` counter.
- `append(taker_leg, maker_leg, now)` writes both side-projected records and both `by_user`
  entries (denormalized; 2 + 2 inserts per fill). `fills_after(order, after, length)` is a
  reverse prefix range scan over `fills` (no indirection). `trades_after(user, after, length)`
  reverse-scans `by_user` then `get`s each `FillId` from `fills` — the exact shape of
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
maker/taker roles — all available there because `base_scale` and `fee_rates` are both in scope),
for each `fill`:

- Extend the per-order `OrderUpdate` map: the taker order gets `quote_delta += notional` and
  `fee_delta += <taker-side fee>`; the maker order gets `quote_delta += notional` and
  `fee_delta += <maker-side fee>`. (`filled_delta` is already accumulated per DEFI-2852.) Both
  legs share the same `notional`; the `fee_delta` differs by side (R1, R2, R6).
- Build the two side-projected `FillRecord`s (taker leg, maker leg) — each with its own
  `order_id`, `side`, `fee`, `fee_token`, `is_maker` — and call `FillStore::append`, stamped with
  the matching `Event`'s timestamp (R3, R6, R11).

This keeps a single computation of the realized values feeding both balance ops and fills (R11).
The matcher's `Fill` struct is unchanged.

### Storage & lifecycle — `canister/src/storage`, `canister/src/lifecycle`

- Add `FILLS_MEMORY_ID = MemoryId::new(7)` and `FILLS_BY_USER_MEMORY_ID = MemoryId::new(8)` with
  accessors mirroring `order_history_memory` / `user_orders_memory`.
- `init` and `post_upgrade` construct `FillStore::new(fills_memory(), fills_by_user_memory())`
  alongside `OrderHistory`; the regions init fresh and auto-load on upgrade — no
  upgrade-serialization cost (R8).

### Endpoint — `canister/src/lib.rs`, `canister/src/main.rs`

- `get_my_trades(filter)`: resolve the caller's `UserId`, then match `filter`. `ByOrder { order_id,
  after, length }` → if `order_id` is the caller's (same ownership check as `get_my_orders {
  ById }`), return `fills_after`, else `Ok([])`. `ByAccount { after, length }` → `trades_after`.
  `length` clamped to `MAX_FILLS_PER_RESPONSE` (R10). Malformed `order_id` / cursor → `Err`,
  unknown → `Ok([])` (R5). A `#[ic_cdk::query]` wrapper in `main.rs` over a business fn in
  `lib.rs`, returning the DEFI-2801 envelope.

### Test plan

Unit (`*/tests.rs`, helpers/fixtures per repo convention):

- `order/history/tests.rs`: `OrderUpdate::apply` adds `quote_delta` / `fee_delta` in the same
  single write as `filled_delta` and `status` (R7); the monotonic invariant traps on overflow in
  **release config** (always-on, not a compiled-out `debug_assert!`) (R9).
- `order/fills/tests.rs`: `append` writes two side-projected records + two `by_user` entries per
  fill and advances `FillSeq` by two (R3, R8); `fills_after` prefix range scan returns one order's
  fills newest-first and excludes another order's (R4 `ByOrder`); `trades_after` returns a user's
  fills across orders newest-first (R4 `ByAccount`); unknown cursor → empty page; `length` clamped
  (R10); counterparty fields absent from the record (R3).
- `state/tests.rs`: the [Worked example](#worked-example) numbers — a buy taker sweeping two
  maker levels (2 ICP @ 10, 3 ICP @ 11) records `filled_quote = 53 ckUSDT`, VWAP `10.6`,
  base-denominated `filled_fee = 0.005 ICP`, with the 7-ckUSDT reservation surplus released (not
  in `filled_quote`), and the two maker orders' `filled_fee` `0.01` / `0.0165 ckUSDT` in quote
  (R1, R2, R6); one fill per swept level at its own maker price (R6); an order that crosses then
  rests-and-is-hit records a taker fill (`is_maker = false`) and a maker fill (`is_maker = true`)
  (R6); replay under `Skip` writes no fills and no scalar deltas (R8).

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Place a maker, hit it with a price-improving taker; `get_my_trades { ByOrder }` on each order
  returns the fill at the maker price with correct `notional`/`fee`/`is_maker`/`side`, counterparty
  absent (R3, R4, R6). `get_my_orders` shows `filled_quote` / `filled_fee` consistent with the
  fills, and `filled_quote × base_scale / filled_quantity` is the expected VWAP (R1, R2).
- `get_my_trades { ByOrder }` for an unknown id and for an id owned by another principal →
  `Ok([])`; a malformed id / cursor → `Err` (R4, R5).
- `get_my_trades { ByAccount }` returns fills across multiple orders newest-first, paginates by
  `after`, clamps `length` (R4, R10).

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
just bench                # settlement path, R12 (canbench)
# + the repo's candid equality / backward-compat check (see justfile / CI)
```

### Delivery / PR sequence

Four stacked PRs, each independently mergeable / compilable / testable.

1. **Order-level scalars.** `OrderRecord += filled_quote, filled_fee` (internal + `libs/types` +
   `.did`); `OrderUpdate += quote_delta, fee_delta` and `apply`; settlement extended to compute
   the per-fill realized values once and feed the extended `OrderUpdate`. Ships order-level VWAP &
   fees through the existing `get_my_orders` immediately. **Acceptance: R1, R2, R6 (order-level),
   R7, R9, R11.**
2. **Fill store (full engine).** New `canister/src/order/fills` module (incl. `FillSeq` and the
   composite `FillId` identity), both `FILLS_MEMORY_ID` and `FILLS_BY_USER_MEMORY_ID` regions,
   `FillRecord` type; settlement resolves each leg's owning `UserId` and writes the two
   side-projected records plus their two `by_user` index entries (Write-gated), durable across
   upgrade / snapshot / event-log replay; the `trades_after` account-wide primitive; and the
   canbench measurement of the full +4-inserts/fill cost. No public retrieval endpoint yet.
   **Acceptance: R3, R6 (per-fill), R8, R11, R12.**
3. **Per-order feed endpoint.** `Trade` / `TradesFilter` / `FillId` Candid types, `get_my_trades`
   with the `ByOrder` filter (error-enveloped, bounded pages), client method, and end-to-end
   tests. **Acceptance: R4 (`ByOrder`), R5, R10.**
4. **Account-wide filter (API only).** The `ByAccount` filter arm, `get_user_trades` wiring over
   the existing `trades_after` store primitive, and the end-to-end test. **Acceptance: R4
   (`ByAccount`).**

## Discussed Alternatives

- **Two endpoints (`get_order_fills` + `get_my_trades`).** The first sketch had a dedicated
  per-order endpoint and a separate account-wide one. Rejected: they return the same `Trade`,
  share the ownership guard and cursor convention, and differ only in scan domain — exactly the
  case a filter variant covers. One endpoint with `ByOrder | ByAccount` is less surface and
  matches the `ById | ByPage` shape DEFI-2852 just introduced on `get_my_orders`. (The opposite
  fold — cramming fills *into* `get_my_orders` — is also rejected: fills are a separate,
  higher-cardinality resource with their own cursor; overloading the orders endpoint would
  conflate two pagination domains.)

- **Compute the per-fill fee/notional in the matcher and carry it on `Fill`.** Tempting for a
  self-describing `Fill`, but `OrderBook` is deliberately token-scale-agnostic — it holds
  `fee_rates` but not `base_scale`, which lives in `State`'s token registry. Computing fee/notional
  at match time would require threading `base_scale` (and token metadata) into the matcher,
  coupling it to the token layer for no gain: settlement already computes these values for the
  balance operations, so computing them there and reusing them (R11) is both cheaper and
  architecture-preserving.

- **Normalized storage: one canonical fill record + pointer entries.** Store each fill once under
  a global `FillSeq` and add `by_order` / `by_user` pointer entries (canonical + 4 pointers = 5
  inserts/fill) instead of denormalizing (2 records + 2 user-index entries = 4 inserts/fill).
  Rejected: it is *more* inserts on the settlement hot path (the stated top risk), it forces a
  read indirection (`get` per pointer) even on the common per-order scan, and the canonical record
  must hold both legs and be projected + privacy-filtered per requester. Its only win is ~half the
  storage — and storage has a separate, deferrable mitigation (retention), whereas hot-path
  instructions do not. Denormalized is simpler and cheaper where it matters.

- **Embed fills in `OrderRecord` (a `Vec<Fill>`).** Rejected: `O(n²)` write amplification (`n` =
  fills on the order) — each fill re-serializes an ever-growing record through `apply_update` — and
  it bloats the hot `get_my_orders` map with data most reads don't want. The whole point of a
  separate region is to keep the order write `O(1)`.

- **Store a pre-computed average price on the order.** Rejected: not exactly representable as an
  integer `Price`. Exposing exact `filled_quote` and deriving VWAP on read is lossless; the client
  divides (as Binance integrators do).

- **Store the fee rate (bps) per fill and derive the amount.** Rejected: correct only while rates
  never change. The realized amount stays correct across any future rate change without
  rate-versioning or a timestamp join. The flat bps on the pair config still answers "what will I
  pay next."
