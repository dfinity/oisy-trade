---
id: DEFI-2737
title: Batch order cancellation and placement
tags: [orders, candid, errors, market-making]
---

# Batch order cancellation and placement — Design

Companion to [`requirements.md`](requirements.md). Criteria are cited as `Req N` / `Req N.M` and are
not restated here.

## Overview

Both endpoints are loops over the **existing** single-order entry point, with the per-call preamble
hoisted out of the loop and — for placement only — one Matching_Kickoff after it. Writing them this
way is what makes the sequential-equivalence contract (Req 1.3, Req 2.3) true by construction rather
than by test, and it is why the ticket needs no new state, no new event type, and no second copy of
order validation.

There is consequently very little design here. What there *is* consists of one interface decision
(the two-level result shape, D1), one forward-compatibility constraint that must be got right the
first time because it cannot be fixed later (D2), and one measurement that fixes a number which can
never afterwards be lowered (D3). Everything else is a loop.

Delivery is two stacked PRs: cancellation, then placement.

## Constraints

- **Fully synchronous trading.** No inter-canister calls and no `await` on any of these paths, so a
  Batch cannot interleave with another message part-way through.
- **Event-sourced.** Every mutation flows through `state::audit::process_event` and is re-applied on
  replay. These endpoints emit the existing events `N` times (Req 11).
- **Cancellation settles inline.** `State::cancel_limit_order` drains the settling events it produced
  within the same message (`pending_settling_events.split_off(..)`), so a batch cancellation costs
  `N` cancellations *plus* `N` inline settlements. This is what makes cancellation, not placement,
  the binding case for Req 10 — and it is why cancellation ships first (D4).
- **`OrderBook::remove_order` has two unbounded paths.** A resting order costs `O(log p + k)` in the
  depth `k` of its price level (`OrderQueue::remove` scans it with `iter().position(..)`); a
  still-pending order falls through to a linear `pending_orders` scan and costs `O(pending)`. Neither
  `k` nor `pending` is bounded by Caller input or by the Canister's own state — this pre-existing
  property is what forces Req 10.5's envelope scoping. At a large enough live depth even a *single*
  cancellation can exceed the budget; what batching adds is a multiplier of up to MAX_BATCH_LEN.
- **`check_candid_interface_compatibility`** pins `oisy_trade.did` to the generated interface; the
  new endpoints must be added there and the equality check must pass.
- **Back-compat is required** — the canister is live.

## Design Decisions

### D1 — Two-level results, partial success, no atomicity

