---
id: DEFI-2853
title: Fill-or-Kill (FOK) limit orders
tags: [orders, time-in-force, matching-engine, fees]
---

# Fill-or-Kill (FOK) limit orders

## Motivation

Today every limit order is implicitly **Good-Til-Canceled** (GTC): it rests in the book until
filled or canceled and may fill partially over time. This is the right primitive for
market-making, but the wrong one for swap-style UX. A swap user expects atomic semantics —
"give me exactly X for at least Y, all at once, or don't touch my funds" — which is exactly
what the Oisy integration needs.

**Fill-or-Kill (FOK)** adds that primitive. It is not a new order type; it is a
`time_in_force` value on the existing limit order:

- **GTC** (current default): rests until filled or canceled; partial fills allowed.
- **FOK** (new): the entire quantity must fill immediately against resting liquidity at the
  order's price or better, otherwise the whole order is killed with **zero** execution. No
  resting, no partial fill.

FOK is the matching-engine half of the swap story; the value to the client is a terminal
"filled or killed" outcome rather than a resting order it has to manage.

## Requirements

The execution-model decision (synchronous vs asynchronous matching — see Design Decisions) is
**still open**. The requirements below are written to hold under either model; the few places
where the model changes observable behavior are called out as **(model-dependent)** and
resolved once the decision is made.

- **R1 — Optional TIF, backwards compatible.** `LimitOrderRequest` accepts an optional
  `time_in_force`. An absent value defaults to `GoodTilCanceled`, so every existing client
  keeps working unchanged.
- **R2 — GTC unchanged.** A `GoodTilCanceled` order (explicit or defaulted) behaves exactly as
  today: it may rest, may fill partially, and reaches `Open` / `Filled` / `Canceled` through
  the existing transitions. No GTC observable behavior changes.
- **R3 — FOK full fill.** A FOK order whose full `quantity` can be satisfied against resting
  liquidity at its price or better fills completely and reaches `status == Filled` with
  `filled_quantity == quantity`. It never rests in the book.
- **R4 — FOK kill.** A FOK order whose full quantity *cannot* be satisfied reaches
  `status == Expired` with `filled_quantity == 0`, leaves **no** trace in the book (it never
  rests), and moves no balances (the reservation taken at placement is fully released).
- **R5 — No partial FOK.** The "some liquidity but not enough" case is killed exactly like the
  "no liquidity" case: `Expired`, `filled_quantity == 0`. FOK never settles a partial fill.
- **R6 — FOK always pays taker.** Every fill produced by a FOK order is charged the taker fee
  rate (`FeeRates.taker`), on both the base and quote legs as applicable, regardless of pair
  and regardless of which resting order it crossed. A FOK can never be a maker.
- **R7 — GTC fees unchanged.** GTC fee assignment is untouched: the resting side of a fill pays
  the maker rate, the incoming (crossing) side pays the taker rate, exactly as today.
- **R8 — `Expired` is distinct from `Canceled`.** `OrderStatus` gains a new **unit** variant
  `Expired`, surfaced in the public Candid type. `Canceled` keeps its current meaning —
  user-initiated termination (`cancel_limit_order`, admin sweep) — and `Expired` is reserved
  for system-initiated FOK kills. A client can distinguish "I changed my mind" (`Canceled`)
  from "the engine couldn't honor my FOK" (`Expired`).
- **R9 — TIF is durable and observable.** An order's `time_in_force` is recorded on its order
  record, surfaced on `OrderRecord` in the query API, and round-trips through the state
  snapshot and event-log replay. The matching engine can always determine an order's TIF when
  it evaluates it.
- **R10 — Bounded cost.** A worst-case FOK that must inspect liquidity across many price levels
  completes within a single message's instruction/heap limits; it does not allow an order to be
  accepted whose evaluation could exceed canister message limits. (A single FOK is evaluated
  atomically and therefore cannot be chunked across messages — see Design Decisions.)
- **R11 — Outcome delivery (model-dependent).** How the caller learns the FOK outcome
  (`Filled` / `Expired`) — whether it is returned by the `add_limit_order` call itself
  (synchronous model) or observed via a subsequent `get_my_orders` lookup (asynchronous model)
  — is fixed by the execution-model decision. This requirement is a placeholder until then.

## Non-goals

- **IOC (Immediate-Or-Cancel).** Conceptually the sibling of FOK (same `TimeInForce` enum, same
  always-taker fee, same `Expired` terminal state) but it *does* allow partial fill of the
  immediately-available quantity. Deferred to a follow-up; it should reuse whatever
  atomic/synchronous-matching infrastructure FOK introduces.
- **`LIMIT_MAKER` / post-only.** The opposite end of the TIF spectrum (reject if it would
  cross). Separate scope.
