---
id: DEFI-2849
title: MVP Circuit-Breaker Controls
tags: [circuit-breaker, permissions, security, trading-halt]
---

# MVP Circuit-Breaker Controls

## Motivation

OISY TRADE has no way to stop trading when something goes wrong. We want two
controller-gated **soft halts** so an operator can contain an incident without
tearing down state and without trapping users' funds:

- **Global trading halt** — when the matching engine itself is suspect (a matching
  or settlement bug), stop all new orders and all matching.
- **Per-pair halt** — when one pair's ledger is compromised or behaving suspiciously,
  stop new orders and matching on that pair only, leaving every other pair trading.

These are **soft** halts: state is always preserved, and users can always exit
(cancel resting orders and withdraw available balance). Both are invoked only by the
canister controller.

## Requirements

- **R1 — Global halt blocks entry.** If trading is halted, then a new `add_limit_order`
  is rejected with `TradingHalted` and the matching engine starts no new matching and
  produces no new fills (in-flight settling still drains — R2).
- **R2 — Halt preserves the exit.** Under global halt, `cancel_limit_order` and
  withdrawal of available balance still succeed; `resume_trading` re-enables orders and
  matching.
- **R3 — Per-pair halt is isolated.** If pair A is halted, then orders on A are rejected
  with `TradingHalted` and A's resting orders do not fill; orders on every other pair
  succeed and match; a cancel on A still succeeds. A per-pair halt is requested by passing
  a pair list to `halt_trading` / `resume_trading`; targeting an unregistered pair traps.
- **R4 — Controller-only.** Every admin endpoint rejects a non-controller caller with
  `NotController`.
- **R5 — Durable across upgrade and replay.** All control state survives a canister
  upgrade (snapshot) and event-log replay, and old-format snapshots (written before this
  change) still load, decoding to "no controls active".
- **R6 — Idempotent and auditable.** Halting an already-halted target is a no-op success
  that still emits an event for the audit trail.
- **R7 — Reconcile-before-record cannot be skipped (compile-time).** A `deposit`/`withdraw`
  cannot be recorded without first turning its `PreAsyncPermit` into a `PostAsyncPermit`
  via the post-await `reconcile` step; omitting it fails to compile. This is a
  compile-time gate only — `reconcile` does not re-check permissions post-await.

## Non-goals

- **`Delisted` pair state.** MVP has two per-pair states only — Active and Halted.
  A delist/teardown state is out of scope.
- **Per-account freeze.** Descoped after leadership review; freezing a principal's
  deposits/withdrawals/orders will not be built under DEFI-2849. The async-permit
  machinery is retained only for its compile-time reconcile-before-record obligation.
- **Hard halts.** No mechanism tears down or discards state; every control here is
  reversible and state-preserving.
- **Binding a sync permit to its payload.** The async path is compile-enforced
  (reconcile-before-record), but nothing stops in-module code from minting a
  `Permit::Sync` for the wrong payload (e.g. recording a deposit as sync). Closing that
  would need per-event smart constructors — deliberately out of scope; it's a
  deliberate-misuse case caught by review, not the forgettable "forgot to reconcile"
  mistake the types *do* close.

## Design Decisions

Two decisions are foundational; the rest of the design is in service of them.

- **Gate every state change at a single choke point: event recording.** Every state
  mutation is already an append-only audit event, so the one place to enforce "is this
  allowed?" is at the moment the event is recorded — not scattered across call sites.
  That gives exactly one site to get right, and nothing can mutate state without having
  passed a check. Enforcement therefore lives on the **live recording path**, above the
  shared apply path: the apply path is also the replay path and must stay unconditional,
  so replay reproduces state regardless of the permissions in force at replay time.

- **Synchronous and asynchronous admission are structurally different.** A synchronous
  action (e.g. recording a fill) is checked once, at the recording site. An asynchronous
  action (deposit, withdraw) crosses an `await` to touch the ledger — the "outside world"
  — and the external effect commits across that `await`, so its admission cannot be a
  single synchronous check at the recording site. Instead it is checked **pre-await** and
  the pre-await admission *must* be carried across the `await` and reconciled **post-await**
  before the event can be recorded. This obligation is enforced at the type level rather
  than by convention — see Implementation. (No control in scope denies post-await; the
  reconcile step is therefore observational, but the obligation to perform it before
  recording is compile-enforced.)

