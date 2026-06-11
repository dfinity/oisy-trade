<!-- Filename: docs/specs/DEFI-2801-error-envelope.md -->
---
id: DEFI-2801
title: Consolidate user-facing errors into a forward-compatible error envelope
tags: [errors, candid, api]
---

# Consolidate user-facing errors into a forward-compatible error envelope

## Motivation

Today every fallible endpoint returns a bare, typed error variant (`DepositError`,
`WithdrawError`, `AddLimitOrderError`, `CancelLimitOrderError`, …). A caller that wants to
decide *whether to retry* must enumerate each variant and hard-code the mapping itself, and
the DEX gives it no machine-readable signal. Some failures are transient and worth retrying
(`OperationInProgress`, a ledger call that was `TemporarilyUnavailable`), some are permanent
(`UnsupportedToken`, `InvalidPrice`), some are recoverable only after a caller action
(`InsufficientBalance` — top up, then retry), and at least one is *indeterminate* (`CallFailed`
— a ledger transfer that may or may not have executed; blindly retrying risks a double
deposit/withdrawal).

The consumers here are **multiple independent clients, written in different languages, that are
not upgraded in lockstep with the DEX**. That rules out a Rust-only helper and makes Candid
forward-compatibility a hard requirement: a client built against today's interface must keep
working — without traps and without silently mishandling — after the DEX adds a new error case
tomorrow.

Two adjacent input-handling bugs are folded in because they live in the same surface and the
PRs touch the same files: `cancel_limit_order` maps a *malformed* `order_id` to `OrderNotFound`
(conflating bad input with a missing order), and `get_order_status` **traps** on a malformed
`order_id` (user input can panic the canister).

## Requirements

- **R1**: Every error-returning canister endpoint returns its error as an envelope `record` of a
  `code : nat16` and a `detail` holding `opt` of that endpoint's error enum — for example `deposit`
  returns `record { code : nat16; detail : opt DepositError }`.
  This applies to `add_limit_order`, `cancel_limit_order`, `deposit`, `withdraw`,
  `get_order_status`, `get_order_book_ticker`, `get_order_book_depth`, `add_trading_pair`, and
  both the per-token and request-level errors of `get_balances` / `get_fee_balances`.
- **R2**: `code` is always present and is the single source of truth for retry disposition,
  derived from the variant via `ErrorCode::code`. The envelope is constructed as
  `{ code: detail.code(), detail: Some(detail) }`.
- **R3**: `code` follows the disposition contract (documented in `dex.did`):
  - Unrecognized codes fall back by leading digit: **4xx ⇒ caller-side**, do not auto-retry the
    identical request; **5xx ⇒ DEX/ledger-side**, retry with backoff.
  - Recognized codes refine the fallback: **402** = action required (fix balance/allowance,
    then retry); **504** = indeterminate (reconcile state before retrying). **409** carries no
    special retry meaning — it is an ordinary 4xx permanent conflict (the resource is already in a
    terminal/existing state).
  - The leading-digit fallback is the only *stable* guarantee; specific code assignments may
    gain entries over time (see R6).
