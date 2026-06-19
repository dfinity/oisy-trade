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
- **FOK** (new): when the matching engine executes the order, the entire quantity must fill
  against resting liquidity at the order's price or better, otherwise the whole order is killed
  with **zero** execution. No resting, no partial fill.

FOK is the matching-engine half of the swap story; the value to the client is a terminal
"filled or killed" outcome rather than a resting order it has to manage.

## Requirements

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
  user-initiated termination (`cancel_limit_order`, admin sweep) — and `Expired` is reserved for
  system-initiated, time-in-force-driven terminations. This ticket produces exactly one such
  case (a FOK that cannot fully fill); a future IOC would terminate as `Expired` too. A client
  can distinguish "I changed my mind" (`Canceled`) from "the engine couldn't honor my
  time-in-force" (`Expired`).
- **R9 — TIF is durable and observable.** An order's `time_in_force` is recorded on its order
  record, surfaced on `OrderRecord` in the query API, and round-trips through the state
  snapshot and event-log replay. The matching engine can always determine an order's TIF when
  it evaluates it.
- **R10 — Bounded cost.** A worst-case FOK that must inspect liquidity across many price levels
  completes within a single message's instruction/heap limits; it does not allow an order to be
  accepted whose evaluation could exceed canister message limits. (A single FOK is evaluated
  atomically and therefore cannot be chunked across messages — see Design Decisions.)
- **R11 — Async outcome, never `Open`.** `add_limit_order` enqueues a FOK and returns an
  `OrderId` exactly as it does for a GTC order — it does not block on the matching result. The
  FOK is evaluated when the matching engine processes it, transitioning only `Pending → Filled`
  or `Pending → Expired`; it never reaches `Open`. The caller observes the terminal outcome via
  `get_my_orders`.

## Non-goals

- **IOC (Immediate-Or-Cancel).** Conceptually the sibling of FOK (same `TimeInForce` enum, same
  always-taker fee, same `Expired` terminal state — see R8) but it *does* allow partial fill of
  the available quantity. Deferred to a follow-up; it reuses the same plan/execute matching this
  ticket introduces — applying whatever crosses, then **canceling** the unfilled remainder. That
  is a third remainder disposition alongside GTC's *rest* and FOK's *require-full-or-kill*, not
  simply `require_full = false` (which is the GTC path, and rests the remainder).
- **`LIMIT_MAKER` / post-only.** The opposite end of the TIF spectrum (reject if it would
  cross). Separate scope.
- **Self-Trade Prevention and `EXPIRED_IN_MATCH`.** STP is not in scope, so there is no distinct
  STP-driven expiry sub-case to model; `Expired` here means only "FOK could not fully fill".
- **Changing GTC semantics or the FIFO matching order of existing orders** beyond what the
  execution-model decision strictly requires.

## Design Decisions

### Execution model — asynchronous FOK

A FOK order queues as `Pending` exactly like a GTC order; the existing timer-driven executor
evaluates it when it pulls the order for matching, transitioning it only `Pending → Filled` or
`Pending → Expired` — never `Open`, since a FOK never rests.

**Rationale.** Time-in-force governs how long an order stays *active in the book* — and
`add_limit_order` does not put the order in the book. It lands in a pre-processing (pending)
queue and only reaches the book when the matching engine processes it. So the correct moment to
evaluate "can this fill in full" is when the engine pulls the order for matching, not at the
Candid call. This is also exactly Binance's wording — a FOK *"will expire if the full order
cannot be filled upon execution"* — and it keeps FOK on the same execution path, FIFO ordering,
and message-chunking as every other order: no second matching entry point, no per-call
instruction-bound special case.

Matching is **asynchronous and timer-driven**: `add_limit_order` validates the order, reserves
balance, enqueues it as `Pending`, and returns an `OrderId`; a separately-scheduled zero-delay
timer (`drive_matching`) then matches it in a later message (`docs/src/development/design.md`,
Matching Engine). FOK reuses this unchanged. The caller observes the terminal `Filled` /
`Expired` outcome via `get_my_orders`; with the existing zero-delay kick the result is typically
available on the very next poll. (Why not synchronous inline matching — see Discussed
Alternatives.)

