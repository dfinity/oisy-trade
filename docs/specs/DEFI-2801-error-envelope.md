---
id: DEFI-2801
title: Consolidate user-facing errors into forward-compatible, disposition-tagged variants
tags: [errors, candid, api]
---

# Consolidate user-facing errors into forward-compatible, disposition-tagged variants

## Motivation

Today every fallible endpoint returns a bare, flat error variant (`DepositError`, `WithdrawError`,
`AddLimitOrderError`, `CancelLimitOrderError`, …). A caller that wants to decide *whether to
retry* must enumerate every variant and hard-code the mapping itself; the DEX gives it no
machine-readable signal. Some failures are the caller's to fix (`UnsupportedToken`, `InvalidPrice`,
`InsufficientFunds`), some are transient and worth retrying (`OperationInProgress`, a ledger that is
`TemporarilyUnavailable`), and some are the DEX's own fault (`LedgerInternalError`, an
accounting/ledger inconsistency).

The consumers here are **multiple independent clients, written in different languages, that are not
upgraded in lockstep with the DEX**. That rules out a Rust-only helper and makes Candid
forward-compatibility a hard requirement: a client built against today's interface must keep working
— without traps and without silently mishandling — after the DEX adds a new error case tomorrow.

The fix is to tag every error with the **disposition** — what the caller should do — as the
outer variant, and carry the specific reason as an inner `opt variant`:

- **`RequestError`** — caller-side; the request will not succeed as-is. Correct the input, satisfy a
  precondition (fund / approve / authorize), or stop. Do **not** auto-retry unchanged.
- **`TemporaryError`** — transient; retry the same call after a backoff.
- **`InternalError`** — DEX-side fault; surface to operators. Do **not** retry.

Two adjacent input-handling bugs are folded in because they live in the same surface and the PRs
touch the same files: `cancel_limit_order` maps a *malformed* `order_id` to `OrderNotFound`
(conflating bad input with a missing order), and `get_order_status` **traps** on a malformed
`order_id` (user input can panic the canister).

## Requirements

- **R1**: Every user-facing error is a **disposition-tagged variant** whose arms are drawn from
  `RequestError` / `TemporaryError` / `InternalError`; each arm carries `opt variant { <leaves> }`.
  An error declares only the arms it can actually produce. Applies to `add_limit_order`,
  `cancel_limit_order`, `deposit`, `withdraw`, `get_order_status`, `get_order_book_ticker`,
  `get_order_book_depth`, and both the per-token and request-level errors of `get_balances` /
  `get_fee_balances`. Admin endpoints are out of scope.
- **R2**: The disposition arm is the contract (documented in `dex.did`):
  `RequestError` ⇒ caller-side, do not auto-retry unchanged (fix input / satisfy a precondition /
  stop); `TemporaryError` ⇒ retry after backoff; `InternalError` ⇒ DEX-side fault, surface, do not
  retry.
