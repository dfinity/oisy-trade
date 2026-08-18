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
  window — every leg is validated before anything mutates and the call commits as a unit, so there
  is no outcome where the cancels land and the replacements do not. The gap that remains is
  bounded by one matching tick, rather than by a second round trip that may never succeed.

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
  Equivalence is **modulo timestamps and inter-message interleaving**: a batch is one message, so
  every order it records shares a single `Runtime::time` sample, while separate calls need not.
  Nothing else may differ, and the R2 property test must normalize timestamps rather than expect
  them to match.
- **R3 — Reservations accumulate within a batch, and a failed item consumes nothing.** Item `k` is
  validated against the balance left after the **successful** reservations among items `0..k`. An
  item that fails takes no funds and therefore does not doom its successors: with 10 free units, an
  item needing 11 fails while a following item needing 1 still succeeds. (A consequence of R2,
  called out because it is the behavior a client will test — and the one most likely to be
  mis-implemented as "stop at the first insufficient balance".)
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
- **R10 — Batch size is capped.** A batch of more than `MAX_BATCH_LEN` items fails the whole call
  with `BatchTooLarge { len; max }` under `RequestError`, before any item is processed, with `max`
  reporting the effective cap. For `replace_limit_orders` the cap applies to `cancel` and `create`
  **combined**. This requirement fixes the *behavior*, not the number: `MAX_BATCH_LEN` is
  **proposed at 100** (D4) and its final value is settled by R17's measurement, so R10 and R17
  cannot pull against each other.
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
  call; cancels remain permitted under halt, preserving the "users can always exit" guarantee. In
  replace, this check must run in **phase 1**, before any mutation — see Implementation — and it
  surfaces under the outer `TemporaryError` arm, not `RequestError` (R19).
- **R17 — Batching adds no super-linear cost, and the absolute fit is scoped to a documented state
  envelope.** No benchmark can establish an unconditional "a full batch always fits one message".
  `OrderBook::remove_order` costs `O(log p + k)` in the depth `k` of the order's price level
  (`OrderQueue::remove` scans it with `iter().position(..)`), or `O(pending)` when it falls through
  to the linear `pending_orders` scan — and neither `k` nor `pending` is bounded by caller input
  **or by the canister's own state**. At a large enough live depth even a *single* cancel can
  exceed the instruction budget; that is a pre-existing property of `cancel_limit_order`, not
  something batching introduces. What batching introduces is a multiplier of up to
  `MAX_BATCH_LEN`. The requirement therefore splits into three parts:
  - **Enforceable, and required.** A batch of `N` items costs `N ×` the equivalent single-item
    work plus `O(N)` batch overhead (cap check, outcome vector, result encoding): batching
    contributes no **super-linear** factor of its own. This is a *scaling* claim, not an exact
    inequality — a one-item batch legitimately costs slightly more than one single-order call. The
    `canbench` benchmarks therefore measure **several values of `N`** and check that growth stays
    linear, across both worst-case shapes: deep fragmented resting queues, and a large pending
    backlog with the targets at its far end.
  - **Conditional, and documented.** The absolute "fits one message" claim holds only inside a
    stated **state envelope** — the level depth and pending-backlog size the benchmark ran at.
    `MAX_BATCH_LEN` is chosen so the worse of the two shapes fits with margin at that envelope, and
    the envelope is recorded next to the number. A fixture is evidence for the envelope, never a
    proof for all depths.
  - **Failure behavior beyond the envelope.** Exceeding the instruction limit traps, and the
    replica discards the message's state changes — so an over-budget batch applies **nothing** and
    the caller retries with fewer items. The residual risk is a rejected call, never partial or
    corrupted state.

  Making the absolute claim unconditional requires **caller-bounded removal** — an `O(1)` position
  index for resting orders and for the pending queue — which would also retire the pre-existing
  single-cancel exposure. That is the principled fix; PR 2 chooses between delivering it and
  lowering the cap, on the measurement.
- **R18 — No new event types and no state migration.** A batch records **one existing
  `AddLimitOrderEvent` / `CancelLimitOrderEvent` per *successful* item**: a rejected item in a
  partial-success batch records nothing, so a three-item batch with one rejection writes two
  events, and a successful `replace_limit_orders` writes exactly `cancel.len() + create.len()` of
  them (a rejected replace writes none, per R6). Event-log replay, the state snapshot, and
  persisted state are untouched.
