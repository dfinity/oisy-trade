---
id: DEFI-2737
title: Batch order placement, cancellation, and replacement
tags: [orders, candid, errors, market-making]
---

# Batch order placement, cancellation, and replacement

## Motivation

Every order today costs one update call. A market maker quoting both sides of several pairs
re-quotes continuously, so it pays a round trip per order — and, worse, has no safe way to move a
quote: `cancel_limit_order` and `add_limit_order` are separate calls, so between them the maker is
exposed, and if the create is rejected or the client dies in between, it stays exposed
indefinitely.

The ask came from the field. G20 (Sandeep) asked for "batch place order and batch cancel
orders"; the same primitives are standard on Binance, Kraken, and Coinbase precisely because
they are what a maker's re-quote loop is built from.

This ticket adds three endpoints:

- **`add_limit_orders`** — place up to 100 orders in one call, each succeeding or failing on
  its own.
- **`cancel_limit_orders`** — cancel up to 100 orders in one call, same per-item behavior.
- **`replace_limit_orders`** — cancel a set and create a set **atomically**, so a re-quote either
  lands whole or leaves the maker's existing quotes untouched. It does not make the replacement
  liquidity appear instantly: new orders are enqueued as `Pending` and rest only once the matching
  engine processes them (see Non-goals). What it removes is the *failure-prone* part of the
  window — the replacements are durably accepted before the cancels apply, so the gap is bounded
  by one matching tick instead of by a second round trip that may never succeed.

## Requirements

- **R1 — Batch placement returns one outcome per request.** `add_limit_orders` accepts a `vec
  LimitOrderRequest` and returns a `vec` of per-item outcomes, positionally aligned with the
  request and of the same length. Each item is either the assigned `OrderId` or exactly the
  `AddLimitOrderError` the single-order endpoint would have returned for that request.
- **R2 — Batch placement is sequential placement.** The observable effect of
  `add_limit_orders([a, b, c])` is identical to `add_limit_order(a)`, `add_limit_order(b)`,
  `add_limit_order(c)` issued in that order by the same caller: the same events recorded, the
  same order ids assigned in the same sequence, the same reservations, the same per-item
  errors. The batch differs only in costing one message and arming matching once (R14).
- **R3 — Reservations accumulate within a batch.** Item `k` is validated against the balance
  remaining after items `0..k` have taken their reservations. A batch that overdraws
  therefore fails from the point the funds run out; the items before it stand. (A consequence
  of R2, called out because it is the behavior a client will test.)
- **R4 — Batch cancellation returns one outcome per id.** `cancel_limit_orders` accepts a `vec
  OrderId` and returns a positionally-aligned `vec` of per-item outcomes: the canceled
  `OrderRecord`, or exactly the `CancelLimitOrderError` the single-order endpoint would have
  returned.
- **R5 — Batch cancellation is sequential cancellation.** As R2, for `cancel_limit_order`.
  In particular a repeated id fails its second occurrence with `OrderAlreadyTerminal`, because
  the first occurrence canceled it — no special-casing.
- **R6 — Replace is atomic.** `replace_limit_orders` either applies every cancel and every
  create, or applies nothing at all. There is no partial outcome. On rejection **no event is
  recorded** and no balance moves: the caller's existing quotes are exactly as they were.
- **R7 — Replace frees before it funds.** Every cancel is applied **and settled** — its
  reservation released from `reserved` into `free` — before the first create is recorded, and
  each create is validated against that released balance. A maker re-quoting with the proceeds
  of its own canceled orders must succeed even when its stored free balance alone would not
  cover the new orders. Recording a create against a not-yet-settled cancel would reserve
  against funds that are still locked and fail with a spurious `InsufficientBalance`.
- **R8 — Replace rejects by returning, never by trapping.** A rejected replace returns a typed
  `ReplaceLimitOrdersError` naming the offending leg and index. Reverting state by trapping is
  not an acceptable implementation (it discards the typed error the caller needs).