- **R3**: Each leaf error is assigned to exactly one arm per [Disposition membership](#disposition-membership).
- **R4**: Each arm's payload is `opt variant`. A client generated against an older interface decodes
  an unknown future *leaf* as `null` while still reading the **arm** (disposition). (Verified by a
  decode test that feeds a superset-leaf value into the shipped type — inner `null`, arm intact.)
- **R5**: `CallFailed` is a `TemporaryError`, not an indeterminate/reconcile case — see D3. This holds
  only while the ledger calls are guaranteed-response (`call_unbounded_wait`); see Constraints.
- **R6**: `cancel_limit_order` with a malformed `order_id` returns `RequestError(InvalidOrderId)`,
  distinct from `RequestError(OrderNotFound)`.
- **R7**: `get_order_status` never traps. A malformed `order_id` returns
  `Err(GetOrderStatusError::RequestError(InvalidOrderId))`; a well-formed but unknown id returns
  `Ok(OrderStatus::NotFound)`; a well-formed known id returns `Ok(<status>)`.
- **R8**: The hand-written `canister/dex.did` matches the generated interface
  (`check_candid_interface_compatibility` passes) and documents the R2 disposition contract.

## Non-goals

- **No fourth disposition arm.** No `Indeterminate`/`Reconcile` (see D3) and no split of `RequestError`
  into "fix the request" vs "satisfy a precondition" vs "stop" — those finer distinctions are carried
  by the inner leaf (`InsufficientFunds` is self-evidently fund-and-retry) and don't change the coarse
  client action. The three arms are treated as the complete, frozen partition of caller actions.
- **No free-text `message` field.** The typed leaves are self-describing. If a human-readable message
  is ever needed it goes on the individual leaf records (e.g. `CallFailed { …, reason }` already does),
  not as a top-level field.
- **Admin endpoints are out of scope** (e.g. `add_trading_pair`): controller-only, not part of the
  multi-language client surface this targets.
- **No changes to internal/state-layer error types** (`canister/src/state`, `order`, `ledger`
  internal enums) beyond mapping them to the disposition-tagged public types at the boundary.
- **No change to which errors are logged.** The `main.rs` per-error logging arms encode
  *log-worthiness*, which is deliberately not the disposition (`OperationInProgress` is a
  `TemporaryError` yet logged as a user action). Left untouched.
- **Accepted residual limitations**:
  - A client hitting a *future leaf* sees inner `null` and loses the specific reason, but keeps the
    disposition arm — the intended trade.
  - The outer arm set is frozen: adding a *new disposition* later is a breaking change (old clients
    trap on the unknown arm). Accepted, because the three arms exhaustively partition what a caller can
    do (fix your side / wait / it's the DEX's fault).

## Design Decisions

- **D1 — Disposition is the outer variant tag; the specific reason is an inner `opt variant`.**
  `type DepositError = variant { RequestError : opt variant {…}; TemporaryError : opt variant {…};
  InternalError : opt variant {…} }`. The tag is typed and self-documenting (no numeric-code → meaning
  doc dependency), and it separates *what grows* (specific reasons → inner `opt`, forward-compatible)
  from *what's stable* (the small set of caller actions → bare outer arm).
- **D2 — Three arms, not more.** The disposition axis is "what does the client do," and that space is
  small and bounded: act on your side, wait, or escalate to the DEX. Finer distinctions
  (precondition-vs-malformed, terminal-vs-correctable) live in the inner leaf, which clients inspect
  only when they want the specific reason.
- **D3 — No `Indeterminate`/reconcile arm; `CallFailed` ⇒ `TemporaryError`.** Both ledger calls use
  `call_unbounded_wait` (guaranteed response) and ICRC `icrc1_transfer` / `icrc2_transfer_from` commit
  atomically with their reply. So a reject implies the transfer did **not** commit — no side effect on
  either side — making the whole operation safe to retry. There is nothing to reconcile.
- **D4 — Naming `RequestError / TemporaryError / InternalError`.** Symmetric `-Error` triple,
  attribution-clear (your *request* / *transient* / the DEX's *internals*), and IC-native — no
  `Server` (not IC terminology) and no `Caller`/`Callee` jargon. `Internal` is standard Rust/IC
  vocabulary and fits the contents literally (`LedgerInternalError`).
- **D5 — The `InsufficientFunds` asymmetry.** Deposit `InsufficientFunds` (the caller's external
  wallet is short) ⇒ `RequestError`. Withdraw's ledger-reported `InsufficientFunds` (DEX accounting
  says it has the funds, the ledger disagrees) ⇒ `InternalError` — a genuine invariant violation.
- **D6 — Malformed `order_id` ⇒ a dedicated `InvalidOrderId` leaf** under `RequestError` (not
  `OrderNotFound`, not a trap), on both `cancel_limit_order` and `get_order_status`.

## Implementation

### Constraints

- Both ledger calls are guaranteed-response: `icrc2_transfer_from` (`ledger/mod.rs`, deposit) and
  `icrc1_transfer` (`ledger/mod.rs`, withdraw) use `call_unbounded_wait`. **D3/R5 depend on this.**
  `ledger/mod.rs` carries `TODO(DEFI-2745): Consider switching to bounded_wait` — if that lands,
  best-effort timeouts become genuinely indeterminate and `CallFailed` must move out of
  `TemporaryError` into a reconcile-style disposition (reintroducing a fourth arm).
- `dex_types::OrderId = String`, parsed to `canister::order::OrderId` via `FromStr`
  (`OrderIdParseError`). Parse points: `dex_canister::cancel_limit_order`, `dex_canister::get_order_status`.
- `check_candid_interface_compatibility` (`canister/src/main.rs`) pins `dex.did` to the generated
  interface via `service_equal`; every interface change updates `dex.did` by hand.
- Candid's forgiving `opt` decode rule provides the inner-leaf forward-compatibility; only clients
  generated from the updated `.did` benefit (the `opt` must ship now — it can't be retrofitted).

### `dex_types` (`libs/types/src/lib.rs`)

Each public error becomes a disposition enum whose arms wrap `Option<…>` of a per-arm leaf enum
(`Option` ⇒ Candid `opt`). Pattern (DepositError shown; the rest follow):

```rust
pub enum DepositError {
    RequestError(Option<DepositRequestError>),
    TemporaryError(Option<DepositTemporaryError>),
    InternalError(Option<DepositInternalError>),
}

pub enum DepositRequestError {
    AmountExceedsMaximum,
    UnsupportedToken { token_id: TokenId },
    InsufficientFunds { balance: Nat },
    InsufficientAllowance { allowance: Nat },
}
pub enum DepositTemporaryError {
    OperationInProgress,
    LedgerTemporarilyUnavailable,
    CallFailed { ledger: Principal, method: String, reason: String },
}
pub enum DepositInternalError { LedgerError { reason: String } }
```

Provide `From<leaf>`/constructors so call sites read cleanly (e.g. `DepositRequestError::UnsupportedToken{..}.into()`).
The existing internal→public conversions (`canister/src/state`, `ledger`) and the direct
construction sites in `canister/src/lib.rs` now target these disposition-tagged types — placing each
leaf into its arm is the only behavioral change; internal flat enums are untouched (Non-goals).

Order-id fixes add leaves: `CancelLimitOrderError::RequestError(InvalidOrderId)` and a new
`GetOrderStatusError { RequestError(Option<GetOrderStatusRequestError>) }` with an `InvalidOrderId`
leaf.

### Disposition membership

| Error | `RequestError` | `TemporaryError` | `InternalError` |
|---|---|---|---|
| **DepositError** | `AmountExceedsMaximum`, `UnsupportedToken`, `InsufficientFunds`, `InsufficientAllowance` | `OperationInProgress`, `LedgerTemporarilyUnavailable`, `CallFailed` | `LedgerError` |
| **WithdrawError** | `AmountExceedsMaximum`, `AmountTooSmall`, `UnsupportedToken`, `InsufficientBalance` | `OperationInProgress`, `LedgerTemporarilyUnavailable`, `CallFailed` | `LedgerError`, `LedgerInsufficientFunds`* |
| **AddLimitOrderError** | `AmountExceedsMaximum`, `UnknownTradingPair`, `InvalidPrice`, `InvalidQuantity`, `InsufficientBalance` | — | — |
| **CancelLimitOrderError** | `InvalidOrderId`, `OrderNotFound`, `NotOrderOwner`, `OrderAlreadyFilled`, `OrderAlreadyCanceled` | — | — |
| **GetOrderStatusError** | `InvalidOrderId` | — | — |
| **GetOrderBookTickerError** | `UnknownTradingPair` | — | — |
| **GetOrderBookDepthError** | `UnknownTradingPair`, `LimitTooLarge` | — | — |
| **GetBalancesError** | `TokenNotSupported` | — | — |
| **GetBalancesRequestError** | `FilterTooLarge` | — | — |

\* withdraw's ledger-reported `InsufficientFunds` (D5). The synchronous errors (add/cancel/queries)
have only `RequestError` — a single-arm variant, which keeps the disposition vocabulary uniform across
the API.

### Canister logic (`canister/src/lib.rs`)

- `cancel_limit_order`: map `OrderId` parse failure to `RequestError(InvalidOrderId)` (was `OrderNotFound`).
- `get_order_status`: return `Result<OrderStatus, GetOrderStatusError>`; parse failure ⇒
  `Err(RequestError(InvalidOrderId))`; well-formed unknown id ⇒ `Ok(OrderStatus::NotFound)`. Remove the `panic!`.

### Candid (`canister/dex.did`)

- Top-of-file comment documenting the R2 disposition contract.
- Each error renders as `variant { RequestError : opt variant {…}; TemporaryError : opt variant {…};
  InternalError : opt variant {…} }`, declaring only the arms it produces.
- New `InvalidOrderId` leaf on `CancelLimitOrderError`; new `GetOrderStatusError`; new
  `get_order_status` signature returning a result.

### Test plan

Unit (`libs/types/src/tests.rs`):
- For every leaf, assert the internal→public conversion places it under the arm in the membership
  table — parameterized, no copy/paste. (**R2**, **R3**)
- Forward-compat decode test: encode an error whose inner arm has an *extra* leaf, decode into the
  shipped (smaller) type; assert the inner decodes to `None` while the outer arm decodes intact.
  (**R4**)

Unit (`canister/src/.../tests.rs`, sibling files):
- `cancel_limit_order` with a malformed id ⇒ `RequestError(InvalidOrderId)`, not `OrderNotFound`. (**R6**)
- `get_order_status` with a malformed id ⇒ `Err(RequestError(InvalidOrderId))` and does not panic;
  well-formed unknown ⇒ `Ok(NotFound)`; well-formed known ⇒ `Ok(<status>)`. (**R7**)

Integration (`dex_int_tests`):
- Update existing deposit/withdraw/add/cancel error assertions to the disposition-tagged shape; assert
  the arm and the inner leaf for at least one case per endpoint. (**R1**)
- New: cancel + `get_order_status` malformed-id cases over the canister boundary, asserting no trap and
  the expected arm/leaf. (**R6**, **R7**)

Interface: `check_candid_interface_compatibility` passes against the updated `dex.did`. (**R8**)

Commands: `cargo test --workspace`, `cargo fmt --all -- --check`, `just lint`.

### Delivery / PR sequence

Stacked, bottom-to-top; each compiles and tests independently. PR2 and PR3 each depend only on PR1.

1. **PR1 — Disposition-tagged errors for the four update-endpoint errors.**
   `RequestError/TemporaryError/InternalError` shape + leaf enums for `AddLimitOrderError`,
   `CancelLimitOrderError`, `DepositError`, `WithdrawError`; map internal→public at the boundary;
   `dex.did` (disposition arms + contract doc block); unit + integration tests.
   *Accepts*: R1 (these four), R2, R3, R4, R8.
2. **PR2 — Extend to query errors.**
   Same shape for `GetOrderBookTickerError`, `GetOrderBookDepthError`, `GetBalancesError`,
   `GetBalancesRequestError`; `dex.did`; tests. *Accepts*: R1 (remainder), R3 (remainder), R8.
3. **PR3 — Stop conflating / trapping on malformed order IDs.**
   Add the `InvalidOrderId` leaf (map cancel parse failure to it); add `GetOrderStatusError` and make
   `get_order_status` return a non-panicking result; `dex.did`; unit + integration tests.
   *Accepts*: R6, R7, R8.

## Discussed Alternatives

1. **Keep bare flat enums, document retryability** (Binance/Coinbase norm). Rejected: no
   machine-readable signal; every multi-language client re-derives the map; doesn't satisfy the
   ticket's "an API that allows the caller to differentiate."
2. **Rust `is_retryable()`/`category()` helper, no wire change.** Rejected: serves only Rust callers;
   our consumers are multi-language and uncoordinated with DEX versions.
3. **Numeric `record { code : nat16; detail : opt E }` envelope** (HTTP-status-class codes). Seriously
   considered. Rejected in favor of typed disposition tags: clearer for multi-language clients (no
   code-range → meaning doc dependency), avoids the awkward "409 is a 4xx but retryable" tension, and
   the typed tag *is* the contract. Its one edge — adding a disposition is non-breaking (a new code) —
   was given up knowingly, since the three dispositions are treated as an exhaustive, frozen partition.
4. **Closed flat `variant { Transient; Permanent }` category, or a `record { category; error }`
   wrapper.** Rejected: a closed category in a return position can't gain cases compatibly, and the
   detail loses forward-compatibility; the inner `opt variant` solves the latter.
5. **A fourth `Indeterminate`/`Reconcile` arm for `CallFailed`.** Rejected once we confirmed both
   ledger calls are guaranteed-response (`call_unbounded_wait`): a reject implies no side effect, so
   there is nothing to reconcile and `CallFailed` is a safe-to-retry `TemporaryError` (D3). Revisit if
   DEFI-2745 switches to bounded-wait.
6. **Finer caller-side arms** (`Fix`/`Resolve`/`Stop`, or `BadRequest`/`UnprocessableRequest`). Rejected:
   they split on "what's wrong," not "what to do" — the caller action for all of them is the same
   ("don't auto-retry; act on your side"), so the distinction belongs in the inner leaf.
7. **Verb arms** (`Fix`/`Retry`/`Report` or `Resolve`/`Retry`/`Report`). Considered; the team preferred
   the symmetric, attribution-clear noun triple `RequestError/TemporaryError/InternalError`.
8. **Flat `record { code; message : text }`.** Rejected: discards the typed, structured leaf payloads
   on-chain callers consume.