### Split matching into a read-only *plan* and a mutating *execute*

`OrderBook::match_order` today fills greedily and **mutates the book as it goes** (it reduces
resting quantities and pops fully-consumed makers in `fill_against_queue`, and rests any
remainder at the tail). So "fill fully or do nothing" cannot be expressed by calling
`match_order` and reacting to a `PartiallyFilled` result — by then the book is already mutated
and partial fills already exist.

We therefore restructure matching into two phases. **`plan_fills`** walks the crossing prices
read-only and records the fills it *would* make (`maker_seq`, fill price, quantity, and whether
the maker is emptied) plus whether the order fully fills — touching no book state.
**`apply_plan`** then replays that plan, performing the mutations. A single parameter,
`require_full`, gates the two: when it is set and the plan does not fully fill, `execute` returns
a *killed* outcome **before `apply_plan` runs**, so the book is provably untouched. GTC calls
`execute(order, require_full = false)` and behaves exactly as today; FOK calls
`execute(order, require_full = true)`.

Chosen over the two narrower alternatives (a non-mutating liquidity *pre-check* that then reuses
the unchanged `match_order`, and an *operation-log rollback* that undoes a partial match — both
in Discussed Alternatives) because plan/execute makes the no-mutation-on-kill guarantee
**structural** rather than test-enforced, keeps a single matching traversal with no duplicated
crossing predicate, and **generalizes to other time-in-force values**: the time-in-force selects
what happens to the taker's unfilled remainder after whatever crossed has been applied — GTC
rests it, FOK requires the whole order to fill or kills it (`require_full`), and a future IOC
would cancel it — all on the same single plan/apply traversal. The cost is a constant-factor
second pass on the GTC hot path (see Performance).

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

### Plan/execute matching — `canister/src/order/book.rs`

Refactor `match_order` into a read-only **plan** and a mutating **apply**, joined by a single
`execute(order, require_full)` entry point. The refactor is behavior-preserving for GTC.

- **`plan_fills(side, price, quantity) -> FillPlan`** — read-only. Iterates the crossing price
  levels best-first (`asks` ascending while `price ≤ order price`; `bids` descending while
  `price ≥ order price` — the *same* crossing predicate as today's `match_order` break) and,
  FIFO within each level, records one `PlannedFill { maker_seq, maker_price, fill_qty,
  maker_emptied }` per maker it would touch, accumulating until the order is satisfied. Returns
  `FillPlan { fills, fully_filled }`. No mutation.
- **`apply_plan(side, &FillPlan, &mut Order, &mut fills_out, &mut filled_orders)`** — mutating.
  Replays the plan: for each `PlannedFill`, reduce the maker (held at the front of its level —
  re-acquire the level cursor only when `maker_price` changes, so cost stays `O(L log p + f)`,
  not `O(f log p)`), reduce the taker, push the `Fill`, and on `maker_emptied` pop the maker,
  drop its `resting_orders` index entry, insert into `filled_orders`, and remove the level if its
  queue empties. An **always-on** check (`expect("BUG: …")` / `assert_eq!`, matching the
  codebase convention — *not* `debug_assert!`, which the release canister compiles out, per the
  DEFI-2852 invariant convention) asserts the maker at the level front matches
  `planned.maker_seq`, trapping on any plan/apply divergence rather than corrupting the book.
- **`execute(order, require_full) -> Execution`** — `let plan = plan_fills(..)`; if
  `require_full && !plan.fully_filled` return `Execution::Killed { seq }` *before* any mutation
  (R4, R5); else `apply_plan(..)` and, per the existing tail, return `Filled` when the remainder
  is zero or rest it (`insert_order`) and return `Resting`. `match_order` becomes
  `execute(order, false)` (GTC unchanged, R2); FOK is `execute(order, true)`.

`MatchingOutput` gains `expired_orders: BTreeSet<OrderSeq>` alongside `fills` / `resting_orders`
/ `filled_orders`. `process_pending_orders` runs the per-order loop — `pop_front`, `execute` with
`require_full` derived from the order's `time_in_force`, and on `Killed` insert into
`expired_orders` (the order, already popped, leaves no book trace) (R3, R4, R5, R10).

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

### Execution wiring — `canister/src/execute`, `canister/src/state` (`record_matching_event`)

`add_limit_order` is unchanged: it validates, reserves balance, and enqueues the FOK as
`Pending`, returning an `OrderId` (R11). The plan/execute branch lives entirely in the order book
(above); `record_matching_event` consumes the resulting `MatchingOutput`, mapping each
`expired_orders` entry to `status = Expired` and releasing the reservation taken at placement. A
FOK never transitions to `Open`; GTC keeps its `Pending → Open` / `Filled` transitions. The
reservation-release on kill reuses the same unreserve/refund computation the cancel path already
performs over the order's full quantity (it never entered the book, so there is no `RemovedOrder`
to read) (R4, R5).