Both endpoints adopt the two-level shape `Result<Vec<Result<T, ItemError>>, BatchError>` that
**DEFI-2801 D8 already reserved for exactly this endpoint** ("No write batch endpoint exists yet; a
future `add_limit_orders` would adopt the two-level shape"). Partial success is the right default for
a write batch: re-issuing a half-applied atomic batch risks double-placing the items that already
succeeded, so the Caller must be told precisely which went through — hence Req 1.1 / Req 2.1's
positional alignment, which is what lets a client zip its request list against the response.

This is why neither endpoint is atomic, and why the batch-level error is reserved for conditions that
are true of the whole call before any item is looked at (Req 3.1, Req 4).

### D2 — The per-item variant never gains a third arm

The Error_Envelope's forward compatibility comes from its inner `opt variant`: an old client decodes
an unknown future leaf as `null` while still reading the arm and `message`. **This property is
depth-independent** — the softening is applied per `opt` value — which is what makes Req 5.4 hold
identically for an error nested inside a `vec`, and Req 5.5 hold for its siblings. This was verified
empirically against the `candid` crate rather than assumed.

The exception is the *arm* set. Adding a third arm to `variant { Ok; Err }` — say a `Skipped` for an
exhausted instruction budget — is breaking, and breaks in the worst way: an old client keeps decoding
fine until the first time the canister actually puts `Skipped` on the wire. It is a latent break that
surfaces in production, not at deploy and not in CI. Hence Req 6. For the "skipped" example a leaf is
also the semantically better answer, since it belongs under `TemporaryError`: the Caller should retry.

This is the one decision in the ticket that cannot be corrected in a later release.

### D3 — MAX_BATCH_LEN starts at 100, matching the existing cap convention

`MAX_FILTER_LEN`, `MAX_ORDERS_PER_RESPONSE`, `MAX_TRADES_PER_RESPONSE`, and `MAX_HALT_BOOKS` are all
100. A cap of 100 is well above a realistic re-quote — a maker quoting both sides of a handful of
pairs moves tens of orders — and consistent with the rest of the surface.

**100 is the starting point the Req 10.4 measurement must validate, not a constant the design may
assume.** Because Req 3.5 forbids lowering it after publication, the measurement lands in PR 1.

### D4 — Cancellation ships before placement

The cap becomes a public contract the moment the *first* endpoint ships, and it can never afterwards
be lowered (Req 3.5). It must therefore be fixed by measuring the endpoint whose per-item cost binds
it — which is cancellation, because cancellation settles inline (see Constraints) while placement
merely enqueues.

Shipping cancellation first makes the PR that sets the cap the same PR that ships the endpoint the
cap is derived from. The alternative order would have PR 1 fixing a public cap from a benchmark of an
endpoint it does not yet ship, on the strength of a cost model rather than the delivered path — a
number that is expensive to be wrong about and impossible to walk back.

### D5 — No new events

A Batch is just the existing events repeated (Req 11). No new event *type* appears, so nothing about
the event log's shape, replay, or the snapshot changes, and this ticket carries no persistence risk
and no migration — worth stating explicitly given the canister is live.

## Implementation

### Public types & Candid — `libs/types/src/lib.rs`, `libs/types/src/error/mod.rs`, `canister/oisy_trade.did`

```candid
type CancelLimitOrdersError = record {
    kind : variant {
        RequestError : opt variant {
            BatchTooLarge : record { len : nat32; max : nat32 };
        };
        TemporaryError : opt variant {};
        InternalError : opt variant {};
    };
    message : opt text;
};

type CancelLimitOrderOutcome = variant { Ok : OrderRecord; Err : CancelLimitOrderError };

type AddLimitOrdersError = record { /* as CancelLimitOrdersError */ };
type AddLimitOrderOutcome = variant { Ok : OrderId; Err : AddLimitOrderError };

service : {
    cancel_limit_orders : (vec OrderId)
        -> (variant { Ok : vec CancelLimitOrderOutcome; Err : CancelLimitOrdersError });
    add_limit_orders : (vec LimitOrderRequest)
        -> (variant { Ok : vec AddLimitOrderOutcome; Err : AddLimitOrdersError });
}
```

`CancelLimitOrdersError` / `AddLimitOrdersError` are instantiations of the existing generic
`Error<Request, Temporary, Internal>` (DEFI-2801 D7), so the envelope shape cannot drift (Req 5.1).

### The batch loop — `canister/src/lib.rs`

Both endpoints share one shape:

- Check the cap once (Req 3.1) and return early for an empty Batch (Req 4).
- Hoist the per-call preamble out of the loop: `assert_caller_is_allowed` and the `effective_account`
  resolution run once, not per item (Req 8.1).
- For each item, call the existing per-order logic and collect `Ok` / `Err` into the outcome vector.
  An item's failure does not abort the loop (Req 1.6, Req 2.5).

Req 2.4 and Req 2.6 need no special handling: each item is applied before the next is validated, so
the balance each item sees already reflects its predecessors. This is the single most likely
behaviour to be mis-implemented as "stop at the first insufficient balance", and Req 2.6 is the
criterion that catches it.

Req 1.5 likewise needs no special handling — the first occurrence terminates the order, so the second
fails on the ordinary non-terminal check.

### Entry points — `canister/src/main.rs`

Two new `#[ic_cdk::update]` wrappers mirroring the existing ones' logging convention (log successes,
do not log user-caused errors). `add_limit_orders` arms the zero-delay `drive_matching` timer once
after the Batch and only if at least one order was enqueued (Req 7.1, Req 7.2);
`TODO DEFI-2823` (timer coalescing) applies here too and will subsume the kickoff.
`cancel_limit_orders` does not arm it (Req 7.3).

