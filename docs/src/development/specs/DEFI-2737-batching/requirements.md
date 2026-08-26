---
id: DEFI-2737
title: Batch order cancellation and placement
tags: [orders, candid, errors, market-making]
---

# Batch order cancellation and placement — Requirements

## Introduction

Every order today costs one update call. A market maker quoting both sides of several pairs
re-quotes continuously, so it pays a round trip per order it moves. The ask came from the field —
G20 asked for "batch place order and batch cancel orders" — and the same primitives are standard on
Binance, Kraken, and Coinbase precisely because they are what a maker's re-quote loop is built from.

This ticket adds two endpoints:

- **`cancel_limit_orders`** — cancel many orders in one call, each succeeding or failing on its own.
- **`add_limit_orders`** — place many orders in one call, same per-item behaviour.

Both are throughput conveniences, and their whole contract is that a Batch is indistinguishable
from the same single-order calls issued in sequence. There is no second semantics to learn, no new
event, and no new state. Cancellation ships first because its per-item cost is the one that binds
the size cap (Requirement 10).

Atomic replacement — cancelling a set and creating a set as one all-or-nothing unit — is the
primitive that would actually close a maker's exposure window, but it needs machinery neither of
these endpoints does. It is deferred; see Non-goals.

## Glossary

- **Canister**: The OISY TRADE ICP canister implementing the order-book exchange.
- **Caller**: The IC principal invoking a Candid endpoint. May be a funding account or a registered
  trading account.
- **Batch**: The vector of items processed by one call to a Batch_Endpoint.
- **Batch_Endpoint**: `cancel_limit_orders` or `add_limit_orders`.
- **MAX_BATCH_LEN**: The cap on the number of items in a Batch. Its value is not fixed by this
  document; Requirement 3 fixes the behaviour and the conditions under which the value may change.
- **Error_Envelope**: The existing three-arm disposition record
  `{ kind : variant { RequestError : opt variant {…}; TemporaryError : opt variant {…};
  InternalError : opt variant {…} }; message : opt text }` introduced by DEFI-2801. `RequestError`
  means "do not auto-retry unchanged", `TemporaryError` means "retry after backoff", and
  `InternalError` means "surface to operators".
- **Per_Item_Outcome**: The two-arm variant `variant { Ok : T; Err : E }` returned for each item of
  a Batch.
- **`AddLimitOrderError`** / **`CancelLimitOrderError`**: The existing per-item error types returned
  by `add_limit_order` and `cancel_limit_order`.
- **Reservation**: The amount of a token moved from `free` to `reserved` while an order is open,
  and released back into `free` when the order is cancelled or filled.
- **Operation_Event**: An `AddLimitOrderEvent` or `CancelLimitOrderEvent` recorded in the event log.
- **Settling_Event**: The balance-movement event a cancellation produces to release its Reservation.
- **Matching_Kickoff**: The arming of the zero-delay `drive_matching` timer that asks the matching
  engine to run.
- **Trading_Halt**: The per-book flag controllers set to block new order placement on a trading
  pair while still permitting cancellation.
- **Instruction_Limit**: The Internet Computer's per-message instruction budget. Exceeding it traps
  and the replica discards the message's state changes.
- **State_Envelope**: The order-book state a cost measurement was taken at — specifically the depth
  of the price levels involved and the size of the pending-order backlog. Neither is bounded by
  Caller input or by the Canister's own state.

## Non-goals

- **Atomic replacement (`replace_limit_orders`).** Cancelling a set and creating a set as one unit
  is the primitive that removes a maker's exposure window, and it remains the most valuable thing in
  this area — but it needs a validate-then-apply split, a projected balance so the creations can be
  funded by the cancellations' released Reservations, and a rejection type that names the offending
  leg and index. None of that is needed by the two endpoints here. Deferred deliberately to keep
  this ticket small; the two endpoints below are useful on their own and are the scaffolding
  replacement would build on. **Accepted:** until it exists, a maker moving a quote still issues a
  cancellation call and a placement call, and is exposed between them.
- **Modify / amend an existing order in place.** An in-place amend has to answer whether a re-price
  keeps its FIFO queue position, which is a matching-engine policy question, not a batching one.
- **A "cancel all" / "cancel by pair" selector.** The standard CEX panic button. Its cost is not
  bounded by the Caller's input, so it needs its own chunking story, and a maker cancelling its own
  book already tracks its ids. Worth a follow-up on its own terms.