### Performance

The plan/execute refactor costs GTC a **constant-factor second pass**: today's single fused
matching pass becomes a read-only `plan_fills` walk plus an `apply_plan` replay, plus one
transient `O(f)` `FillPlan` allocation (`f` = fills). Asymptotics are unchanged at
`O(L log p + f)` **provided** `apply_plan` holds the level cursor across a level's fills rather
than re-looking-up by price per fill. The non-crossing/rest case is negligibly affected
(`plan_fills` stops at the first non-crossing level). The overhead is expected to be small
relative to the settlement that follows in `record_matching_event` (per-fill fee math and
balance operations, plus stable-memory writes), but it falls on the hot path — confirm with the
`canbench` suite (the `bench_scopes!` instrumentation already in `lib.rs`) before relying on the
estimate.

### Docs — `docs/src/development/design.md`

Document the `time_in_force` field, the asynchronous execution model (FOK is evaluated when the
engine processes it — "upon execution" — and so transitions `Pending → Filled` or
`Pending → Expired` and never rests), and the `Canceled` (user-initiated) vs `Expired`
(system-initiated FOK kill) distinction. (R8, plus the AC requiring the design doc to record the
model.)

### Test plan

Unit (`*/tests.rs`, fixtures in `canister/src/test_fixtures`):

- `order/book.rs` tests:
  - **GTC regression / plan==apply:** a property test over arbitrary books + orders asserts
    `execute(order, false)` produces the identical fills, resting state, and final book as the
    pre-refactor `match_order` — i.e. the refactor is behavior-preserving (R2). The always-on
    plan/apply-divergence check in `apply_plan` holds throughout.
  - **Kill is mutation-free:** when `plan_fills(..).fully_filled` is `false`,
    `execute(order, true)` returns `Killed` and the book is byte-identical to its pre-call state
    — compared via the `OrderBookSnapshot` round-trip (no `PartialEq` on `OrderBook` needed)
    (R4, R5). Covers the *some-but-insufficient* liquidity case explicitly (R5).
  - **Boundary + fill:** available liquidity exactly equal to `quantity` ⇒ `Filled` (inclusive);
    a fillable FOK produces `Filled` with the expected fills (R3). A deep-book worst case stays
    within expected instruction bounds (R10).
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

A stack of three PRs, each independently compilable/testable, that isolate the
behavior-preserving refactor from the FOK feature itself.

- **PR 1 (1/3) — plan/execute refactor (pure implementation detail).** Introduce `plan_fills`
  and `apply_plan`, rewrite `match_order` as plan-then-apply, and add the always-on
  plan/apply-divergence check. **No new public types, no behavior change, no new requirement** —
  this is purely how matching is structured internally. The acceptance signal is that **every
  existing order-book/matching test passes unmodified**; the only test addition is an optional
  new property test asserting plan-then-apply equals the prior `match_order` output. The
  `require_full` gate, `Killed` outcome, and `expired_orders` are *not* introduced here (they
  have no caller yet and would be dead code) — they arrive with the FOK wiring in PR 3.
  - *Acceptance:* existing matching tests green without edits (behavior preserved, R2).