- **Self-Trade Prevention and `EXPIRED_IN_MATCH`.** STP is not in scope, so there is no distinct
  STP-driven expiry sub-case to model; `Expired` here means only "FOK could not fully fill".
- **Changing GTC semantics or the FIFO matching order of existing orders** beyond what the
  execution-model decision strictly requires.

## Design Decisions

### Execution model — **OPEN (A vs B), to be decided before the contingent PR**

FOK's "immediate or kill" semantics sit in tension with the engine's current shape. Matching is
**asynchronous and timer-driven**: `add_limit_order` validates the order, reserves balance,
enqueues it as `Pending`, and returns an `OrderId`; a separately-scheduled zero-delay timer
(`drive_matching`) then matches it in a later message (`docs/src/development/design.md`,
Matching Engine). Trade matching and settlement are **fully synchronous with no inter-canister
calls** (design.md "No async complexity during matching") — settlement just moves reserved
balances between users — so an inline matching path is technically feasible; the timer exists for
*chunking* (instruction-limit headroom, DEFI-2724), not because settlement needs async.

**Option A — synchronous FOK.** A FOK order bypasses the pending queue and is matched inline,
within the `add_limit_order` update call. The caller gets the terminal `Filled` / `Expired`
outcome in a single round-trip.
- *Pro:* delivers the swap UX literally — one Candid call returns the result; the book state at
  submission is the state matched against.
- *Con:* a second matching entry path alongside the timer-driven one.
- *Con:* a single FOK is atomic and so **cannot be chunked**; a FOK sweeping many price levels
  does all that work in one message, so R10 becomes a hard acceptance gate rather than headroom.
- *Con:* inline matching jumps ahead of GTC orders already waiting in the pending queue,
  breaking strict FIFO unless the queue is drained first.

**Option B — asynchronous FOK.** A FOK order queues as `Pending` like GTC; the existing
timer/executor evaluates it, transitioning it only `Pending → Filled` or `Pending → Expired`
(never `Open`).
- *Pro:* zero architectural change — reuses the executor, the chunking, and FIFO ordering.
- *Con:* "immediate" means "book state when the timer fires", not "at submission". With the
  existing zero-delay kick this is typically the very next message, but other orders in the same
  burst are processed first.
- *Con:* not a single-call swap — the caller must poll `get_my_orders` for the outcome.

**Recommendation: lean A**, because the swap motivation (single-call "swap and tell me the
result") is the entire reason FOK exists, and synchronous settlement makes it feasible. The two
real costs to weigh before committing are the **per-message instruction bound** (R10) and
**FIFO fairness** versus already-queued orders. **This decision does not block the bulk of the
work** — see the PR sequence: the TIF data model, the `Expired` status, the always-taker fee
rule, and the atomic-fill primitive are all model-independent and ship first; only the final
wiring PR depends on A vs B.

### FOK atomicity needs a non-mutating liquidity check — independent of A/B

`OrderBook::match_order` fills greedily and **mutates the book as it goes** (it reduces resting
quantities and pops fully-consumed makers in `fill_against_queue`). So "fill fully or do
nothing" cannot be expressed by calling `match_order` and reacting to a `PartiallyFilled`
result — by then the book is already mutated and partial fills already exist. FOK therefore
needs the fill to be *gated*: first determine whether the full quantity is satisfiable at the
order's price or better **without mutating**, then either execute (which is guaranteed to fully
fill) or kill (touching nothing). This primitive is the same regardless of whether it runs
inline (A) or in the timer (B), which is why it lands in the model-independent PR.

### `Expired` is a unit variant, matching the current `Canceled`

The ticket drafted `Canceled(CanceledOrderInfo)` and `Expired(ExpiredOrderInfo)`, but
**DEFI-2852 has since landed** and made `Canceled` a plain unit variant (and removed
`NotFound`): `OrderStatus = { Pending, Open, Filled, Canceled }`. Per-order fill data now lives
in flat `OrderRecord` fields (`filled_quantity`, `last_updated_at`), not in a status payload.
`Expired` follows that established shape — a **unit** variant — and a killed FOK is simply a
record with `status == Expired` and `filled_quantity == 0`. No `ExpiredOrderInfo` struct.

### Dependencies are satisfied on current main

The ticket lists DEFI-2848 (price encoding) and DEFI-2850 (min/max notional) as prerequisites;
both, plus DEFI-2852 (order status), are merged. The scaled-settlement math
(`Price::checked_mul_quantity_scaled`, `Fill::quote_amount`) and the shared notional gate
(`OrderBook::check_notional`, called from `State::validate_limit_order`) are present, so FOK
orders pass through the same tick/lot/notional/balance validation as GTC with no extra work
beyond confirming the path is shared (R-coverage in the test plan).