**Req 7 needs an observation seam.** Today the wrappers call `ic_cdk_timers::set_timer` directly,
which no unit test can count, so nothing would catch a regression that armed the timer per item or
armed it from the cancellation path. Route the kickoff through the `Runtime` abstraction (or a small
injectable scheduler) so a test can assert the *number* of kickoffs. Introducing that seam is part of
PR 1, since PR 1 must prove Req 7.3.

### Benchmarks — `canister/src/benchmarks.rs`, `canister/canbench_results.yml`

One `#[bench]` entry point per endpoint per batch size, plus the two worst-case book fixtures the
cancellation path needs. Benches differing only by a size parameter share a body and keep distinct
entry points, since canbench reports one result per function. The recorded State_Envelope (Req 10.3)
is committed as a comment beside the MAX_BATCH_LEN constant, not left in a PR description. What each
benchmark measures and why is in the Test plan.

### Rust client — `libs/client/src/lib.rs`

`OisyTradeClient` mirrors the public surface and is what the PocketIC harness drives
(`integration_tests/src/lib.rs` builds one per test). Each endpoint needs a corresponding client
method **in the PR that introduces it**, or the supported client surface lags the canister and the
integration round trips have nothing to call.

### Docs — `docs/src/development/design.md`

Add each endpoint to *Main Endpoints* with its cost, and document the partial-success contract (D1).
`design.md` currently lists "Batch operations" under *Potential Additional Features* and describes
them as cancelling or placing "multiple orders **atomically** in a single call" — wrong under D1 — so
that bullet must go as soon as the **first** endpoint ships, not at the end of the stack. Each PR
updates the doc for the endpoint it introduces.

## Test plan

Unit (`*/tests.rs`, fixtures in `canister/src/test_fixtures`):

- **Sequential equivalence (Req 1.3, Req 2.3, Req 1.4).** A property test asserting a Batch leaves
  state and the event log identical to the same requests issued one at a time, **and** that the
  returned outcome vector matches the sequential results **positionally** — id for id, error for
  error (Req 1.1, Req 2.1). State equality alone would pass a Batch that returned the right orders in
  the wrong order or paired an item with the wrong error, which is exactly what a Caller relies on.
  Normalize timestamps throughout; the `OrderRecord`s the cancellation path returns carry their own
  `created_at` / `last_updated_at`.
- **Matching kickoff (Req 7).** Assert the **number** of kickoffs through the seam introduced above —
  no state or event-log assertion can see it. Exactly one: a mixed-success `add_limit_orders`. None:
  an empty Batch, an `add_limit_orders` whose every item failed, an over-cap rejection, and every
  `cancel_limit_orders` call. Without this the wrappers can regress Req 7 with every other test green.
- **Cumulative reservations (Req 2.4, Req 2.6).** A Batch whose later items overdraw: earlier items
  stand, the overdrawing item reports `InsufficientBalance`, **and a smaller following item still
  succeeds**. The last clause is the whole point; without it the test passes a stop-at-first-failure
  implementation.
- **Repeated id (Req 1.5).** The same id twice in one call: cancelled at the first occurrence,
  `OrderAlreadyTerminal` at the second, one Operation_Event recorded, not two.
- **Same-book sequence assignment (Req 2.3).** A Batch with several creations on the **same** book
  assigns distinct, consecutive `OrderId`s and applies without tripping `add_pending_order`'s seq
  assertion.