- **PR 2 (2/3) — FOK data model + fee rule.** `TimeInForce` enum; optional `time_in_force` on
  `LimitOrderRequest` defaulting to GTC; `OrderStatus::Expired` (internal + public + Candid);
  `time_in_force` on the order model, record, snapshot, and `OrderRecord`; the always-taker fee
  rule (`fee_rate_for_fill`), unit-tested directly. Types are defined and surfaced but FOK is
  not yet routed through matching.
  - *Acceptance:* R1, R2, R6, R7, R8, R9.
- **PR 3 (3/3) — FOK matching gate + execution wiring.** Add the `require_full` gate to
  `execute` (the `Killed` outcome) and `expired_orders` to `MatchingOutput`; drive `require_full`
  from `time_in_force` in `process_pending_orders`; map `expired_orders` to `Pending → Expired`
  plus reservation release in `record_matching_event`; update `design.md`; add unit + end-to-end
  integration tests.
  - *Acceptance:* R3, R4, R5, R10, R11, and the design-doc AC.

## Discussed Alternatives

- **A separate `add_fok_order` endpoint.** Rejected (per the ticket): the only thing that varies
  is the TIF enum value, so a second endpoint would just duplicate the entire
  tick/lot/notional/balance validation path and create a second place to keep it in sync. One
  endpoint, one extra optional field, mirrors Binance/CEX convention.
- **A payload-carrying `Expired(ExpiredOrderInfo)` (the ticket's draft).** Rejected: DEFI-2852
  already moved per-order fill data to flat `OrderRecord` fields and made `Canceled` a unit
  variant. A unit `Expired` is consistent with the current model; the "executedQty == 0"
  property is expressed by the existing `filled_quantity` field, not a status payload.
- **Synchronous inline FOK (match within the `add_limit_order` call).** This would match a FOK
  against the live book inside the update call and return the terminal outcome in one round-trip
  (closer to Coinbase's "filled immediately at submission"). Rejected: time-in-force describes
  how long an order stays *active in the book*, and the Candid call does not put the order in the
  book — it enqueues it for pre-processing. Evaluating "fill or kill" at the call would conflate
  submission with book-entry. It would also add a second matching entry point, jump the FOK
  ahead of GTC orders already queued (breaking FIFO), and make the per-message instruction bound
  a hard gate because a single atomic FOK cannot be chunked. The asynchronous model (Binance's
  "upon execution") avoids all of this; the only cost is that the caller polls `get_my_orders`
  for the outcome instead of reading it from the call result.
- **Non-mutating liquidity pre-check, then reuse the unchanged `match_order`.** A read-only walk
  summing crossing liquidity gates a kill; if it passes, run today's `match_order` (guaranteed
  `Filled`). *More surgical* — it leaves the GTC hot path at exactly one pass (zero regression)
  and pays the second walk only on FOK; a killed FOK is a single cheap read-only walk. Rejected
  for this ticket in favor of the broader plan/execute refactor: the pre-check's no-mutation
  guarantee is *by avoidance* (it depends on the walk's crossing predicate staying in lockstep
  with `match_order`'s, a divergence risk closed only by a property test), and it does not
  generalize to IOC, which needs to *commit* a partial — exactly what plan/execute's `apply_plan`
  provides. We accept a constant-factor GTC cost (see Performance) to get the structural
  guarantee and the IOC-ready shape.
- **Operation-log rollback.** Run the unchanged `match_order` while journaling each mutation onto
  a stack, then pop-and-invert the stack if a FOK did not fully fill. Rejected: it carries the
  largest correctness surface of the options (every mutating primitive must emit a correct,
  correctly-ordered inverse, or a kill silently corrupts the book), and its performance profile
  is backwards for this feature — it makes the *kill* path the most expensive (full mutate, then
  full reverse with index/level churn) when a kill ("insufficient liquidity") is exactly the case
  that should be cheap and side-effect-free. It also yields no benefit for a future IOC, which
  keeps its partial and never rolls back.
- **A dedicated `PartiallyFilled` / richer FOK-specific status set.** Out of scope and
  unnecessary: FOK only ever reaches `Filled` or `Expired`, both of which already exist (or are
  added as the single `Expired` unit variant).