Supporting decisions:

- **Per-pair status is keyed by `OrderBookId` (a set of halted books), not a field on
  `TradingPair`.** `TradingPair` is a `BiBTreeMap` key; mutating a status field on a map
  key is a bug. The set matches how orders and the matching loop already resolve
  `pair -> book_id`.

(Why not a `UserOpGuard` bolt-on, a `process_async` function, a single `SyncOp`/`AsyncOp`
enum, or a persisted `clean: bool` — see Discussed Alternatives.)

## Implementation

### Constraints (architecture that shapes everything)

The canister is **event-sourced**. `state::audit::process_event`
(`canister/src/state/audit/mod.rs`) applies a mutation via `apply_state_transition`
**and** appends the event to the stable log via `storage::record_event`;
`state::audit::record_event` appends without applying (used by the async withdraw path,
where the debit is applied directly before the `await`). The invariant is **replay
equivalence**: replaying the log through `apply_state_transition` reproduces heap state
exactly. Separately, the heap is snapshotted at `pre_upgrade` and restored at
`post_upgrade` (`canister/src/state/snapshot/mod.rs`).

Two consequences the whole design relies on:

- **`replay_events` calls `apply_state_transition` directly, bypassing
  `process_event`/`record_event`.** So anything added to `process_event`/`record_event`
  (including the `Permit` parameter) is **live-path only** and never constrains replay.
- **New persisted state must be (a) added to `State`, (b) written by an
  `apply_state_transition` arm so replay reproduces it, and (c) added to `StateSnapshot`
  so upgrades preserve it.** `State` mutators stay **unconditional** — admission is
  checked before the event is emitted, never re-checked on replay.

### Permissions layer

New module `canister/src/state/permissions/` (`mod.rs` + `tests.rs`). A `Permissions`
struct owns both controls and all gating logic:

```rust
pub struct Permissions {
    trading_halted: bool,
    halted_pairs: BTreeSet<OrderBookId>,   // Active = absent, Halted = present
}
```

`struct State` gains a `permissions: Permissions` field, default-empty in `State::new`;
`from_state` destructures `State` exhaustively, which forces the snapshot wiring.

Permit tokens — produced only by `Permissions` (`SyncPermit`'s private field makes it
non-constructible outside this module, so holding any permit is proof a check ran):

```rust
pub struct SyncPermit(());                                // sync admission proof (non-forgeable)
#[must_use] pub struct PreAsyncPermit(());
pub struct PostAsyncPermit(());                           // only via PreAsyncPermit::reconcile

pub enum Permit { Sync(SyncPermit), Async(PostAsyncPermit) }
// From<SyncPermit> / From<PostAsyncPermit> for Permit, so call sites read `permit.into()`.

pub enum UnauthorizedError { TradingHalted, NotController }
```

`PreAsyncPermit::reconcile(self) -> PostAsyncPermit` consumes the pre-permit and yields
the post-await proof. It is **observational only** — the ledger effect already committed,
so it never denies; its sole purpose is to carry the compile-time reconcile-before-record
obligation across the `await`.

One `permit_*` per `EventType`, so the policy for each event is exhaustive, named, and
greppable:

- Gated: `permit_trading(caller, book)` (global-or-pair halt → `TradingHalted`),
  `permit_matching(book)` (global halt or that book's pair halt → `TradingHalted` —
  **per-book**, so the matching loop gates each book through this one method rather than a
  separate `is_pair_halted` filter), `permit_deposit(caller)` / `permit_withdraw(caller)`
  (return `PreAsyncPermit`). A globally- or per-pair-halted pair both surface the single
  `TradingHalted` — there is no distinct `PairHalted`.