## Implementation

### Constraints

- The canister is **event-sourced**: an `AddLimitOrderEvent` is recorded via
  `state::audit::process_event` and re-applied on replay; matching results are applied by
  `State::record_matching_event` **only under `StableMemoryOptions::Write`** (replay runs
  `Skip`). Any new persistence (the TIF field, the `Expired` transition) must flow through this
  chain and respect the `Write` gate so replay does not double-apply.
- Matching/settlement are synchronous and free of inter-canister calls, but bounded by the
  per-message instruction limit; the timer-driven model chunks GTC matching to stay within it. A
  single FOK is atomic and cannot be chunked (R10).
- `OrderStatus` is shared between the internal engine (`canister/src/order/mod.rs`) and the
  public Candid type (`libs/types/src/lib.rs`); both must gain `Expired`.

### Public types & Candid — `libs/types/src/lib.rs`, `canister/oisy_trade.did`

- New `TimeInForce` enum: `GoodTilCanceled`, `FillOrKill`.
- `LimitOrderRequest` gains `time_in_force: Option<TimeInForce>` (absent ⇒ `GoodTilCanceled`,
  R1). Candid: `time_in_force : opt TimeInForce`.
- `OrderStatus` gains the unit variant `Expired` (R8). Candid:
  `variant { Pending; Open; Filled; Canceled; Expired }`.
- `OrderRecord` gains `time_in_force: TimeInForce` (R9).
- The candid equality check must pass (`oisy_trade.did` regenerated/updated).

### Order model — `canister/src/order`

- `PendingOrder` / `Order` carry `time_in_force`; `PendingOrder::try_from(LimitOrderRequest)`
  reads the optional field, defaulting to `GoodTilCanceled`.
- Internal `OrderStatus` (`order/mod.rs`) gains `Expired` (next minicbor index).
- Internal `OrderRecord` (`order/history`) gains `time_in_force` as a new trailing minicbor
  field (append-only index; never reuse) so it round-trips through history and snapshot (R9).

### Atomic fill primitive — `canister/src/order/book.rs`

Add a non-mutating check that answers "can an order of this `side`/`price`/`quantity` be fully
filled against current resting liquidity at price-or-better?" — e.g. an
`available_liquidity_at_or_better(side, price, up_to: quantity)` walk over `asks`/`bids` that
sums matchable resting quantity and short-circuits once it reaches `quantity`. FOK execution
calls this first: if satisfiable, run the existing `match_order` (now guaranteed to return
`Filled`); otherwise produce a kill outcome that mutates nothing. The `MatchResult` /
`MatchingOutput` surface gains a way to express "killed/expired" so the caller can record the
`Expired` status and release the reservation. (R3, R4, R5, R10.)

### Fee logic — `canister/src/state` (`compute_balance_operations`)

Fees are assigned per fill from `fill.taker_side` (resting side → maker rate, crossing side →
taker rate). For a FOK fill, **both** legs use the taker rate (R6): the engine must know the
order's TIF at settlement time. Thread the taker order's `time_in_force` into the fee
computation; when it is `FillOrKill`, charge `FeeRates.taker` on both the maker and taker legs
of every fill the FOK produced. GTC assignment is unchanged (R7).

> Helper shape (from the ticket), adapted to the unit-status world:
> ```rust
> fn fee_rate_for_fill(tif: TimeInForce, was_maker: bool, rates: &FeeRates) -> BasisPoint {
>     match tif {
>         TimeInForce::FillOrKill => rates.taker,
>         TimeInForce::GoodTilCanceled => if was_maker { rates.maker } else { rates.taker },
>     }
> }
> ```

### Execution wiring — **gated on the A/B decision**

- **If A (synchronous):** in `canister/src/lib.rs::add_limit_order`, branch on TIF after
  validation; for FOK, run the gated atomic match inline (against the live book, after deciding
  how to order relative to any queued GTC orders — R10/FIFO), settle on success, record
  `Filled` or `Expired`, release the reservation on kill, and return the terminal outcome (R11
  shape: a richer `Ok` payload or an immediate status). Bound the inline sweep (R10).
- **If B (asynchronous):** enqueue the FOK as `Pending` as today; in the executor / 
  `record_matching_event`, when dequeuing a FOK use the gated atomic match and transition
  `Pending → Filled` or `Pending → Expired` (never `Open`), releasing the reservation on kill.
  `add_limit_order` keeps returning `OrderId`; the caller polls `get_my_orders` (R11).

### Docs — `docs/src/development/design.md`

Document the `time_in_force` field, the **chosen** execution model (sync vs async) and why, and
the `Canceled` (user-initiated) vs `Expired` (system-initiated FOK kill) distinction, including
that a FOK never reaches `Open`. (R8, plus the AC requiring the design doc to record the model.)

