---
id: DEFI-2850
title: Min/max notional filter per trading pair
tags: [order-book, trading-pair, validation]
---

# Min/max notional filter per trading pair

## Motivation

A trading pair today constrains only the **granularity** of price (`tick_size`) and quantity
(`lot_size`). It places no constraint on an order's **value** — its notional, i.e. the quote
amount that settles, `price × quantity / 10^base_decimals`.

Tick and lot are orthogonal to notional: they bound increments, not the product's worth. Under
realistic Binance-equivalent parameters for ckETH/ckUSDC (`tick_size = 10_000`,
`lot_size = 10^14`) the smallest order that passes tick/lot is `1 tick × 1 lot` ≈ `$10⁻¹²` —
pure dust, worth far less than the ICRC fee required to settle it. A canister that accepts such
orders bleeds cycles processing trades that can never cover their own settlement cost. There is
also no upper guard against fat-finger orders.

This adds two filters, modeled on Binance's `NOTIONAL` filter:

- `min_notional` (required): rejects dust, and serves as the natural place to keep an order
  worth at least the cost of settling it (the ICRC transfer-fee floor — set manually for now).
- `max_notional` (optional): rejects fat-finger orders and caps single-order impact.

These apply at limit-order placement. The canister has no market orders, so the
average-price / `applyMinToMarket` machinery from Binance does not apply.

## Requirements

- R1: An order whose notional `< min_notional` is rejected with `BelowMinNotional`.
- R2: An order whose notional `> max_notional` (when `max_notional` is set) is rejected with
  `AboveMaxNotional`.
- R3: An order whose notional `== min_notional` exactly is accepted (boundary is inclusive).
- R4: Pair creation rejects `min_notional == 0`.
- R5: Pair creation rejects `max_notional < min_notional` (when `max_notional` is set).
- R6: tick, lot, `min_notional`, and `max_notional` are enforced independently — an order may
  fail any one of them, and none is implied by another. No relationship is enforced between
  `min_notional` and `tick_size × lot_size` (a `min_notional` larger than one tick·lot is the
  normal, intended case).
- R7: `min_notional` and `max_notional` round-trip through the state snapshot.

## Non-goals

- **Per-token transfer-fee-aware auto-floor** (enforcing `min_notional ≥ icrc_transfer_fee` at
  pair creation). Deferred until the ledger fee is queryable; for now an operator sets
  `min_notional` manually with the transfer fee in mind.
- **Market orders.** None exist in the canister; the Binance `avgPriceMins` /
  `applyMinToMarket` / `applyMaxToMarket` behavior is therefore irrelevant here.
- **Dynamic min-notional** based on volatility or an oracle price.

## Design Decisions

**Notional is the scaled quote amount, not the raw product.** Notional is defined as
`price × quantity / 10^base_decimals` — exactly the `quote_amount` that settlement already
computes via `Price::checked_mul_quantity_scaled`. This is the only definition under which a
bound like "min_notional = 5 USDC" is meaningful; the raw `price × quantity` is off by a factor
of `10^base_decimals` and has no quote-token interpretation. The bound type is therefore
`Quantity` (the 256-bit `(high, low)` type), the same type `quote_amount` returns.

**The check lives in `State::validate_limit_order`, not `OrderBook::validate_order`.** Computing
the scaled notional needs `base_scale = 10^base_decimals`, which is derived from token metadata
held by `State` and is not available inside `OrderBook`. This is already why the existing
`AmountExceedsMaximum` overflow guard sits in `State::validate_limit_order` rather than in the
order book. The notional bounds reuse the `amount` that guard already computes.
`OrderBook::validate_order` stays tick/lot-only; the bound *values* are still stored on
`OrderBook` alongside `tick_size`/`lot_size`, since they are immutable per-pair configuration.

**`max_notional` is optional.** Not every pair needs a cap; `None` means no upper bound.

## Implementation

Bound types: `min_notional: Quantity`, `max_notional: Option<Quantity>`. Public API surfaces
them as `Nat` / `Option<Nat>`.

### Constraints

- `base_scale` (= `10^base_decimals`) is only available at the `State` layer.
- Trading-pair configuration is event-sourced: `add_trading_pair` builds an
  `AddTradingPairEvent`, the audit handler applies it via `record_trading_pair`, and snapshots
  persist the resulting `OrderBook`. New configuration must flow through every link in that
  chain.