- **Cap boundaries (Req 3.1, Req 3.2, Req 3.3).** `MAX_BATCH_LEN + 1` items ⇒ `BatchTooLarge` with
  `max` equal to the effective cap and no state change; exactly `MAX_BATCH_LEN` ⇒ processed normally.
  The inclusive-boundary case is the one an off-by-one gets wrong and the over-cap case alone does not
  catch.
- **Empty (Req 4).** 0 items ⇒ `Ok([])`, no event, no balance movement.
- **Envelope forward compatibility (Req 5.4, Req 5.5, Req 6.1).** Encode a batch reply carrying an
  unknown future leaf and decode it against the current types: the unknown leaf softens to `None` in
  its own item while a known leaf in a **sibling item** still decodes typed, and `message` survives at
  both levels. This extends the existing `should_decode_future_leaf_as_none_keeping_arm_and_message`
  test to the `vec` position.

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Round trip of both endpoints against a live canister, including per-item error reporting.
- **Halt (Req 9).** A halted pair fails only its own items in an `add_limit_orders` Batch while items
  on other pairs succeed in the same call, and the failure arrives under the `TemporaryError` arm
  (Req 9.4). A `cancel_limit_orders` Batch succeeds against the halted pair (Req 9.3).
- **Trading accounts (Req 8).** A trading account batches on its funding account's behalf;
  `placed_by` / `canceled_by` are recorded per order.

Benchmarks (`canbench`):

- One measurement per endpoint at **several batch sizes** — 1, 10, 50, and the cap — not a single
  cap-sized run. Req 10.1's scaling claim needs more than one data point to test, and one cap-sized
  measurement cannot tell you *which* lower cap is safe if 100 turns out to exceed the budget.
- The binding case is `cancel_limit_orders`. `OrderBook::remove_order`'s two unbounded paths each need
  their own fixture — a deep, fragmented resting book, **and** a large pending backlog with the
  targets at its far end — and MAX_BATCH_LEN is set from the **worse** of the two (Req 10.4).
- Multiplying one measured `cancel_limit_order` by MAX_BATCH_LEN is **not** a sound bound: Req 10.1's
  model is `N ×` single-item work *plus* a positive overhead term, so the scalar product omits real
  cost. Benchmark cap-many cancellations **within a single message**.
- **Record the State_Envelope** each measurement was taken at, alongside the chosen cap (Req 10.3).
- If the margin at a realistic envelope is uncomfortable, the remedy is bounded removal, not a relaxed
  Req 10. Note an index alone is insufficient: price levels and `pending_orders` are both `VecDeque`s
  and `VecDeque::remove(pos)` shifts elements, so removal stays `O(k)` even when the position is
  known. The real fix is a FIFO structure supporting constant-time arbitrary deletion — an intrusive
  doubly-linked list over a slab, or tombstoning with lazy compaction — applied to both `OrderQueue`'s
  levels and `pending_orders`. That would also retire the pre-existing single-cancel exposure.