- **R9 — Duplicate cancel ids are rejected in a replace, ahead of every other cancel check.** The
  `cancel` list is **pre-scanned** for repeated ids; a repeat fails the whole call with
  `DuplicateOrderId` carrying the index of the **second** occurrence, regardless of whether an
  earlier id would also have failed validation. The pre-scan is what makes this outcome
  unconditional — validating in order would surface a `CancelRejected` for an earlier bad id and
  never reach the duplicate. (Unlike R5, the atomic path cannot let the second occurrence fail on
  its own: it would double-count the released reservation in the projection.)
- **R10 — Batch size is capped.** A batch of more than `MAX_BATCH_LEN = 100` items fails the
  whole call with `BatchTooLarge { len; max }` under `RequestError`, before any item is
  processed. For `replace_limit_orders` the cap applies to `cancel` and `create` **combined**.
- **R11 — An empty batch is a successful no-op.** It returns an empty `Ok` and records no
  event, mirroring how `get_balances` treats an empty filter.
- **R12 — The error envelope is preserved at every level.** Every error these endpoints
  produce — per-item, whole-batch, and the `reason` nested inside a replace leaf — is the
  three-arm `{ kind : variant { RequestError : opt variant {…}; TemporaryError : opt variant
  {…}; InternalError : opt variant {…} }; message : opt text }` shape — **every** arm carries
  its own `opt variant` payload, which is precisely what lets a future leaf decode as `null`
  on an old client instead of breaking it. Per-item errors reuse the **existing**
  `AddLimitOrderError` / `CancelLimitOrderError` verbatim: no new item leaves, no second
  definition to keep in sync.
- **R13 — The per-item variant is frozen at two arms.** `variant { Ok; Err }` gains no third
  arm, ever. Any future per-item outcome is added as a **leaf** under one of the three existing
  disposition arms. (See D3: a new arm is a latent, production-only break.)
- **R14 — One matching kickoff per batch, and only when something was enqueued.** A batch that
  successfully enqueues at least one new order arms the zero-delay matching timer exactly once,
  not once per order. `cancel_limit_orders` never arms it — the existing `cancel_limit_order`
  wrapper does not either — and neither does an empty batch nor an `add_limit_orders` call whose
  every item failed. Arming it otherwise would pull unrelated pending orders into a matching round
  that N sequential calls would not have triggered, breaking the equivalence of R2 and R5.
- **R15 — Trading accounts behave as they do on the single-order endpoints.** All three
  endpoints resolve the caller to its funding account via `effective_account`, and record
  `placed_by` / `canceled_by` per order exactly as the single-order paths do.
- **R16 — Halt is enforced per book.** In `add_limit_orders`, a create on a halted pair fails
  that item with `TradingHalted` while items on other pairs proceed. In
  `replace_limit_orders`, because the call is atomic, a create on a halted pair fails the whole
  call; cancels remain permitted under halt, preserving the "users can always exit" guarantee.
- **R17 — A full batch fits one message.** The worst case — 100 cancels, each settling inline —
  stays within the per-message instruction limit, verified by a `canbench` benchmark.
- **R18 — No new event types and no state migration.** A batch records N existing
  `AddLimitOrderEvent` / `CancelLimitOrderEvent` events. Event-log replay, the state snapshot,
  and persisted state are untouched.

## Non-goals

- **Modify / amend an existing order in place.** The ticket flags it as "trickier, maybe
  out-scope" and it is: an in-place amend has to answer whether a re-price keeps its FIFO
  queue position, which is a matching-engine policy question, not a batching one. Under the
  hood it is cancel-plus-create — which `replace_limit_orders` already delivers, with explicit
  rather than implied queue semantics (the new order goes to the back, as any new order does).
- **A `ByPair` / `All` cancel selector.** The standard CEX "cancel all open orders" panic
  button. Deliberately excluded: its cost is not bounded by the caller's input, so it needs its
  own chunking story, and a maker canceling its own book already tracks its ids. Worth a
  follow-up on its own terms.
- **Atomicity for `add_limit_orders` / `cancel_limit_orders`.** These are throughput
  conveniences; per-item partial success is the more useful behavior and the one DEFI-2801 D8
  prescribes. Only `replace_limit_orders` is atomic, and only because eliminating the
  naked-quote window is its entire purpose (see D1).