- Infallible — ungated *in the permission layer*, but not all truly unguarded:
  `permit_cancel` and `permit_settling` are genuinely ungated; `permit_admin` is the
  permit for the halt/upgrade events and is controller/lifecycle-gated *at the endpoint*;
  `permit_add_trading_pair` is controller-gated at the endpoint. These permits return
  their permit value directly — documenting "not gated here" at a named, greppable site
  (it does not mean "unguarded").
  **`permit_settling` is intentionally book-less and never gated** — settling must always
  drain (even under halt) so already-matched fills don't strand (R2); a per-book settling
  gate would reintroduce that stranding.
- Predicate: per-pair halt is enforced via `permit_matching(book)`, not a standalone
  matching-loop filter.

`NotController` is **not** produced by `permit_*` (that axis needs `runtime.is_controller`,
which pure state can't see) — it's returned by the endpoint guard, but lives in the same
enum because both axes mean "you may not do this".

`audit::process_event` and `audit::record_event` gain a `permit: Permit` parameter
(live-only, never touches replay). To persist a deposit/withdraw you need
`Permit::Async(PostAsyncPermit)`, and a `PostAsyncPermit` exists **only** via
`reconcile` — so skipping the post-await reconcile step won't compile (R7).

Bound on R7: the types force *reconcile-before-record* for the async path, but a
`Permit::Sync` is still constructible in-module for any payload (e.g. a deposit could
mint a `permit_settling()` token and record itself as sync). `SyncPermit`'s private field
only prevents forging a permit *outside* this module; it does not bind a token to a
specific payload. That residual is accepted — see Non-goals.

### Events

`enum EventType` (`canister/src/state/event/mod.rs`) — **append** minicbor indices, never
reuse:

```rust
#[n(9)]  SetHalt(#[n(0)] SetHaltEvent),                   // { book_ids: Option<Vec<OrderBookId>>, halted }
```

`SetHaltEvent` carries the optional book-id list (the resolved pair filter) and the new
halted flag. Replay reproduces the endpoint semantics exactly: `book_ids = None` sets the
global flag, and on resume (`halted = false`) additionally clears the whole per-pair set;
`book_ids = Some(ids)` adds/removes those books from the set. The
`apply_state_transition` arm mutates `state.permissions` (persistence-independent — no
stable-memory writes).

### Snapshot

`StateSnapshot` (`canister/src/state/snapshot/mod.rs`) gains one backward-compatible
field after `fee_pool`:

```rust
#[n(10)] pub permissions: Option<PermissionsSnapshot>,    // { trading_halted, halted_pairs }
```

A small `PermissionsSnapshot` entry struct. `from_state` encodes `None` when all-default
(per the `fee_pool` idiom); `into_state` does `unwrap_or_default()` and rebuilds the sets.
Absent field decodes to default (R5).

### Enforcement points

- **`add_limit_order`** — after `assert_caller_is_allowed`, validate the order (unknown
  pair → `UnknownTradingPair`; tick/lot/notional → their errors), then the halt gate
  `permit_trading(caller, book)?`. Map `UnauthorizedError::TradingHalted` onto the
  internal + public `AddLimitOrderError` (a halted pair, global or per-pair, surfaces
  `TradingHalted`). The `SyncPermit` flows into the existing
  `process_event(AddLimitOrder, …)`.
- **Matching** (`canister/src/execute/mod.rs`) — `run_once` **always drains in-flight
  settling first** (`drain_settling` before any matching), then matches only the books
  for which `permit_matching(book)` is `Ok`. A book is gated by that one call: it returns
  `Err(TradingHalted)` under global halt (every book) and for a per-pair-halted book — so
  there is no separate `is_pair_halted` loop filter. Draining-first is required:
  a halt can land while `pending_settling_events` are queued (a prior chunk hit the
  instruction budget), and those events apply the balance effects of already-matched
  fills — skipping them would strand a counterparty's proceeds for the halt's duration,
  violating "users can always exit" (R2). The "work remaining?" predicate
  (`has_matchable_pending_orders`) counts only books with pending orders **and**
  `permit_matching(book).is_ok()`, so under global or per-pair halt `run_once` reschedules
  **only** for leftover settling (`MoreWork` iff `has_pending_settling_events()`, else
  `Complete`) and never busy-spins on halted books' pending orders. `resume_trading`
  (global or per-pair) re-arms matching from the endpoint.
- **`deposit` / `withdraw`** (both async) — identical shape:

  ```rust
  let pre  = state::with_state(|s| s.permissions().permit_<op>(caller))?;   // pre-await admission
  // ... existing async ledger work (withdraw debits directly before its await) ...
  let post = pre.reconcile();                                               // post-await reconcile
  state::with_state_mut(|s| process_event(s, Deposit, post.into(), runtime)); // deposit
  //   or withdraw success branch:  record_event(Withdraw, post.into(), runtime);
  ```

  The error path (`await?` fails) drops the un-reconciled `PreAsyncPermit`: no record,
  no permit, no false trap.
- **`cancel_limit_order`** — **no change**; cancels stay open under every control.
  Covered by tests, not code.
- **Other recorders** (`add_trading_pair`, matching/settling, `Upgrade`) pass the
  matching infallible permit (`permit_add_trading_pair` / `permit_settling` /
  `permit_admin`). The low-level `Init` append in `lifecycle.rs` is unchanged.

### Admin endpoints

Two controller-gated endpoints. Each: a business fn in `canister/src/lib.rs` guarded by
`if !runtime.is_controller(&runtime.msg_caller()) { return Err(...NotController) }`, which
builds the event and records it with `permit_admin()`; a thin `#[ic_cdk::update]` wrapper
in `canister/src/main.rs`; and a declaration in `canister/oisy_trade.did`.

| Endpoint | Arg | Event |
|---|---|---|
| `halt_trading` | `(Option<Vec<TradingPair>>)` | `SetHalt { book_ids, halted: true }` |
| `resume_trading` | `(Option<Vec<TradingPair>>)` | `SetHalt { book_ids, halted: false }` |

`halt_trading` / `resume_trading` take an optional pair filter and keep returning
`Result<(), UnauthorizedError>` (`UnauthorizedError { NotController }` only):

- `halt_trading(None)` sets the global flag; `halt_trading(Some(pairs))` adds those pairs
  to the halted set.
- `resume_trading(None)` clears the global flag **and** empties the entire per-pair set in
  one call; `resume_trading(Some(pairs))` removes those pairs from the set.
- A pair is halted iff `global_flag || pair ∈ set`; this also drives `get_trading_pairs`'
  `TradingStatus::Halted`.
- `Some(pairs)` is validated up front: any unregistered pair **traps** (`ic_cdk::trap`)
  before anything is recorded — no new error variant.
- `Some(pairs)` carrying more than `MAX_HALT_BOOKS` (100) entries **traps**
  (`ic_cdk::trap`) before anything is recorded, bounding the size of the `SetHalt` audit
  event. `None` (global) is unaffected.

Idempotent calls are no-op successes that still emit the event (R6). `oisy_trade.did`
updates the two endpoints' signatures, the unified `SetHaltEvent`, and the
`AddLimitOrderError::TradingHalted` variant. The repo's candid equality check must pass.

### Test plan

Integration (`integration_tests/tests/tests.rs`, PocketIC):

1. **Global halt** (R1, R2): under halt, `add_limit_order` → `TradingHalted`; a resting
   order placed pre-halt still cancels; a withdrawal of available balance succeeds;
   `resume_trading` re-enables orders.
2. **Per-pair halt** (R3): with two pairs, `halt_trading(Some([A]))` → orders on A →
   `TradingHalted`, orders on B succeed and match, a cancel on A succeeds, and
   `get_trading_pairs` reports A `Halted` / B `Trading`. A controller targeting an
   unregistered pair traps; `resume_trading(None)` clears the per-pair halt too.
3. **Per-pair halt stops matching** (R3): resting crossable orders on a halted pair don't
   fill after the timer ticks; `resume_trading(Some([A]))` lets them fill; the other pair
   is never affected.
4. **Authorization** (R4): every admin endpoint rejects a non-controller with
   `NotController`.

Unit:

- `state/permissions/tests.rs`: `permit_trading`/`permit_matching` return the right
  `Ok`/`UnauthorizedError`; the infallible permits return their permit unconditionally.
- `state/audit/tests.rs`: each new `EventType` arm applies the expected mutation (R5 replay).
- `state/snapshot/tests.rs`: `from_state -> into_state` round-trips both controls; an
  old-format snapshot (field absent) decodes to defaults (R5 upgrade).
- `execute/tests.rs`: `run_once` is a no-op under global halt; halted books are skipped
  while others match and the executor settles rather than busy-spinning.
- Worst-case CBOR roundtrip proptest (`test_fixtures`) fuzzes the new events.

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
# + the repo's candid equality check (see justfile / CI)
```

### Delivery / PR sequence

Stacked, ordered by increasing complexity; each PR is independently mergeable,
compilable, and testable, and rebases on its parent. The async-permit *types* land in
PR 1 as part of the permit vocabulary (so the sync/async distinction is visible from the
scaffolding). Each mechanism PR carries its own
section in the runbook (`docs/runbook-circuit-breakers.md`) so docs stay in lockstep;
each section states *who* may invoke the control (the canister controller) and *when*
to use it (matching-engine bug → global halt; compromised/suspect ledger → per-pair
halt).

- **PR 1 — Permission scaffolding (behavior-neutral).** Empty `Permissions` + `State`
  field, snapshotted (backward-compatible); the full permit vocabulary —
  `UnauthorizedError`, `SyncPermit`, the async types (`PreAsyncPermit`/`PostAsyncPermit`/
  `reconcile`), and `Permit { Sync, Async }`; one infallible `permit_*`
  per `EventType`, with `permit_matching(book)` taking the book and
  `permit_deposit`/`permit_withdraw` returning `PreAsyncPermit` (reconciled to
  `Permit::Async` at the deposit/withdraw recorder sites); thread the `Permit` parameter
  through `process_event`/`record_event` and every call site.
  *Acceptance:* no behavioral change (all existing tests pass); `oisy_trade.did` unchanged;
  snapshot round-trips empty + old-format decodes to default; every recorder call site
  supplies a permit; deposit/withdraw record via `Permit::Async` (the post-await reconcile
  is structurally present even though it never denies).
- **PR 2 — Global trading halt.** `trading_halted`; the unified `SetHalt` event + arm +
  snapshot; `permit_trading`/`permit_matching(book)` gate the global halt; `run_once`
  drains settling then matches only `permit_matching(book).is_ok()` books;
  `halt_trading(None)`/`resume_trading(None)` + candid; `AddLimitOrderError::TradingHalted`.
  *Acceptance:* R1, R2 (incl. settling still drains under halt), R4 (these two endpoints),
  R5 for the halt flag.
- **PR 3 — Per-pair halt.** `halted_pairs`; extend the unified `SetHalt` event with the
  optional `book_ids` filter + arm + snapshot; `permit_matching(book)` and `permit_trading`
  extended with the per-book pair check (no separate matching-loop filter); the existing
  `halt_trading`/`resume_trading` endpoints gain the `Option<Vec<TradingPair>>` filter
  (per-pair halt reuses `TradingHalted`; an unregistered pair traps; `resume_trading(None)`
  also clears the whole per-pair set).
  *Acceptance:* R3, R4 (the halt endpoints, incl. trap-on-unknown-pair), R5 for per-pair
  state, and the executor does not busy-spin on a halted-but-non-empty book.

## Discussed Alternatives

- **An `Authority` guard parameter on the `State` mutators.** Rejected: the mutators are
  the replay path, so replay would re-acquire the guard — which diverges for async ops (a
  permission change landing during an `await` is logged before the deposit/withdraw event,
  so a re-check at the event's log position would deny an op that legitimately committed).
  Admission must live above the shared apply path.
- **A single recording function with an `Unguarded`/`System` authority variant.**
  Superseded by `permit_*`-per-event: the infallible permits document "intentionally
  ungated" at a named site and remove the need for a freely-constructible catch-all.
- **A dedicated `process_async` consuming `PostAsyncPermit`.** Superseded: putting the
  `Permit` parameter on the existing `process_event`/`record_event` subsumes it and keeps
  a single recording API.
- **A single `SyncOp`/`AsyncOp` enum guard.** Rejected: one enum shares a method surface
  and cannot express "a `PreAsyncPermit` *must* become a `PostAsyncPermit`". Distinct
  types are what make the post-await reconcile compile-enforced.