- **R4**: Each variant maps to the `code` listed in [Code assignments](#code-assignments).
- **R5**: `detail` is encoded as `opt` in the Candid interface for every envelope, so a client
  generated against an older interface decodes an unknown future variant as `null` while still
  reading `code`.
- **R6**: Adding a new error variant later requires no change to existing clients: against the
  shipped interface an unknown variant decodes to `detail = null` with `code` still readable.
  (Verified structurally by R5 plus a decode test that feeds a superset-variant value into the
  shipped type.)
- **R7**: `cancel_limit_order` with a malformed `order_id` returns `InvalidOrderId`
  (code 400), distinct from `OrderNotFound` (code 404).
- **R8**: `get_order_status` never traps. A malformed `order_id` returns
  `Err(GetOrderStatusError::InvalidOrderId)` (code 400); a well-formed but unknown id returns
  `Ok(OrderStatus::NotFound)`; a well-formed known id returns `Ok(<status>)`.
- **R9**: The hand-written `canister/dex.did` matches the generated interface
  (`check_candid_interface_compatibility` passes) and documents the R3 disposition contract.

## Non-goals

- **No category enum/field on the wire.** Disposition is carried by the numeric `code` range,
  not a `variant { Transient; Permanent; … }` — a returned variant cannot gain cases compatibly,
  and we already foresee ≥3 dispositions. (See Discussed Alternatives.)
- **No free-text `message` field now.** The typed `detail` is self-describing. Because the
  envelope is a record, a `message : opt text` / `retry_after : opt nat` field can be added
  later with zero breakage; deferred until a concrete need.
- **No changes to internal/state-layer error types** (`canister/src/state`, `order`, `ledger`
  internal enums) beyond mapping at the boundary.
- **No change to which errors are logged.** The `main.rs` per-error logging arms encode
  *log-worthiness*, which is deliberately not the same as retry disposition (`OperationInProgress`
  is retryable yet logged as a user action). Left untouched.
- **Accepted residual limitation**: an old client that hits a *future* error variant loses the
  fine-grained reason (`detail = null`) and must fall back to `code`. This is the intended
  trade — the coarse signal survives, the typed detail does not.

## Design Decisions

- **D1 — Envelope `record { code : nat16; detail : opt E }`.** Chosen over (a) a closed
  `variant { Transient; Permanent }` category and (b) a flat `record { code; message : text }`.
  Each axis is made forward-compatible by a *different* mechanism: `code` is an open numeric
  space (new values never break decoders, clients branch on range), and `detail` is `opt`
  (new variants decode to `null`). Unlike (a) it can grow; unlike (b) it keeps the typed,
  structured payloads (`InsufficientBalance { available, required }`, `CallFailed { ledger,
  method, reason }`).
- **D2 — Numeric `code` borrows the HTTP status *class* contract, not strict IANA semantics.**
  The stable guarantee is only the leading-digit fallback (4xx caller / 5xx server); specific
  codes reuse familiar HTTP values where they fit (402/403/404/409/422/500/503/504). Chosen over
  inventing a domain scheme (familiarity) and over committing to exact HTTP semantics everywhere
  (avoids procrustean mappings).
- **D3 — `ErrorCode::code` in `dex_types` is the single source of truth; wrapping happens at the
  canister entry-point boundary (`main.rs`).** `lib.rs` and its unit tests keep returning bare
  enums; only the thin entry points and the integration tests that exercise the wire change.
- **D4 — `CallFailed` ⇒ 504 (indeterminate), separate from 503/500 (retryable).** A failed
  inter-canister ledger call may have executed; a blind retry risks a double transfer, so it must
  be distinguishable from a safe-to-retry transient.
- **D5 — Malformed `order_id` ⇒ dedicated `InvalidOrderId`** (not `OrderNotFound`, not a trap),
  on both `cancel_limit_order` and `get_order_status`.

## Implementation

### Constraints

- `dex_types::OrderId = String`, parsed to `canister::order::OrderId` via `FromStr`, which
  returns `OrderIdParseError`. The parse points are `dex_canister::cancel_limit_order` and
  `dex_canister::get_order_status`.
- `check_candid_interface_compatibility` (in `canister/src/main.rs`) pins `dex.did` to the
  generated interface via `service_equal`; every interface change updates `dex.did` by hand.
- Candid's forgiving `opt` decode rule is what provides `detail` forward-compatibility; only
  clients generated from the updated `.did` benefit (the `opt` must be present from this release
  onward — it cannot be retrofitted to already-shipped bare-variant clients).
- The ic_cdk entry points in `main.rs` are the wire boundary; `dex_canister`'s `lib.rs` functions
  are the internal Rust API used by unit and integration tests.

### `dex_types` (`libs/types/src/lib.rs`)

New public items:

```rust
/// Retry disposition is carried by `code`; see the code-range contract in `dex.did`.
/// `detail` is `opt` on the wire so clients built against an older interface decode an
/// unknown future variant as `null` while still reading `code`.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Serialize, Deserialize)]
pub struct ErrorInfo<E> {
    pub code: u16,            // candid: nat16
    pub detail: Option<E>,   // candid: opt E
}

pub trait ErrorCode {
    fn code(&self) -> u16;
}

impl<E: ErrorCode> From<E> for ErrorInfo<E> {
    fn from(detail: E) -> Self {
        Self { code: detail.code(), detail: Some(detail) }
    }
}
```

(If the blanket `From` trips coherence, fall back to an inherent `ErrorInfo::of(detail)` and use
`.map_err(ErrorInfo::of)` at the boundary.)

`ErrorCode` is implemented for every user-facing error enum per [Code assignments](#code-assignments).
Nested cases (`DepositError::LedgerError(LedgerTransferFromError::…)`,
`WithdrawError::LedgerError(LedgerTransferError::…)`) match on the inner variant.

New error surface for the order-id fixes:

```rust
pub enum CancelLimitOrderError { /* …existing… */ InvalidOrderId }

pub enum GetOrderStatusError { InvalidOrderId }
```

### Canister entry points (`canister/src/main.rs`)

Each fallible endpoint wraps at the boundary, e.g. `… .map_err(ErrorInfo::from)`:

- `add_limit_order`, `cancel_limit_order`, `deposit`, `withdraw` →
  `Result<T, ErrorInfo<…Error>>`.
- `get_order_book_ticker`, `get_order_book_depth`, `add_trading_pair` → likewise.
- `get_order_status` → `Result<OrderStatus, ErrorInfo<GetOrderStatusError>>`.
- `get_balances` / `get_fee_balances` →
  `Result<Vec<Result<UserTokenBalance, ErrorInfo<GetBalancesError>>>, ErrorInfo<GetBalancesRequestError>>`
  (both the per-token and the request-level error are wrapped).

The existing per-error logging `match` arms are preserved; if they now sit before the `map_err`,
they keep matching the bare enum.

### Canister logic (`canister/src/lib.rs`)

- `cancel_limit_order`: map `OrderId` parse failure to `CancelLimitOrderError::InvalidOrderId`
  (was `OrderNotFound`).
- `get_order_status`: return `Result<OrderStatus, GetOrderStatusError>`; parse failure ⇒
  `Err(GetOrderStatusError::InvalidOrderId)`; well-formed unknown id ⇒ `Ok(OrderStatus::NotFound)`.
  Remove the `panic!`.

### Candid (`canister/dex.did`)

- A top-of-file comment block documenting the R3 disposition contract.
- Each `*Result` `Err` arm becomes the envelope record whose `detail` is `opt` of that result's
  error enum (e.g. `opt WithdrawError`).
- New `InvalidOrderId` variant on `CancelLimitOrderError`; new `GetOrderStatusError`; new
  `get_order_status` signature returning a result.

### Code assignments

Leading digit = fallback disposition (R3). `t` transient, `p` permanent, `a` action-required,
`i` indeterminate.

| Error · variant | code | disp |
|---|---|---|
| `AddLimitOrderError::{AmountExceedsMaximum, UnknownTradingPair, InvalidPrice, InvalidQuantity}` | 422 | p |
| `AddLimitOrderError::InsufficientBalance` | 402 | a |
| `CancelLimitOrderError::OrderNotFound` | 404 | p |
| `CancelLimitOrderError::NotOrderOwner` | 403 | p |
| `CancelLimitOrderError::{OrderAlreadyFilled, OrderAlreadyCanceled}` | 409 | p |
| `CancelLimitOrderError::InvalidOrderId` | 400 | p |
| `DepositError::{AmountExceedsMaximum, UnsupportedToken}` | 422 | p |
| `DepositError::OperationInProgress` | 503 | t |
| `DepositError::CallFailed` | 504 | i |
| `DepositError::LedgerError(InsufficientFunds \| InsufficientAllowance)` | 402 | a |
| `DepositError::LedgerError(TemporarilyUnavailable)` | 503 | t |
| `DepositError::LedgerError(InternalError)` | 500 | t |
| `WithdrawError::{AmountExceedsMaximum, UnsupportedToken, AmountTooSmall}` | 422 | p |
| `WithdrawError::InsufficientBalance` | 402 | a |
| `WithdrawError::OperationInProgress` | 503 | t |
| `WithdrawError::CallFailed` | 504 | i |
| `WithdrawError::LedgerError(InsufficientFunds)` | 500 | t (DEX-side inconsistency) |
| `WithdrawError::LedgerError(TemporarilyUnavailable)` | 503 | t |
| `WithdrawError::LedgerError(InternalError)` | 500 | t |
| `GetOrderStatusError::InvalidOrderId` | 400 | p |
| `GetOrderBookTickerError::UnknownTradingPair` | 422 | p |
| `GetOrderBookDepthError::{UnknownTradingPair, LimitTooLarge}` | 422 | p |
| `AddTradingPairError::NotController` | 403 | p |
| `AddTradingPairError::TradingPairAlreadyExists` | 409 | p |
| `AddTradingPairError::*` (remaining validation) | 422 | p |
| `GetBalancesError::TokenNotSupported` | 422 | p |
| `GetBalancesRequestError::FilterTooLarge` | 422 | p |

Note the deliberate `InsufficientFunds` asymmetry: deposit = 402 (caller's external wallet is
short — caller action fixes it), withdraw = 500 (DEX accounting says funds exist but the ledger
disagrees — internal inconsistency, retry-after-reconcile).

`OperationInProgress` uses **503** (transient, retry with backoff), not 409, so the leading digit
alone tells an unrecognizing client to retry. **409** is reserved for *permanent* conflicts
(`OrderAlreadyFilled`, `OrderAlreadyCanceled`, `TradingPairAlreadyExists`), whose 4xx fallback
correctly says do-not-auto-retry.

### Test plan

Unit (`libs/types/src/tests.rs`):
- For every variant of every error enum, assert `code()` equals the value in
  [Code assignments](#code-assignments) — parameterized, no copy/paste. (**R4**)
- Assert `ErrorInfo::from(e) == ErrorInfo { code: e.code(), detail: Some(e) }`. (**R2**)
- Assert every assigned code's leading digit matches its documented disposition class. (**R3**)
- Forward-compat decode test: encode a value of an enum that has an *extra* variant and decode it
  into the shipped (smaller) type; assert `detail` decodes to `None` while `code` decodes intact.
  (**R5**, **R6**)

Unit (`canister/src/.../tests.rs`, sibling files):
- `cancel_limit_order` with a malformed id ⇒ `InvalidOrderId`, not `OrderNotFound`. (**R7**)
- `get_order_status` with a malformed id ⇒ `Err(InvalidOrderId)` and does not panic; well-formed
  unknown ⇒ `Ok(NotFound)`; well-formed known ⇒ `Ok(<status>)`. (**R8**)

Integration (`dex_int_tests`):
- Update existing deposit/withdraw/add/cancel error assertions to the envelope shape; assert both
  `code` and `detail` for at least one variant per endpoint. (**R1**)
- New: cancel + `get_order_status` malformed-id cases over the canister boundary, asserting no
  trap and the expected `code`. (**R7**, **R8**)

Interface:
- `check_candid_interface_compatibility` passes against the updated `dex.did`. (**R9**)

Commands: `cargo test --workspace`, `cargo fmt --all -- --check`, `just lint`.

### Delivery / PR sequence

Stacked, bottom-to-top; each compiles and tests independently. PR2 and PR3 each depend only on
PR1.

1. **PR1 — Error envelope + the four update-endpoint errors.**
   `ErrorInfo<E>`, `ErrorCode`, and `code()` for `AddLimitOrderError`, `CancelLimitOrderError`,
   `DepositError`, `WithdrawError`; wrap `add_limit_order`, `cancel_limit_order`, `deposit`,
   `withdraw`; `dex.did` (envelopes + disposition doc block); unit + integration tests.
   *Accepts*: R1 (these four), R2, R3, R4 (these four), R5, R6, R9.
2. **PR2 — Extend the envelope to query/admin errors.**
   `code()` + wrapping for `GetOrderBookTickerError`, `GetOrderBookDepthError`,
   `AddTradingPairError`, `GetBalancesError`, `GetBalancesRequestError`; `dex.did`; tests.
   *Accepts*: R1 (remainder), R4 (remainder), R9.
3. **PR3 — Stop conflating / trapping on malformed order IDs.**
   Add `CancelLimitOrderError::InvalidOrderId` (map cancel parse failure to it); add
   `GetOrderStatusError` and make `get_order_status` return a non-panicking result; `code()` for
   both new variants; `dex.did`; unit + integration tests.
   *Accepts*: R7, R8 (and R4 for the two new variants, R9).

## Discussed Alternatives

1. **Keep bare enums, document retryability** (the Binance/Coinbase norm). Rejected: gives no
   machine-readable signal; every multi-language client re-derives the map; doesn't satisfy the
   ticket's "an API that allows the caller to differentiate."
2. **Rust `is_retryable()` / `category()` helper on the shared crate, no wire change.** Rejected:
   serves only Rust callers; our consumers are multi-language and uncoordinated with DEX versions.
3. **Closed `variant { Transient; Permanent }` category on the wire** (or a `record { category;
   error }` wrapper with a closed category). Rejected: a variant in a *return* position cannot
   gain cases backward-compatibly (old clients trap on the unknown tag), and the disposition space
   is already ≥3 (transient, permanent, indeterminate) and likely to grow (action-required, a
   future admin-gated `Unauthorized`).
4. **Bare `opt variant` category** (lean on the forgiving-`opt` rule for the category itself).
   Rejected: when it decodes to `null` on an old client, the *coarse* signal is lost too; putting
   the disposition in a non-opt `code` field keeps it readable even when `detail` is `null`.
5. **Flat `record { code : nat16; message : text }`** (Binance/Coinbase-Pro style). Rejected:
   discards the typed, structured payloads that on-chain callers consume; degrades machine-readable
   detail to a human string.
6. **Exact HTTP codes everywhere, or a bespoke Binance-style numeric scheme.** Partially adopted:
   borrow the leading-digit *class* contract as the only stable guarantee, reuse familiar HTTP
   codes where they fit, but don't commit to strict IANA semantics for every domain error.