### Public types — `libs/types/src/lib.rs`

- `AddTradingPairRequest`: add `min_notional: Nat` and `max_notional: Option<Nat>`.
- `AddTradingPairError`: add variants for the two pair-creation rejections (R4, R5).
- `AddLimitOrderError`: add `BelowMinNotional` and `AboveMaxNotional`, each carrying the
  rejected `notional` and the relevant bound, mirroring the existing `InvalidPrice` shape.
- `TradingPairInfo`: surface `min_notional` and `max_notional` in the query response.

### Pair creation — `canister/src/lib.rs::add_trading_pair`

Parse `min_notional` / `max_notional` from `Nat` into `Quantity`; reject `min_notional == 0`
(R4) and `max_notional < min_notional` (R5); carry both into `AddTradingPairEvent`.

### Event plumbing — `canister/src/state/event.rs`, `canister/src/state/audit/mod.rs`

Add the two fields to `AddTradingPairEvent`; destructure and forward them through the
`AddTradingPair` handler into `record_trading_pair`.

### Order book — `canister/src/order/book.rs`

`OrderBook` gains `min_notional` / `max_notional` fields, set via `OrderBook::new` and exposed
through getters. `validate_order` is unchanged (tick/lot only).

### State — `canister/src/state/mod.rs`

- `record_trading_pair`: accept the two bounds and forward to `OrderBook::new`.
- `validate_limit_order`: after computing `amount` (the scaled notional), reject with the new
  internal `AddLimitOrderError::BelowMinNotional` when `amount < min_notional` (R1) and
  `AboveMaxNotional` when `max_notional` is set and `amount > max_notional` (R2); `==` passes
  (R3). Extend the internal `AddLimitOrderError` enum and its `From` mapping to
  `dex_types::AddLimitOrderError`.

### Persistence — `canister/src/state/snapshot/mod.rs`

`OrderBookSnapshot` persists and restores the two bounds (R7).

### Interface & docs

- `canister/dex.did`: update `AddTradingPairRequest`, `AddTradingPairError`,
  `AddLimitOrderError`, and `TradingPairInfo`.
- `docs/design.md`: document the two filters in the pair-parameters section alongside tick/lot.

### Test plan

Helpers in `canister/src/test_fixtures/mod.rs`: add `MIN_NOTIONAL` / `MAX_NOTIONAL` constants
and thread them through `trading_pair_request()` and `init_state_with_order_book()`.

- `canister/src/state/tests.rs`
  - R1: notional below `min_notional` → `BelowMinNotional`.
  - R2: notional above `max_notional` → `AboveMaxNotional`.
  - R3: notional exactly `min_notional` → accepted.
  - R6: an order that satisfies tick/lot but fails a notional bound, and one that satisfies the
    notional bounds but fails tick/lot — confirming independence. Confirm the existing
    `validate_overflow_invariant` prop-test still holds.
- `canister/src/tests.rs` (`add_trading_pair` module)
  - R4: `min_notional == 0` rejected.
  - R5: `max_notional < min_notional` rejected.
- `canister/src/state/snapshot/tests.rs`
  - R7: bounds survive a snapshot round-trip.

Verification commands: `cargo fmt --all`, `just lint`, `cargo test` (workspace).

### Delivery / PR sequence

Single PR. The feature is small and cohesive — the data model (request → event → `OrderBook` →
snapshot → query/did), pair-creation validation, order-time enforcement, and the design-doc
update ship together as one independently compilable and testable draft PR.

- PR 1 (1/1): all requirements R1–R7.

## Discussed Alternatives

- **Check in `OrderBook::validate_order` against the raw `price × quantity`** (the ticket's
  literal pseudocode). Rejected: `OrderBook` has no `base_scale`, so it cannot compute the
  scaled quote amount, and the raw product is not a quote-token value — a `min_notional`
  expressed against it would be off by `10^base_decimals` and meaningless. Threading
  `base_scale` into `OrderBook` would duplicate state that already lives, by deliberate design,
  at the `State` layer (where the overflow guard already is).
- **Storing the bounds outside `OrderBook`** (e.g. a separate per-pair config map). Rejected:
  the bounds are immutable per-pair configuration of the same kind as `tick_size`/`lot_size`,
  which already live on `OrderBook`; co-locating them keeps one source of truth.