- **Closing the residual pending window.** `replace_limit_orders` is atomic in *acceptance*, not
  in *resting liquidity*. `State::record_limit_order` enqueues a new order via
  `OrderBook::add_pending_order` with `status = Pending`; it enters the book only when the
  timer-driven matching engine processes it. So between the call committing and the next matching
  tick, the maker's old quotes are gone and its replacements are queued but not yet resting.
  Eliminating that gap would require matching the replacements synchronously inside the update
  call, which the architecture deliberately rejects (see Discussed Alternatives). **Accepted:** the
  window is bounded by one matching tick and the replacements are already durably accepted —
  versus a two-call re-quote, where the window is unbounded and can outlive a crashed client.
- **Batching across users, or batching deposits / withdrawals.** Deposits and withdrawals make
  inter-canister calls and carry a per-(user, token) in-flight guard; batching them is a
  different problem.
- **Cross-pair atomicity guarantees beyond what R6 states.** A replace spanning several pairs
  is atomic as a whole, but this ticket adds no notion of a cross-pair transaction elsewhere.

## Design Decisions

### D1 — Two-level results for add/cancel; a single-level atomic result for replace

`add_limit_orders` and `cancel_limit_orders` adopt the two-level shape
`Result<Vec<Result<T, ItemError>>, BatchError>` that **DEFI-2801 D8 already reserved for exactly
this endpoint** ("No write batch endpoint exists yet; a future `add_limit_orders` would adopt
the two-level shape"). Partial success is the right default for a write batch: re-issuing a
half-applied batch risks double-placing the items that already succeeded, so the caller must be
told precisely which went through.

`replace_limit_orders` is the deliberate exception. Partial success there would defeat the
primitive: a maker whose cancels land but whose creates are rejected is left with nothing even
queued to replace them — the failure mode it called `replace` to avoid. So replace is all-or-nothing, and its result carries
no per-item **outcomes**: it still returns a `vec OrderId` naming the orders it created, but every
entry in it is a success, because any failure would have rejected the whole call before anything
was applied. The single `Err` reports which item was at fault by index (see D5).

The asymmetry is intentional and is the spec's main product decision.

### D2 — Atomicity by validate-then-apply, not by rollback and not by trapping

Replace validates every cancel and every create **before mutating anything**, and returns a
typed error from the validation phase. This mirrors the `plan_fills` / `apply_plan` split
DEFI-2853 introduced for FOK, where the kill decision is likewise made before `apply_plan` runs,
so the no-mutation guarantee is structural rather than test-enforced.

Two alternatives were rejected:

- **Trapping to revert.** A trap does roll back state, but it discards the return value, so the
  caller learns nothing beyond "it trapped" — no leg, no index, no typed reason. R8 forbids it.
- **Applying then undoing.** This is the operation-log rollback DEFI-2853 already considered and
  rejected for the matching engine: every mutation needs a correct, correctly-ordered inverse,
  and a missed one silently corrupts the book.

Validate-then-apply is sound here because trading is fully synchronous — no `await`, no
inter-canister call between the phases (`design.md`, *Synchronous Trading*). Nothing can
interleave, so a validation that passed cannot go stale before it is applied.

### D3 — The per-item variant never gains a third arm

The envelope's forward-compatibility comes from the inner `opt variant`: an old client decodes an
unknown future leaf as `null` while still reading the arm and `message`. **This property is
depth-independent** — the softening is applied per `opt` value, so it works identically for an
error inside a `vec`, and inside the nested `reason` of a replace leaf. An unknown leaf in one
item does not affect its siblings: a known leaf next to it still decodes fully typed.

The exception is the *arm* set. Adding a third arm to `variant { Ok; Err }` — say a `Skipped` for
an exhausted instruction budget — is breaking, and breaks in the worst way: an old client keeps
decoding fine until the first time the canister actually puts `Skipped` on the wire. It is a
latent break that surfaces in production, not at deploy and not in CI.

Hence R13. Any future per-item outcome is a **leaf**, not an arm — and for the "skipped" example a
leaf is also the semantically better answer, since it belongs under `TemporaryError`: the caller
should retry it.

### D4 — `MAX_BATCH_LEN = 100`, matching the existing cap convention

`MAX_FILTER_LEN`, `MAX_ORDERS_PER_RESPONSE`, `MAX_TRADES_PER_RESPONSE`, and `MAX_HALT_BOOKS` are
all 100. A batch cap bounds the per-message instruction cost by the caller's input, which is what
keeps R17 checkable; 100 is well above a realistic re-quote (a maker quoting both sides of a
handful of pairs moves tens of orders) and consistent with the rest of the surface.

### D5 — Replace reports the offending item by index, reusing the existing error types

Being single-level, the replace result has nowhere to hang a per-item error — but the caller still
needs to know what failed and where. Its `RequestError` leaves therefore carry the index plus the
**existing** typed error as a nested `reason`. That keeps R12 at both levels (the nested `reason`
is itself the three-arm envelope and softens independently) and avoids defining a parallel set of
replace-specific reasons that would drift from the single-order ones.

### D6 — No new events

A batch is N existing events. Nothing about the event log, replay, or the snapshot changes, so
this ticket carries no persistence risk and no migration — worth stating explicitly given the
canister is live and back-compat is required.

## Implementation

### Constraints

- **Fully synchronous trading.** No inter-canister calls and no `await` on any of these paths,
  which is what makes D2's validate-then-apply equivalent to a transaction.
- **Event-sourced.** Every mutation flows through `state::audit::process_event` and is re-applied
  on replay. The batch endpoints add no event type (D6); they emit the existing ones N times.
- **Cancellation settles inline.** `State::cancel_limit_order` drains the settling events it
  produces within the same message (`pending_settling_events.split_off(..)`), so a batch cancel's
  cost is N cancels *plus* N inline settlements — the binding case for R17.
- **The refund of a removed order is already a pure function.** `RemovedOrderSettlement::new`
  computes it from `(side, price, remaining_quantity, base_scale)`, all readable from the order
  record. The replace projection needs no bespoke refund math.
- **`check_candid_interface_compatibility`** pins `oisy_trade.did` to the generated interface; the
  new endpoints must be added there and the equality check must pass.
- **Back-compat is required** (the canister is live): these are purely additive endpoints and
  additive Candid types, breaking no existing client.

### Public types & Candid — `libs/types/src/lib.rs`, `libs/types/src/error/mod.rs`, `canister/oisy_trade.did`

```candid
// Maximum items accepted by a batch endpoint (100). For replace_limit_orders
// the cap applies to `cancel` and `create` combined.

type AddLimitOrdersError = record {
    kind : variant {
        RequestError : opt variant {
            BatchTooLarge : record { len : nat32; max : nat32 };
        };
        TemporaryError : opt variant {};
        InternalError : opt variant {};
    };
    message : opt text;
};

// Per-item outcome. This variant has exactly two arms and never gains a third:
// future per-item outcomes are added as leaves under the disposition arms of
// AddLimitOrderError.
type AddLimitOrderOutcome = variant { Ok : OrderId; Err : AddLimitOrderError };

type CancelLimitOrdersError = record { /* as AddLimitOrdersError */ };
type CancelLimitOrderOutcome = variant { Ok : OrderRecord; Err : CancelLimitOrderError };

type ReplaceLimitOrdersRequest = record {
    // Orders to cancel. Applied before any create, so the balance they release
    // funds the creates.
    cancel : vec OrderId;
    // Orders to create, validated against the balance the cancels release.
    create : vec LimitOrderRequest;
};

type ReplaceLimitOrdersError = record {
    kind : variant {
        RequestError : opt variant {
            BatchTooLarge : record { len : nat32; max : nat32 };
            // The same order id appears twice in `cancel`, at `index`.
            DuplicateOrderId : record { index : nat32 };
            // `cancel[index]` cannot be canceled; `reason` is what
            // cancel_limit_order would have returned for it.
            CancelRejected : record { index : nat32; reason : CancelLimitOrderError };
            // `create[index]` is not acceptable; `reason` is what add_limit_order
            // would have returned for it, evaluated against the balance the
            // cancels release.
            CreateRejected : record { index : nat32; reason : AddLimitOrderError };
        };
        TemporaryError : opt variant {};
        InternalError : opt variant {};
    };
    message : opt text;
};

service : {
    add_limit_orders : (vec LimitOrderRequest)
        -> (variant { Ok : vec AddLimitOrderOutcome; Err : AddLimitOrdersError });
    cancel_limit_orders : (vec OrderId)
        -> (variant { Ok : vec CancelLimitOrderOutcome; Err : CancelLimitOrdersError });
    replace_limit_orders : (ReplaceLimitOrdersRequest)
        -> (variant { Ok : vec OrderId; Err : ReplaceLimitOrdersError });
}
```

`AddLimitOrdersError` / `CancelLimitOrdersError` are instantiations of the existing generic
`Error<Request, Temporary, Internal>` (DEFI-2801 D7), so the shape cannot drift.

### `add_limit_orders` / `cancel_limit_orders` — `canister/src/lib.rs`

Both are a loop over the **existing** single-order entry point, which is what makes R2 and R5 true
by construction rather than by test:

- Check the cap once (R10) and return `Ok(vec![])` for an empty batch (R11).
- Hoist the per-call preamble out of the loop: `assert_caller_is_allowed` and the
  `effective_account` resolution run once, not per item.
- For each item, call the existing per-order logic and collect `Ok` / `Err` into the outcome
  vector. An item's failure does not abort the loop.
- R3 needs no special handling: each item is applied before the next is validated, so the balance
  each item sees already reflects its predecessors.

### `replace_limit_orders` — `canister/src/lib.rs`, `canister/src/state`

Two phases, with no mutation in the first (D2).

**Phase 1 — validate, read-only, against a projected free balance.** Balance is the *only* state
the cancels and the creates are coupled through: everything else create-validation touches (pair
registered, halt status, tick/lot via `OrderBook::validate_order`, notional via
`check_notional`, the u256 amount bound) is independent of the cancels. So the projection is a
small per-token overlay rather than a state fork.

```rust
/// Free-balance overlay for validating orders that have not been applied yet:
/// seeded lazily from stored free balance on first touch, credited by each
/// validated cancel's released reservation, debited by each validated create's
/// requirement.
///
/// The credit must mirror the `Unreserve` the cancel's settling will apply
/// (reserved → free), so that phase 1's verdict and phase 2's outcome cannot
/// disagree.
struct ProjectedFreeBalance { /* BTreeMap<TokenId, Quantity> */ }
```

- **Pre-scan** `cancel` for repeated ids and reject the whole call before any per-id validation,
  reporting the second occurrence's index (R9).
- Then walk `cancel` in order: reuse `validate_cancel_limit_order` (well-formed, exists, owned,
  non-terminal) and credit the projection by the reservation the cancel would release — computed
  with `RemovedOrderSettlement::new` from the order record, no book mutation.
- Then walk `create` in order (R7): reuse the existing validation, but read free balance through
  the projection, and debit it per accepted create so creates accumulate against each other as
  well as against the freed funds.
- Any failure returns immediately with the leaf naming the leg and index (D5). Nothing has been
  mutated.

To avoid a second copy of order validation, factor the balance lookup out of
`State::validate_limit_order` behind the projection: the single-order path passes a pass-through
projection, and both paths share one validation body.

**Phase 2 — apply, cancels fully settled first.** Emit every `CancelLimitOrderEvent` **and drain
the settling each produces**, then emit the `AddLimitOrderEvent`s — all through the unchanged
`audit::process_event` path. The ordering is load-bearing, not incidental (R7): a cancel's
reservation moves from `reserved` to `free` only when its `Unreserve` settling event is applied,
so a create recorded before that drain would reserve against funds still locked and fail with a
spurious `InsufficientBalance` — in phase 2, where failure is no longer an option.

`State::cancel_limit_order` already gives this for free: it snapshots
`pending_settling_events.len()` before recording the cancel and drains exactly the events it
added (`split_off`) within the same message. Reusing that path per cancel means each cancel is
settled before the next statement runs. What the implementation must **not** do is batch the
cancel events and defer their settling to a single drain after the creates.

Note the drain is scoped to the cancels' *own* settling events; a pre-existing settling backlog
left by an interrupted matching chunk is deliberately not drained, and the projection likewise
models only the cancels' refunds. This matches how `add_limit_order` validates today — a create
never sees an undrained backlog's credits — so replace introduces no new discrepancy.

Because phase 1 approved every item against a projection that mirrors exactly this settlement,
phase 2 cannot fail; an `expect("BUG: …")` on any per-item failure here is correct (matching the
codebase's always-on invariant convention, not `debug_assert!`).

### Entry points — `canister/src/main.rs`

Three new `#[ic_cdk::update]` wrappers mirroring the existing ones' logging convention (log
successes, do not log user-caused errors). `add_limit_orders` and `replace_limit_orders` arm the
zero-delay `drive_matching` timer **once after the batch**, and **only if at least one order was
actually enqueued** (R14) — `TODO DEFI-2823` (timer coalescing) applies here too and will subsume
the kickoff. `cancel_limit_orders` does **not** arm it: the existing `cancel_limit_order` wrapper
does not, so arming it would drag unrelated pending orders into a matching round that N sequential
cancels would never have triggered — a visible divergence from R5.

### Performance — `canister/src/benchmarks.rs`

A `canbench` benchmark per endpoint at the 100-item cap. The binding case is
`cancel_limit_orders`: cancellation settles inline, so 100 cancels are 100 book removals plus 100
settlements in one message (R17). Note the repo's benchmark CI gate fails on **any** delta against
the committed `canbench_results.yml`, so new benchmarks must be persisted with `just bench-check`.

### Docs — `docs/src/development/design.md`

Add the three endpoints to *Main Endpoints* with their cost, document the partial-success vs
atomic split (D1), and move "Batch operations" out of *Potential Additional Features*.

### Test plan

Unit (`*/tests.rs`, fixtures in `canister/src/test_fixtures`):

- Equivalence (R2, R5): a property test asserting `add_limit_orders(reqs)` leaves state and the
  event log identical to the same requests placed one at a time; likewise for cancel.
- Cumulative reservations (R3): a batch whose later items overdraw — earlier items stand, the
  overdrawing item and its successors report `InsufficientBalance`.
- Cap and empty (R10, R11): 101 items ⇒ `BatchTooLarge`, no state change; 0 items ⇒ `Ok([])` and
  no event recorded.
- Replace atomicity (R6, R8): a replace whose last create is invalid leaves the book, balances,
  and event log **byte-identical** to their pre-call state (compared via the snapshot round-trip),
  and returns `CreateRejected` with the right index — no trap.
- Replace self-funding (R7): a maker whose stored free balance cannot cover the creates, but whose
  cancels release enough, succeeds. Run it with **zero** spare free balance so the creates are
  funded *entirely* by the cancels' released reservations — that is the case that fails if phase 2
  records a create before the cancel's `Unreserve` has been settled, and it should be asserted on
  the resulting `free` / `reserved` split, not just on the `Ok`.
- Duplicate cancel id (R9) and per-item duplicate (R5): the atomic path rejects; the batch path
  returns `OrderAlreadyTerminal` on the second occurrence.
- Envelope forward-compat (R12, R13): encode a batch reply carrying an unknown future leaf and
  decode it against the current types — the unknown leaf softens to `None` in its own item while a
  known leaf in a sibling item still decodes typed, and `message` survives at both levels. This
  extends the existing `should_decode_future_leaf_as_none_keeping_arm_and_message` test to the
  `vec` and nested-`reason` positions.

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Round trip of all three endpoints against a live canister, including per-item error reporting.
- Halt behavior (R16): halted pair fails only its own items in a batch, but fails a whole replace.
- Trading accounts (R15): a trading account batches on its funding account's behalf;
  `placed_by` / `canceled_by` are recorded per order.
- A replace that moves a two-sided quote: cancels and creates apply atomically, and the
  replacements are resting once the matching engine has run. Assert the **intermediate** state
  too — immediately after the call the replacements are `Pending`, not `Open`. That is the
  documented residual window (see Non-goals); the test must pin it, not paper over it.

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
just bench-check
```

### Delivery / PR sequence

Each PR is independently compilable, testable, and useful on its own.

- **PR 1 (1/3) — `add_limit_orders`.** The shared batch scaffolding: `MAX_BATCH_LEN`, the
  batch-level error type, the per-item outcome shape, the hoisted preamble, and the single
  matching kickoff. Placement only.
  - *Acceptance:* R1, R2, R3, R10, R11, R12, R13, R14, R15, R16 (batch half), R18.
- **PR 2 (2/3) — `cancel_limit_orders`.** The same shape applied to cancellation, reusing PR 1's
  scaffolding, plus the inline-settlement benchmark that makes R17 checkable.
  - *Acceptance:* R4, R5, R17.
- **PR 3 (3/3) — `replace_limit_orders`.** `ProjectedFreeBalance`, the validate-then-apply split,
  the factoring of the balance lookup out of `validate_limit_order`, and the atomic endpoint.
  - *Acceptance:* R6, R7, R8, R9, R16 (replace half), plus the design-doc update.

## Discussed Alternatives

- **Trapping to get atomicity for free.** A trap does revert state, but it throws away the typed
  error — the caller cannot learn which leg or which index failed, only that the call trapped.
  That is a strictly worse client contract than the existing single-order endpoints, and it would
  be the only place in the surface where a user-caused rejection is not a returned error.
  Rejected as R8.
- **Apply-then-undo for replace.** Apply the cancels, validate the creates, and unwind the cancels
  if a create fails. Rejected for the same reasons DEFI-2853 rejected operation-log rollback: it
  requires a correct inverse for every mutation, and it makes the *rejection* path the most
  expensive one when rejection should be cheap and side-effect-free.
- **Making all three endpoints atomic.** Consistent, and simpler to describe, but wrong for the
  throughput endpoints: one stale order id would throw away 99 good cancels, and one bad tick size
  99 good orders. It also contradicts DEFI-2801 D8. Atomicity earns its cost only where a partial
  outcome is actively harmful, which is replace and only replace.
- **Making replace partial-success like the others.** Uniform, but it reintroduces the naked-quote
  window that is the entire reason to have the endpoint — a maker would still have to reconcile a
  half-applied re-quote itself, which is what it was trying to avoid by not issuing two calls.
- **A per-item `Skipped` outcome for budget exhaustion.** Tempting for R17, but adding an arm to
  the per-item variant is a latent breaking change (D3), and the fixed cap plus the benchmark make
  the case unreachable. If it is ever needed, it arrives as a `TemporaryError` leaf.
- **A `ByPair` / `All` cancel selector instead of an id list.** Genuinely useful as a panic button,
  but its cost is unbounded by the caller's input, so it needs chunking or its own cap and a story
  for what happens when it cannot finish in one message. Out of scope here; see Non-goals.
- **Matching the replacement orders synchronously inside `replace_limit_orders`,** to make a
  re-quote atomic in resting liquidity rather than only in acceptance. Rejected on the same grounds
  DEFI-2853 rejected synchronous inline FOK matching: it introduces a second matching entry point,
  jumps the batch ahead of orders already queued (breaking FIFO), and turns the per-message
  instruction bound into a hard gate for a call that may carry up to 100 orders. The residual
  window is documented as an accepted limitation in Non-goals instead.
- **One combined `batch(vec Operation)` endpoint** with an operation variant covering create,
  cancel, and replace. More extensible, but it collapses three different result shapes (per-item,
  per-item, atomic) into one type that would have to express all of them, and it makes the
  atomicity contract depend on the contents of the vector rather than on which endpoint was
  called. Three explicit endpoints keep each contract legible.