- **R19 — A replace's outer disposition mirrors its nested reason's.** When a replace is rejected
  over one item, the outer `kind` arm is the **same arm** the nested `reason` carries: an item that
  failed with a `TemporaryError` — today `TradingHalted`, which R16 explicitly routes through this
  path — surfaces as an outer `TemporaryError`, and one that failed with a `RequestError` as an
  outer `RequestError`. The outer arm *is* the client's retry contract (DEFI-2801 R2:
  `RequestError` means "do not auto-retry unchanged"), so flattening every rejection into
  `RequestError` would tell a client never to retry a halt that clears on its own.

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

### D4 — `MAX_BATCH_LEN` proposed at 100, matching the existing cap convention

`MAX_FILTER_LEN`, `MAX_ORDERS_PER_RESPONSE`, `MAX_TRADES_PER_RESPONSE`, and `MAX_HALT_BOOKS` are
all 100. A batch cap bounds the *number* of operations by the caller's input — though not their
unit cost, which for a cancel depends on book depth (see R17); 100 is well above a realistic
re-quote (a maker quoting both sides of a handful of pairs moves tens of orders) and consistent
with the rest of the surface. Treat it as the starting point the R17 benchmark must validate, not
a constant the design may assume.

### D5 — Replace reports the offending item by index, reusing the existing error types