### Test plan

Unit (`*/tests.rs`, fixtures in `canister/src/test_fixtures`):

- `order/book.rs` tests: the liquidity check reports "fully fillable" exactly when summed
  resting quantity at price-or-better ≥ `quantity`, and the gated match leaves the book
  untouched when not fillable (R4, R5); a fillable FOK produces `Filled` with the expected fills
  (R3). A deep-book worst case stays within expected instruction bounds (R10).
- `state/tests.rs`: a FOK that fully fills records `Filled` / `filled_quantity == quantity` and
  releases nothing extra (R3); a FOK that can't fill records `Expired` / `filled_quantity == 0`,
  no book trace, reservation released (R4, R5); FOK fills are charged `FeeRates.taker` on both
  legs (R6); GTC fee assignment unchanged — maker when resting, taker when crossing (R7);
  defaulted TIF is `GoodTilCanceled` (R1, R2).
- `order/history` + `state/snapshot` tests: `time_in_force` round-trips through the record and a
  snapshot; replay under `Skip` does not re-settle a FOK (R9).

Integration (`integration_tests/tests/tests.rs`, PocketIC) — these encode the acceptance
criteria end-to-end:

- FOK against sufficient resting liquidity ⇒ `Filled`, `filled_quantity == requested` (R3).
- FOK against no liquidity ⇒ `Expired`, `filled_quantity == 0`, no resting trace (R4).
- FOK against insufficient liquidity (some, but < quantity) ⇒ `Expired`, `filled_quantity == 0`
  (R5 — the no-partial guarantee).
- Fee on a FOK fill equals the taker rate, on an asymmetric-decimal pair (R6).
- GTC fees unchanged: maker-if-resting, taker-on-immediate-cross (R7).
- `Expired` is distinct from `Canceled` in the Candid surface; a user cancel still yields
  `Canceled` (R8).
- Absent `time_in_force` behaves as GTC (R1, R2).

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
# + the repo's candid equality check (see justfile / CI)
```

### Delivery / PR sequence

Structured so the open A/B decision blocks only the last PR.

- **PR 1 (1/2) — model-independent foundation.** `TimeInForce` enum; optional `time_in_force`
  on `LimitOrderRequest` defaulting to GTC; `OrderStatus::Expired` (internal + public + Candid);
  `time_in_force` on the order model, record, snapshot, and `OrderRecord`; the non-mutating
  liquidity check + gated atomic-fill primitive in `OrderBook`; the always-taker fee rule for
  FOK in `compute_balance_operations`. Covers R1, R2, R6, R7, R8, R9, and the primitive behind
  R3/R4/R5/R10. At this point a FOK request is *accepted and parsed* and the engine *can* fully-
  fill-or-kill, but nothing yet routes FOK orders down the kill path end-to-end.
  - *Acceptance:* R1, R2, R6, R7, R8, R9; unit coverage of the fill primitive (R3/R4/R5/R10).
- **PR 2 (2/2) — execution wiring (gated on A vs B).** Route FOK orders through the chosen
  model (inline for A, executor for B), wire the terminal outcome / reservation release, update
  `design.md` with the chosen model, and add the end-to-end integration tests.
  - *Acceptance:* R3, R4, R5, R10, R11, and the design-doc AC.

If the A/B decision is made up front, the two PRs can merge back-to-back; if not, PR 1 proceeds
independently and PR 2 waits on the decision.

## Discussed Alternatives

- **A separate `add_fok_order` endpoint.** Rejected (per the ticket): the only thing that varies
  is the TIF enum value, so a second endpoint would just duplicate the entire
  tick/lot/notional/balance validation path and create a second place to keep it in sync. One
  endpoint, one extra optional field, mirrors Binance/CEX convention.
- **A payload-carrying `Expired(ExpiredOrderInfo)` (the ticket's draft).** Rejected: DEFI-2852
  already moved per-order fill data to flat `OrderRecord` fields and made `Canceled` a unit
  variant. A unit `Expired` is consistent with the current model; the "executedQty == 0"
  property is expressed by the existing `filled_quantity` field, not a status payload.
- **Reacting to `match_order`'s `PartiallyFilled` result instead of pre-checking liquidity.**
  Rejected: `match_order` mutates the book and creates real fills as it runs, so by the time it
  reports `PartiallyFilled` the partial execution has already happened and would need rolling
  back. A non-mutating liquidity check that gates execution is simpler and has no rollback path.
- **A dedicated `PartiallyFilled` / richer FOK-specific status set.** Out of scope and
  unnecessary: FOK only ever reaches `Filled` or `Expired`, both of which already exist (or are
  added as the single `Expired` unit variant).