- The repo's benchmark CI gate fails on **any** delta against the committed `canbench_results.yml`, so
  new benchmarks must be persisted with `just bench-check`.

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
just bench-check
```

## Delivery / PR sequence

Each PR is independently compilable, testable, and useful on its own.

- **PR 1 (1/2) — `cancel_limit_orders`.** The endpoint plus the shared batch scaffolding:
  MAX_BATCH_LEN, the batch-level error type, the per-item outcome shape, the hoisted preamble, and the
  `Runtime` kickoff seam Req 7.3 needs to be assertable. **The cap is settled here** (D4, Req 3.4):
  this PR carries the two worst-case inline-settlement benchmarks across several batch sizes, fixes
  MAX_BATCH_LEN from the worse of them with a recorded margin, and records the State_Envelope beside
  it. From this PR onward the cap may be raised but never lowered.
  - *Acceptance:* Req 1, Req 3, Req 4 (4.1, 4.3), Req 5 (5.1, 5.2, 5.4, 5.5), Req 6, Req 7 (7.3, 7.4,
    7.5 — this endpoint must *not* arm matching), Req 8 (8.1, 8.3, 8.4), Req 9 (9.3), Req 10, Req 11
    — plus its `design.md` entry (document `cancel_limit_orders` and its partial-success contract,
    and drop the stale "Batch operations" bullet from *Potential Additional Features*) and its
    `OisyTradeClient::cancel_limit_orders` method, without which the integration round trips cannot
    run.
- **PR 2 (2/2) — `add_limit_orders`.** The same shape applied to placement, reusing PR 1's
  scaffolding, plus the placement-path scaling curve. These benchmarks **confirm** the cap PR 1 fixed;
  they may raise it but never lower it (Req 3.5). If placement turns out to want a different cap, the
  answer is one shared cap set by the binding endpoint, not a per-endpoint cap.
  - *Acceptance:* Req 2, Req 7 (7.1, 7.2) — **plus the cross-cutting requirements as they apply to
    this endpoint**: Req 3, Req 4 (4.2, 4.3), Req 5 (5.1, 5.3, 5.4, 5.5), Req 6, Req 8 (8.1, 8.2,
    8.4), Req 9 (9.1, 9.2, 9.4), Req 10, Req 11. PR 1 satisfies those only for `cancel_limit_orders`;
    they are re-verified here. Plus its `design.md` entry and its `OisyTradeClient::add_limit_orders`
    method.

## Discussed Alternatives

- **Shipping `replace_limit_orders` in this ticket.** It is the endpoint that would actually close a
  maker's exposure window, and it was specified in an earlier revision of this spec. Dropped to keep
  the ticket small: it needs a validate-then-apply split, a projected free balance so the creations
  can be funded by the cancellations' released Reservations, ordering rules about when a cancellation
  is settled relative to the first creation, and a single-level rejection type naming the offending
  leg and index — none of which the two endpoints here require. The two endpoints are useful on their
  own and are the scaffolding replacement would build on, so nothing here has to be undone to add it.
  See Non-goals for the accepted consequence.
- **Making either endpoint atomic.** Consistent and simpler to describe, but wrong for a throughput
  endpoint: one stale order id would throw away 99 good cancellations, and one bad tick size 99 good
  orders. It also contradicts DEFI-2801 D8. Atomicity earns its cost only where a partial outcome is
  actively harmful, which is replacement — the endpoint this ticket defers.
- **Shipping placement before cancellation.** The natural reading order, and placement is the simpler
  endpoint. Rejected on D4: it would have PR 1 fix a permanently-unlowerable public cap from a
  benchmark of a path it does not ship.
- **A per-endpoint MAX_BATCH_LEN.** Lets placement carry a larger cap than cancellation, since
  placement is cheaper per item. Rejected as surface area a client must track for no real gain: the
  cap exists to bound one message's work, the binding endpoint sets it, and two numbers doing one job
  is two numbers to get wrong.
- **A per-item `Skipped` outcome for budget exhaustion.** Tempting for Req 10, but adding an arm to
  the per-item variant is a latent breaking change (D2) — and, more decisively, the outcome could
  never be *delivered*: instruction exhaustion **traps** and the replica rolls the call back, so there
  is no response in which to report that an item was skipped. The cap does not make exhaustion
  unreachable (Req 10.6), it only bounds when it happens. If a per-item transient outcome is ever
  genuinely needed, it arrives as a `TemporaryError` leaf, never a new arm.
- **A "cancel by pair" / "cancel all" selector instead of an id list.** Genuinely useful as a panic
  button, but its cost is unbounded by the Caller's input, so it needs chunking or its own cap and a
  story for what happens when it cannot finish in one message. Out of scope; see Non-goals.
- **One combined `batch(vec Operation)` endpoint** with an operation variant covering cancel and
  create. More extensible, but it makes the result shape depend on the contents of the vector rather
  than on which endpoint was called, and it would force a single ordering semantics across mixed
  operations. Two explicit endpoints keep each contract legible.