Being single-level, the replace result has nowhere to hang a per-item error — but the caller still
needs to know what failed and where. Its rejection leaves therefore carry the index plus the
**existing** typed error as a nested `reason`, and are declared under **each outer arm the nested
error can itself carry** — so a `TradingHalted` create surfaces as an outer `TemporaryError`, not a
`RequestError` (R19). That keeps R12 at both levels (the nested `reason`
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
// Maximum items accepted by a batch endpoint (MAX_BATCH_LEN, proposed 100;
// the final value is settled by R17's measurement). For replace_limit_orders
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
        TemporaryError : opt variant {
            // `create[index]` was rejected for a transient reason — today only
            // TradingHalted. Same payload as the RequestError leaf; it sits
            // under this arm so the caller's retry contract stays correct
            // (R19). A CancelRejected leaf joins it here if
            // CancelLimitOrderError ever gains a temporary leaf — an additive,
            // non-breaking change.
            CreateRejected : record { index : nat32; reason : AddLimitOrderError };
        };
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

**Phase 1 — validate, read-only, against a projected free balance.** Two pieces of state advance
as the batch is planned, so neither may simply be read live. They are handled differently:

- **Free balance** — couples the cancels to the creates and accumulates across the creates. This
  one is **projected**, in a per-token overlay.
- **Each book's `next_seq`** — advances once per create. `State::validate_limit_order` assigns
  `OrderId::new(book_id, book.next_seq())`, and `OrderBook::add_pending_order` asserts
  `order.id() == self.next_seq` before incrementing. Validating two same-book creates against an
  unmutated book would therefore plan the **same** id twice and trip that assertion on the second
  apply.

  **Decision: id assignment is deferred to phase 2.** Phase 1 validates everything a create must
  satisfy (pair, halt, tick/lot, notional, amount bound, projected balance) but assigns no id;
  phase 2 draws each id from the live `book.next_seq()` as it records the create, exactly as the
  single-order path does. This keeps one source of truth for id assignment and removes the need to
  mirror `next_seq` in the overlay at all — so the projection stays purely a **free-balance**
  overlay. It does require splitting `validate_limit_order`'s validation from its id assignment;
  the single-order path keeps its current behavior by calling both.

Everything else create-validation touches (pair registered, tick/lot via
`OrderBook::validate_order`, notional via `check_notional`, the u256 amount bound) is independent
of both, so the overlay stays small rather than becoming a state fork.

**Phase 1 must also run the halt check itself.** `State::validate_limit_order` does **not** inspect
halt state — `add_limit_order` checks it separately, via
`permissions().permit_trading(owner, book_id)` *inside* the mutating block, immediately before
recording the event (`canister/src/lib.rs`). A replace whose phase 1 called only
`validate_limit_order` would therefore discover `TradingHalted` in phase 2, **after** the cancels
had already mutated and settled — violating R6 and R16. Phase 1 therefore runs `permit_trading`
for every create's book, in the same precedence as the single-order path, and phase 2 re-acquires
the permit it needs to emit each event. Cancels are unaffected: `permit_cancel` is allowed under
halt, which is exactly what preserves the exit guarantee.

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

For R14 to be **testable**, the kickoff must be observable: today the wrappers call
`ic_cdk_timers::set_timer` directly, which no unit test can count. Route it through the `Runtime`
abstraction (or a small injectable scheduler) so a test can assert the number of kickoffs; failing
that, assert it end-to-end in PocketIC by showing an unrelated pending order is *not* matched after
a cancel-only batch.

### Performance — `canister/src/benchmarks.rs`

A `canbench` benchmark per endpoint across **several batch sizes** — say 1, 10, 50, and the cap —
not a single cap-sized run: R17's linear-scaling claim needs more than one data point to test, and
one cap-sized measurement cannot tell you *which* lower cap is safe if 100 turns out to exceed the
budget. The binding case is `cancel_limit_orders`: cancellation settles inline, so a 100-item batch
is 100 book removals plus 100 settlements in one message. `OrderBook::remove_order` has **two** unbounded paths and each needs
its own fixture: a resting order costs `O(log p + k)` in its price level's depth, while a
still-pending order falls through to a linear `pending_orders.iter().position(..)` scan and costs
`O(pending)`. So benchmark a deep, fragmented resting book *and* a large pending backlog with the
targets at its far end, and set `MAX_BATCH_LEN` from the **worse** of the two (R17). **Record the
state envelope** each measurement was taken at — level depth and backlog size — alongside the
chosen cap; R17's absolute claim is scoped to it and is not a proof for all depths. Beyond the
envelope the message traps and the replica discards its state changes, so an over-budget batch
applies nothing and the caller retries smaller. If the margin at a realistic envelope is
uncomfortable, add the `O(1)` removal paths rather than relaxing R17. Note the repo's benchmark CI gate fails on **any** delta against
the committed `canbench_results.yml`, so new benchmarks must be persisted with `just bench-check`.

### Docs — `docs/src/development/design.md`

Add each endpoint to *Main Endpoints* with its cost, and document the partial-success vs atomic
split (D1). Note that `design.md` currently lists "Batch operations" under *Potential Additional
Features* and describes them as placing or cancelling "multiple orders **atomically** in a single
call" — wrong for `add_limit_orders` / `cancel_limit_orders` under D1 — so that bullet must go as
soon as the **first** endpoint ships, not at the end of the stack.

**Each PR updates the doc for the endpoint it introduces** (see Delivery). A PR that ships a public
endpoint must not leave `design.md` calling it hypothetical or mis-stating its atomicity; PR 3
additionally writes the combined overview once all three exist.

### Test plan

Unit (`*/tests.rs`, fixtures in `canister/src/test_fixtures`):

- Equivalence (R2, R5): a property test asserting `add_limit_orders(reqs)` leaves state and the
  event log identical to the same requests placed one at a time, **normalizing timestamps** (a
  batch shares one `Runtime::time` sample; separate calls need not); likewise for cancel.
- Matching kickoff (R14): assert the **number of kickoffs**, which no state or event-log
  assertion can see. A mixed-success `add_limit_orders` and a successful `replace_limit_orders`
  each schedule exactly **one**; an empty batch, an `add_limit_orders` whose every item failed, an
  empty replace, and **every** `cancel_limit_orders` call schedule **none**. Without this the
  wrappers can regress R14 with every other test still green.
- Same-book sequence assignment (R2, and the deferred id assignment): a batch — and a replace —
  with several creates on the **same** book assigns distinct, consecutive `OrderId`s and applies
  without tripping `add_pending_order`'s seq assertion.
- Cumulative reservations (R3): a batch whose later items overdraw — earlier items stand and the
  overdrawing item reports `InsufficientBalance`, **while a smaller following item still
  succeeds**, since a failed item reserves nothing and must not poison its successors.
- Cap and empty (R10, R11): `MAX_BATCH_LEN + 1` items ⇒ `BatchTooLarge` with `max` equal to the
  effective cap, no state change; 0 items ⇒ `Ok([])` and no event recorded.
- Replace atomicity (R6, R8): a replace whose last create is invalid returns `CreateRejected` with
  the right index — no trap — and leaves the book, balances, **and** event log untouched. Assert
  each with an instrument that actually covers it: `StateSnapshot::from_state` snapshots the
  **book only** — it deliberately skips user balances and `order_history` (both stable-memory,
  surviving upgrades on their own) and carries no event log — so a snapshot round-trip alone would
  silently verify one third of this claim. Compare the affected free/reserved balances directly and
  assert `storage::total_event_count()` is unchanged, **alongside** the snapshot comparison.
- Replace self-funding (R7): a maker whose stored free balance cannot cover the creates, but whose
  cancels release enough, succeeds. Run it with **zero** spare free balance so the creates are
  funded *entirely* by the cancels' released reservations — that is the case that fails if phase 2
  records a create before the cancel's `Unreserve` has been settled, and it should be asserted on
  the resulting `free` / `reserved` split, not just on the `Ok`.
- Duplicate cancel id (R9) and per-item duplicate (R5): the batch path returns
  `OrderAlreadyTerminal` on the second occurrence. For the atomic path, the case that actually pins
  R9's precedence is an earlier cancel id that is **independently invalid** (unknown, not owned, or
  terminal) *together with* a later duplicate — assert `DuplicateOrderId` at the second
  occurrence's index. A duplicate-only fixture would pass a sequential implementation that wrongly
  returns the earlier `CancelRejected`.