- **Atomicity for either endpoint.** Per-item partial success is the more useful behaviour and the
  one DEFI-2801 prescribes: one stale order id should not throw away 99 good cancellations, and one
  bad tick size should not throw away 99 good orders. Re-issuing a half-applied atomic batch would
  risk double-placing the items that already succeeded.
- **Batching across users, or batching deposits / withdrawals.** Deposits and withdrawals make
  inter-canister calls and carry a per-(user, token) in-flight guard; batching them is a different
  problem.
- **An unconditional "a full Batch always fits one message" guarantee.** Requirement 10 scopes that
  claim to a recorded State_Envelope, because the per-item cost of a cancellation is not bounded by
  Caller input. Making it unconditional is separate work on the order book's removal cost.

## Requirements

### Requirement 1: Batch Cancellation Behaves as a Sequence of Single Cancellations

**User Story:** As a market maker pulling quotes, I want `cancel_limit_orders` to behave exactly as
if I had issued `cancel_limit_order` once per id, so that a Batch needs no reconciliation logic I
do not already have.

#### Acceptance Criteria

1. WHEN `cancel_limit_orders` is called with a `vec OrderId`, THE Canister SHALL return a `vec` of
   Per_Item_Outcome of the same length, positionally aligned with the request.
2. THE Canister SHALL set each Per_Item_Outcome to `Ok` carrying the cancelled `OrderRecord`, or to
   `Err` carrying exactly the `CancelLimitOrderError` that `cancel_limit_order` would have returned
   for that id.
3. WHEN `cancel_limit_orders` is called with ids `[a, b, c]`, THE Canister SHALL leave state and the
   event log identical to `cancel_limit_order(a)`, `cancel_limit_order(b)`, `cancel_limit_order(c)`
   issued in that order by the same Caller — the same Operation_Events recorded, the same
   Reservations released, and the same per-item errors returned.
4. THE Canister SHALL record every cancellation in one call with the same timestamp, because a
   Batch is one message; the equivalence of 1.3 SHALL therefore be read modulo timestamps and
   modulo inter-message interleaving, and modulo nothing else.
5. WHEN the same `OrderId` appears more than once in the request, THE Canister SHALL cancel it at
   its first occurrence and SHALL return `OrderAlreadyTerminal` at every later occurrence, without
   special-casing the repeat.
6. WHEN an item fails, THE Canister SHALL leave that order's state unchanged and SHALL continue
   processing the remaining items.

### Requirement 2: Batch Placement Behaves as a Sequence of Single Placements

**User Story:** As a market maker posting quotes, I want `add_limit_orders` to behave exactly as if
I had issued `add_limit_order` once per item, so that I do not have to learn or reconcile a second
placement semantics.

#### Acceptance Criteria

1. WHEN `add_limit_orders` is called with a `vec LimitOrderRequest`, THE Canister SHALL return a
   `vec` of Per_Item_Outcome of the same length, positionally aligned with the request.
2. THE Canister SHALL set each Per_Item_Outcome to `Ok` carrying the assigned `OrderId`, or to `Err`
   carrying exactly the `AddLimitOrderError` that `add_limit_order` would have returned for that
   request.
3. WHEN `add_limit_orders` is called with items `[a, b, c]`, THE Canister SHALL leave state and the
   event log identical to `add_limit_order(a)`, `add_limit_order(b)`, `add_limit_order(c)` issued in
   that order by the same Caller — the same Operation_Events recorded, the same `OrderId`s assigned
   in the same sequence, the same Reservations taken, and the same per-item errors returned — modulo
   timestamps as in 1.4.
4. WHEN the Canister validates item `k`, THE Canister SHALL validate it against the balance
   remaining after the Reservations taken by the **successful** items among `0..k`.
5. WHEN an item fails, THE Canister SHALL take no Reservation for it and SHALL continue processing
   the remaining items.
6. WHEN an item fails because the remaining balance does not cover it, THE Canister SHALL still
   accept a following item that the remaining balance does cover, because a failed item reserves
   nothing and must not doom its successors.

### Requirement 3: Batch Size Is Capped

**User Story:** As an operator, I want the number of items in a Batch capped, so that no single
Caller can consume an unbounded amount of computation in one message.

#### Acceptance Criteria

1. WHEN a Batch contains more than MAX_BATCH_LEN items, THE Canister SHALL reject the entire call,
   before processing any item, with `BatchTooLarge { len; max }` under the Error_Envelope's
   `RequestError` arm, where `len` is the submitted count and `max` is the effective cap.
2. WHEN a Batch contains exactly MAX_BATCH_LEN items, THE Canister SHALL accept and process it
   normally, because the cap is inclusive.
3. THE Canister SHALL enforce a MAX_BATCH_LEN of at least 1, so that a cap of zero — which would
   reject every Batch unconditionally — cannot be configured.
4. THE Canister SHALL fix the value of MAX_BATCH_LEN from a recorded cost measurement (10.4) before
   the first Batch_Endpoint is published, because from that moment the cap is part of a public
   contract.
5. THE Canister SHALL NOT lower MAX_BATCH_LEN once a Batch_Endpoint has been published, because
   doing so would reject requests that previously succeeded. It may be raised.
6. IF MAX_BATCH_LEN is raised, THEN THE Canister SHALL NOT apply the higher cap until a cost
   measurement covering the new value has been recorded against the State_Envelope in force at the
   time of the increase.

### Requirement 4: An Empty Batch Is a Successful No-Op

**User Story:** As a client developer whose re-quote loop computed nothing to do this tick, I want
an empty Batch accepted rather than refused, so that I do not have to special-case an empty slice.

#### Acceptance Criteria

1. WHEN `cancel_limit_orders` is called with an empty vector, THE Canister SHALL return `Ok([])`.
2. WHEN `add_limit_orders` is called with an empty vector, THE Canister SHALL return `Ok([])`.
3. WHEN a Batch is empty, THE Canister SHALL cancel no order, create no order, move no balance, and
   record no event.

### Requirement 5: The Error Envelope Is Preserved at Every Level

**User Story:** As a client developer, I want both the batch-level and the per-item errors to use
the same three-arm disposition envelope, so that a future leaf added at either level decodes as
absent on my old client instead of breaking it, and so that the retry advice I act on stays correct.

#### Acceptance Criteria

1. THE Canister SHALL encode every batch-level error using the Error_Envelope shape, with every arm
   carrying its own optional payload.
2. THE Canister SHALL encode per-item errors in `cancel_limit_orders` as the existing
   `CancelLimitOrderError` verbatim, adding no new leaf to it.
3. THE Canister SHALL encode per-item errors in `add_limit_orders` as the existing
   `AddLimitOrderError` verbatim, adding no new leaf to it.
4. WHEN a client decodes a response carrying a leaf it does not know — at batch level or at per-item
   level inside the result vector — THE Canister's encoded response SHALL be structured so that the
   unknown leaf decodes as absent, the `kind` arm remains readable as one of `RequestError` /
   `TemporaryError` / `InternalError`, and `message` remains readable.
5. WHEN one item of a Batch carries a leaf the client does not know, THE Canister's encoded response
   SHALL leave the outcomes at every other index in the same vector fully decodable.

### Requirement 6: The Per-Item Outcome Variant Is Frozen at Two Arms

**User Story:** As a client developer, I want the per-item outcome shape never to gain a third arm,
so that my decoder does not break silently in production the first time the Canister puts a new arm
on the wire.

#### Acceptance Criteria

1. THE Canister SHALL represent each Per_Item_Outcome as a variant with exactly the two arms `Ok`
   and `Err`, in the published Candid interface, for both Batch_Endpoints, in every deployed
   interface version.
2. THE Canister SHALL NOT add to the Per_Item_Outcome variant any arm whose identifier is neither
   `Ok` nor `Err`.
3. IF a future per-item outcome is neither a success expressible as `Ok` nor an already-defined
   error leaf expressible as `Err`, THEN THE Canister SHALL add it as a new leaf under one of the
   Error_Envelope's disposition arms, not as a new arm of the Per_Item_Outcome variant.

### Requirement 7: One Matching Kickoff per Batch, and Only When Orders Were Enqueued

**User Story:** As a system operator, I want a Batch to arm the matching engine at most once, and
only when it actually enqueued an order, so that a Batch does not drag unrelated pending orders into
a matching round that the equivalent single-order calls would not have triggered.

#### Acceptance Criteria

1. WHEN `add_limit_orders` enqueues at least one order, THE Canister SHALL perform exactly one
   Matching_Kickoff after the Batch completes, regardless of how many items succeeded.
2. WHEN every item of `add_limit_orders` fails, THE Canister SHALL perform no Matching_Kickoff.
3. WHEN `cancel_limit_orders` is called, THE Canister SHALL perform no Matching_Kickoff, whatever
   the outcome of its items, because `cancel_limit_order` does not perform one either.
4. WHEN a Batch is rejected under 3.1 so that no item is processed, THE Canister SHALL perform no
   Matching_Kickoff.