- Envelope forward-compat (R12, R13): encode a batch reply carrying an unknown future leaf and
  decode it against the current types — the unknown leaf softens to `None` in its own item while a
  known leaf in a sibling item still decodes typed, and `message` survives at both levels. This
  extends the existing `should_decode_future_leaf_as_none_keeping_arm_and_message` test to the
  `vec` and nested-`reason` positions.

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Round trip of all three endpoints against a live canister, including per-item error reporting.
- Halt behavior (R16, R19): a halted pair fails only its own items in a batch, but fails a whole
  replace — and that replace rejection arrives under the outer **`TemporaryError`** arm carrying
  `CreateRejected`, not `RequestError`, so a client following the disposition contract retries once
  the halt clears.
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
  - *Acceptance:* R1, R2, R3, R10, R11, R12, R13, R14, R15, R16 (batch half), R18 — **plus its
    `design.md` entry**: document `add_limit_orders` and its partial-success contract, and drop the
    stale "Batch operations" bullet from *Potential Additional Features*, which wrongly calls batch
    place/cancel atomic.
- **PR 2 (2/3) — `cancel_limit_orders`.** The same shape applied to cancellation, reusing PR 1's
  scaffolding, plus the two worst-case inline-settlement benchmarks that make R17 checkable, and
  the decision they force: record the state envelope and set the cap, or deliver the `O(1)`
  removal index if the margin is inadequate.
  - *Acceptance:* R4, R5, R17 — **plus the cross-cutting requirements as they apply to this
    endpoint**: R10, R11 (cap, empty no-op), R12, R13 (envelope, frozen two-arm outcome), R14
    (this endpoint must *not* arm matching), R15 (trading accounts), R18 (no new events). PR 1
    satisfies those only for `add_limit_orders`; they are re-verified here. Plus its `design.md`
    entry for `cancel_limit_orders`.
- **PR 3 (3/3) — `replace_limit_orders`.** The phase-1 free-balance projection with **id
  assignment deferred to phase 2**, the validate-then-apply split, the phase-1 halt check, the
  split of `validate_limit_order` into validation (balance read through the projection) and id
  assignment, and the atomic endpoint.
  - *Acceptance:* R6, R7, R8, R9, R16 (replace half), R19 — **plus the cross-cutting requirements for
    this endpoint**: R10, R11 (combined cap, empty no-op), R12 (including the nested `reason`
    envelope), R13, R14 (single conditional kickoff), R15, R18 — and its `design.md` entry for
    `replace_limit_orders`, including the combined batch overview now that all three exist. PR 1
    cannot satisfy these for an endpoint that does not exist until here.

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