5. WHEN a Batch is empty, THE Canister SHALL perform no Matching_Kickoff.

### Requirement 8: Trading Accounts Are Resolved as on the Single-Order Endpoints

**User Story:** As a trading bot operator, I want the Batch_Endpoints to resolve my trading account
to its funding account exactly as the single-order endpoints do, so that batching changes nothing
about who owns an order or who is recorded as having acted on it.

#### Acceptance Criteria

1. THE Canister SHALL resolve the Caller of a Batch_Endpoint to its funding account by the same
   lookup `add_limit_order` and `cancel_limit_order` use.
2. THE Canister SHALL record `placed_by` on each created order as the Caller principal, exactly as
   `add_limit_order` does.
3. THE Canister SHALL record `canceled_by` on each cancelled order as the Caller principal, exactly
   as `cancel_limit_order` does.
4. WHEN a trading account calls a Batch_Endpoint, THE Canister SHALL apply the items against the
   associated funding account's balance and record any created order as owned by that funding
   account.

### Requirement 9: Trading Halts Are Enforced per Book

**User Story:** As an operator halting a pair, I want a halt to stop new orders on that pair only,
while cancellations stay possible everywhere, so that halting one book neither blocks trading on
others nor traps users in positions they cannot exit.

#### Acceptance Criteria

1. WHEN an item of `add_limit_orders` names a pair under Trading_Halt, THE Canister SHALL fail that
   item with `TradingHalted`.
2. WHEN an item of `add_limit_orders` fails under 9.1, THE Canister SHALL process items naming other
   pairs normally.
3. THE Canister SHALL permit cancellation of an order on a pair under Trading_Halt in
   `cancel_limit_orders`, preserving the guarantee that users can always exit.
4. THE Canister SHALL report a `TradingHalted` item failure under the Error_Envelope's
   `TemporaryError` arm, because a halt clears on its own and the arm is the Caller's retry contract.

### Requirement 10: Batching Adds No Quadratic Cost, and the Absolute Fit Is Scoped to a Recorded Envelope

**User Story:** As an operator, I want a Batch's cost to grow no worse than the work it stands for,
and the "a full Batch fits in one message" claim pinned to the state it was measured at, so that
nobody mistakes a fixture for a proof and the cap rests on evidence.

#### Acceptance Criteria

1. THE Canister SHALL process a Batch of `N` items at a total cost of at most `N ×` the equivalent
   single-item cost plus overhead that grows at worst as `O(N log N)` in `N`.
2. THE Canister SHALL NOT use, anywhere in the processing of a Batch, an algorithm that is quadratic
   or worse in `N`.
3. THE Canister SHALL record, alongside the chosen MAX_BATCH_LEN, the State_Envelope each cost
   measurement was taken at.
4. THE Canister SHALL choose MAX_BATCH_LEN so that the most expensive Batch shape measured fits
   within the Instruction_Limit at the recorded State_Envelope, with a recorded margin.
5. THE Canister SHALL scope the claim that a full Batch fits in one message to the State_Envelope of
   10.3 rather than stating it unconditionally, because the per-item cost of a cancellation is
   bounded neither by Caller input nor by the Canister's own state.
6. WHEN a Batch call exceeds the Instruction_Limit, THE Canister SHALL trap and the platform SHALL
   discard the call's state changes, so the call applies nothing and leaves no partial or corrupted
   state.
7. WHEN a Caller retries a Batch with fewer items after one exceeded the Instruction_Limit, THE
   Canister SHALL process the smaller Batch normally, because the failed call applied nothing.

### Requirement 11: No New Event Types and No State Migration

**User Story:** As a canister operator upgrading a live canister, I want batching to add no event
type and no persisted-state change, so that the upgrade carries no migration step and no replay
risk.

#### Acceptance Criteria

1. THE Canister SHALL record exactly one Operation_Event per **successful** item of a Batch.
2. WHEN an item of a Batch fails, THE Canister SHALL record no event for it.
3. WHEN a cancellation succeeds, THE Canister SHALL additionally record its Settling_Event in the
   same message, so that `N` successful cancellations leave `2N` entries in the event log.
4. THE Canister SHALL NOT introduce any new event type.
5. THE Canister SHALL leave event-log replay, the state snapshot, and the shape of persisted state
   unchanged, and SHALL introduce no new persistent state structure and require no migration.
6. THE Canister SHALL add both Batch_Endpoints as purely additive changes that alter the behaviour
   of no existing endpoint and break no existing client.
