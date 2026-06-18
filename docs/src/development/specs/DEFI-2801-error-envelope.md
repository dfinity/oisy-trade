---
id: DEFI-2801
title: Consolidate user-facing errors into forward-compatible, disposition-tagged variants
tags: [errors, candid, api]
---

# Consolidate user-facing errors into forward-compatible, disposition-tagged variants

## Motivation

Today every fallible endpoint returns a bare, flat error variant (`DepositError`, `WithdrawError`,
`AddLimitOrderError`, …). A caller that wants to decide *whether to retry* must enumerate every
variant and hard-code the mapping itself; the DEX gives it no machine-readable signal. Some failures
are the caller's to fix (`UnsupportedToken`, `InvalidPrice`, `InsufficientFunds`), some are transient
(`OperationInProgress`, a ledger that is `TemporarilyUnavailable`, a global `TradingHalted`), and some
are the DEX's own fault (`LedgerInternalError`, an accounting/ledger inconsistency).

The consumers here are **multiple independent clients, written in different languages, that are not
upgraded in lockstep with the DEX**. That rules out a Rust-only helper and makes Candid
forward-compatibility a hard requirement: a client built against today's interface must keep working —
without traps and without silently mishandling — after the DEX adds a new error case tomorrow.

Each error becomes a small record: a **disposition** (what the caller should do) carried as a variant,
plus an advisory free-text **`message`**:

- **`RequestError`** — caller-side; the request will not succeed as-is. Correct the input, satisfy a
  precondition (fund / approve), or stop. Do **not** auto-retry unchanged.
- **`TemporaryError`** — transient; retry the same call after a backoff.
- **`InternalError`** — DEX-side fault; surface to operators. Do **not** retry.

Candid skeleton (every user-facing error follows this shape; `DepositError` shown):

```candid
type DepositError = record {
  kind : variant {
    RequestError : opt variant {
      AmountExceedsMaximum;
      UnsupportedToken      : record { token_id : TokenId };
      InsufficientFunds     : record { balance : nat };
      InsufficientAllowance : record { allowance : nat };
    };
    TemporaryError : opt variant {
      OperationInProgress;
      LedgerTemporarilyUnavailable;
      CallFailed : record { ledger : principal; method : text; reason : text };
    };
    InternalError : opt variant { LedgerError : record { reason : text } };
  };
  message : opt text;  // advisory; branch on `kind` + leaf, never parse `message`
};
```

Arms an endpoint can't produce are still declared, as an empty `opt variant {}` (R1). See
[Disposition membership](#disposition-membership) for the per-endpoint leaves.

Two adjacent input-handling bugs are folded in because they live in the same surface: `cancel_limit_order`
maps a *malformed* `order_id` to `OrderNotFound` (conflating bad input with a missing order), and
`get_my_orders` **traps** on a malformed `order_id` (user input can panic the canister).

## Requirements

- **R1**: Every user-facing error is a `record { kind : variant { … }; message : opt text }`, where `kind` is a
  disposition variant whose arms are drawn from `RequestError` / `TemporaryError` / `InternalError`,
  each carrying `opt variant { … }` of its specific leaves. Every error declares **all three** arms;
  an arm it cannot currently produce carries an empty `opt variant {}`, reserving the slot so that
  leaves can be added to any arm later without breaking clients (and an arm is never *added*).
  Applies to `add_limit_order`, `cancel_limit_order`, `deposit`, `withdraw`, `get_my_orders`,
  `get_order_book_ticker`, `get_order_book_depth`, and both the per-token and request-level errors of
  `get_balances` / `get_fee_balances`. Admin endpoints are out of scope.
- **R2**: The `kind` arm is the contract (documented in `oisy_trade.did`):
  `RequestError` ⇒ caller-side, do not auto-retry unchanged; `TemporaryError` ⇒ retry after backoff;
  `InternalError` ⇒ DEX-side fault, surface, do not retry.
- **R3**: `message` is **advisory** human-readable text. Its purpose is the forward-compat case: when a
  client hits an error it cannot decode — a future leaf that decodes to `null`, or a reserved/empty arm
  — `message` still gives operators/UI something actionable and signals that the client should be
  updated. The canister populates it for every error from the underlying leaf's `Display` /
  `to_string()`; clients **must not** parse it — programmatic
  handling is on `kind` and the inner leaf only.
- **R4**: Each leaf error is assigned to exactly one arm per [Disposition membership](#disposition-membership).
- **R5**: Each arm's payload is `opt variant`. A client built against an older interface decodes an
  unknown future *leaf* as `null`, while still reading the **arm** (`kind`) and `message`. (Verified by
  a decode test feeding a superset-leaf value into the shipped type — inner `null`, arm + message intact.)
- **R6**: `CallFailed` is a `TemporaryError`, not indeterminate — see D3. Holds only while the ledger
  calls are guaranteed-response (`call_unbounded_wait`); see Constraints.
- **R7**: `cancel_limit_order` with a malformed `order_id` returns `RequestError(InvalidOrderId)`,
  distinct from `RequestError(OrderNotFound)`.
- **R8**: `get_my_orders` never traps. Its signature changes from `-> (vec UserOrder)` to a result
  `-> (variant { Ok : vec UserOrder; Err : GetMyOrdersError })`. A malformed `order_id` returns
  `Err(RequestError(InvalidOrderId))`; a well-formed but unknown id
  returns `Ok([])`; otherwise `Ok(<orders>)`. (This is a breaking signature change to a query.)
- **R9**: The hand-written `canister/oisy_trade.did` matches the generated interface
  (`check_candid_interface_compatibility` passes) and documents the R2 disposition contract and the R3
  `message` rule.

## Non-goals

- **No fourth disposition arm.** No `Indeterminate`/`Reconcile` (see D3) and no split of `RequestError`
  into finer "fix the request / satisfy a precondition / stop" — those distinctions are carried by the
  inner leaf and don't change the coarse client action. The three arms are the complete, frozen
  partition of caller actions.
- **`message` is not a wire contract.** It exists for humans/operators; the spec forbids clients from
  matching on it (R3). It is not localized and its exact text may change.
- **Admin endpoints are out of scope** (e.g. `add_trading_pair`): controller-only, not part of the
  multi-language client surface this targets.
- **Deferred to a follow-up PR**: reshaping `get_my_orders` to the `get_balances` pattern (outer result
  + per-item inner results, via a `ByIds : vec OrderId` selector). This PR keeps the single-`Result`
  `get_my_orders` (R8); the batch/per-item form is future work.
- **No changes to internal/state-layer error types** (`canister/src/state`, `order`, `lib`,
  `ledger` — incl. the internal `GetMyOrdersError` / `OrderIdParseError`) beyond mapping them to the
  disposition-tagged public types at the boundary.
- **No change to which errors are logged.** The `main.rs` per-error logging arms encode
  *log-worthiness*, deliberately not the disposition. Left untouched.
- **Accepted residual limitations**:
  - A client hitting a *future leaf* sees inner `null` and loses the typed reason, but keeps the
    disposition arm and the `message` — the intended trade.
  - All three arms are declared on every error from the start (R1), so an arm is never *added*; only a
    hypothetical *fourth* disposition would be breaking. Accepted, because the three arms exhaustively
    partition what a caller can do. (Adding *leaves* to any arm — including a currently-empty one — is
    forward-compatible via the inner `opt`.)

## Design Decisions

- **D1 — `record { kind : variant {…}; message : opt text }`.** The disposition is a typed, self-
  documenting variant (`kind`); the specific reason is an inner `opt variant`; `message` is advisory
  text. This separates *what grows* (specific reasons → inner `opt`) from *what's stable* (the small set
  of caller actions → bare outer arm), and the record gives field-level headroom (a future
  `retry_after` etc. is a non-breaking field add).
- **D2 — `InvalidOrderId` is bare; the leaf name carries the meaning.** Its internal `OrderIdParseError`
  is not a Candid type and carries no dynamic data, so there is nothing to put on the wire — the leaf
  name is self-describing. (`message` is not a payload substitute; see R3 for its purpose.)
- **D3 — No `Indeterminate`/reconcile arm; `CallFailed` ⇒ `TemporaryError`.** Both ledger calls use
  `call_unbounded_wait` (guaranteed response) and ICRC `icrc1_transfer` / `icrc2_transfer_from` commit
  atomically with their reply, so a reject implies the transfer did **not** commit — no side effect on
  either side — making the operation safe to retry. Nothing to reconcile.
- **D4 — Naming `RequestError / TemporaryError / InternalError`.** Symmetric `-Error` triple,
  attribution-clear (your *request* / *transient* / the DEX's *internals*), IC-native — no `Server` and
  no `Caller`/`Callee` jargon.
- **D5 — The `InsufficientFunds` asymmetry.** Deposit `InsufficientFunds` (caller's external wallet) ⇒
  `RequestError`; withdraw's ledger-reported `InsufficientFunds` (DEX accounting says it has the funds,
  the ledger disagrees) ⇒ `InternalError` — a genuine invariant violation.
- **D6 — Malformed `order_id` ⇒ a bare `InvalidOrderId` leaf under `RequestError`**, on
  `cancel_limit_order` and `get_my_orders` (`get_order_status` was removed in #133). `get_my_orders`
  becomes non-trapping by returning a result (R8).
- **D7 — Enforce the shape with a generic `Error<Request, Temporary, Internal>`.** Every public error is
  an instantiation of one generic struct (`{ kind: ErrorKind<…>, message }`), so the three-arm shape is
  structurally identical across the whole surface and can't drift. The `impl` bounds each leaf on
  `std::error::Error` and derives `message` from the leaf's `to_string()`, so the human text is produced
  uniformly rather than hand-set per call site.

## Implementation

### Constraints

- Both ledger calls are guaranteed-response: `icrc2_transfer_from` and `icrc1_transfer` use
  `call_unbounded_wait` (`canister/src/ledger/mod.rs`). **D3/R6 depend on this.** `ledger/mod.rs` carries
  `TODO(DEFI-2745): Consider switching to bounded_wait` — if that lands, best-effort timeouts become
  genuinely indeterminate and `CallFailed` must move out of `TemporaryError` into a reconcile-style
  disposition (reintroducing a fourth arm).
- `dex_types::OrderId = String`, parsed to `canister::order::OrderId` via `FromStr` (`OrderIdParseError`,
  an internal non-Candid unit struct). Parse points: `cancel_limit_order`, `get_my_orders`.
- `get_my_orders` currently returns `-> (vec UserOrder)` and the entry point (`canister/src/main.rs`)
  `panic!`s on the internal `GetMyOrdersError::InvalidOrderId(OrderIdParseError)`. R8 turns that into a
  returned, disposition-tagged public error.
- `check_candid_interface_compatibility` (`canister/src/main.rs`) pins `oisy_trade.did` to the generated
  interface via `service_equal`; every interface change updates `oisy_trade.did` by hand.
- Candid's forgiving `opt` decode rule provides inner-leaf forward-compatibility; only clients generated
  from the updated `.did` benefit.

### `dex_types` (`libs/types/src/lib.rs`)

A single **generic** shape enforces the structure for every user-facing error (D7); each endpoint
instantiates it with three leaf enums:

```rust
pub struct Error<Request, Temporary, Internal> {
    pub kind: ErrorKind<Request, Temporary, Internal>,
    pub message: Option<String>,   // advisory; clients must not parse (R3)
}
pub enum ErrorKind<Request, Temporary, Internal> {
    RequestError(Option<Request>),
    TemporaryError(Option<Temporary>),
    InternalError(Option<Internal>),
}

// Per-endpoint instantiation (DepositError shown; the rest follow):
pub type DepositError = Error<DepositRequestError, DepositTemporaryError, DepositInternalError>;
pub enum DepositRequestError { AmountExceedsMaximum, UnsupportedToken { token_id: TokenId },
                               InsufficientFunds { balance: Nat }, InsufficientAllowance { allowance: Nat } }
pub enum DepositTemporaryError { OperationInProgress, LedgerTemporarilyUnavailable,
                                 CallFailed { ledger: Principal, method: String, reason: String } }
pub enum DepositInternalError { LedgerError { reason: String } }
```

- The `impl` block bounds `Request: Error`, `Temporary: Error`, `Internal: Error` (each leaf enum
  implements `std::error::Error`), and the constructors set `message` from the underlying leaf's
  `to_string()` — e.g. `Error::request(leaf)` ⇒ `{ kind: RequestError(Some(leaf)), message: Some(leaf.to_string()) }`.
- Arms an endpoint can't produce are instantiated with an **empty leaf enum** (e.g.
  `enum AddLimitOrderInternalError {}`, or a shared uninhabited type), rendering as an empty
  `opt variant {}` (R1).
- Candid renders each `Error<…>` instantiation structurally as
  `record { kind : variant { RequestError : opt variant {…}; TemporaryError : opt variant {…}; InternalError : opt variant {…} }; message : opt text }`.

The internal→public conversions (`canister/src/state`, `lib`, `ledger`) and construction sites build
these via the constructors; internal flat enums are untouched (Non-goals).

The order-id fix adds a **public** `GetMyOrdersError = Error<GetMyOrdersRequestError, ∅, ∅>` (the
internal `GetMyOrdersError` in `canister/src/lib.rs` stays internal): a single `RequestError` arm with a
bare `InvalidOrderId` leaf and empty Temporary/Internal arms. The internal
`InvalidOrderId(OrderIdParseError)` maps to the bare public `InvalidOrderId` leaf.

### Disposition membership

All errors declare all three arms (R1); a cell marked `(none)` is a declared-but-empty
`opt variant {}`, reserved so leaves can be added there later without breaking clients.

| Error | `RequestError` | `TemporaryError` | `InternalError` |
|---|---|---|---|
| **DepositError** | `AmountExceedsMaximum`, `UnsupportedToken`, `InsufficientFunds`, `InsufficientAllowance` | `OperationInProgress`, `LedgerTemporarilyUnavailable`, `CallFailed` | `LedgerError`, `CandidDecodeFailed`<sup>3</sup> |
| **WithdrawError** | `AmountExceedsMaximum`, `AmountTooSmall`, `UnsupportedToken`, `InsufficientBalance` | `OperationInProgress`, `LedgerTemporarilyUnavailable`, `CallFailed`, `LedgerFeeChanged`<sup>4</sup> | `LedgerError`, `LedgerInsufficientFunds`<sup>1</sup>, `CandidDecodeFailed`<sup>3</sup> |
| **AddLimitOrderError** | `AmountExceedsMaximum`, `UnknownTradingPair`, `InvalidPrice`, `InvalidQuantity`, `InsufficientBalance`, `InvalidNotional` | `TradingHalted`<sup>2</sup> | (none) |
| **CancelLimitOrderError** | `InvalidOrderId`, `OrderNotFound`, `NotOrderOwner`, `OrderAlreadyFilled`, `OrderAlreadyCanceled` | (none) | (none) |
| **GetMyOrdersError** (new public) | `InvalidOrderId` | (none) | (none) |
| **GetOrderBookTickerError** | `UnknownTradingPair` | (none) | (none) |
| **GetOrderBookDepthError** | `UnknownTradingPair`, `LimitTooLarge` | (none) | (none) |
| **GetBalancesError** | `TokenNotSupported` | (none) | (none) |
| **GetBalancesRequestError** | `FilterTooLarge` | (none) | (none) |

<sup>1</sup> withdraw's ledger-reported `InsufficientFunds` (D5). <sup>2</sup> `TradingHalted` (DEFI-2849) — a global halt
is intentional transient unavailability ("retry when trading resumes", like a ledger
`TemporarilyUnavailable`); `InvalidNotional` (DEFI-2850) is caller-side input → `RequestError`.
`TradingHalted`'s placement is a judgment call worth a second look.
<sup>3</sup> `CandidDecodeFailed` — the ledger replied but the response failed to Candid-decode (a DEX-side
type/version mismatch); since the call may have executed, it's an `InternalError` (surface, don't blindly
retry), distinct from `CallFailed` (the call itself failed with no/rejected response → transient).
<sup>4</sup> `LedgerFeeChanged` — the ledger fee changed between fetch and transfer so nothing was applied;
rare and safe to retry → `TemporaryError`.

### Canister logic (`canister/src/lib.rs`, `main.rs`)

- `cancel_limit_order`: map `OrderId` parse failure to a bare `RequestError(InvalidOrderId)` (was `OrderNotFound`).
- `get_my_orders`: return `Result<Vec<UserOrder>, GetMyOrdersError>` (public); the `main.rs` entry point
  returns `Err(RequestError(InvalidOrderId))` instead of `panic!`; well-formed unknown id ⇒ `Ok(vec![])`.

### Candid (`canister/oisy_trade.did`)

- Top-of-file comment documenting the R2 disposition contract and the R3 `message` rule.
- Each error renders as `record { kind : variant { RequestError : opt variant {…}; TemporaryError : opt variant {…}; InternalError : opt variant {…} }; message : opt text }`,
  declaring all three arms — uninhabited ones as an empty `opt variant {}` (R1).
- `get_my_orders` signature becomes a result; new `GetMyOrdersError`; new bare `InvalidOrderId` leaf on
  `CancelLimitOrderError`.

### Test plan

Unit (`libs/types/src/tests.rs`):
- For every leaf, assert the internal→public conversion places it under the membership-table arm and
  sets a non-empty `message` — parameterized. (**R2**, **R3**, **R4**)
- Forward-compat decode test: encode an error whose inner arm has an *extra* leaf, decode into the
  shipped type; assert inner `null` while `kind` and `message` decode intact. (**R5**)

Unit (`canister/src/.../tests.rs`):
- `cancel_limit_order` malformed id ⇒ `RequestError(InvalidOrderId)`, not `OrderNotFound`. (**R7**)
- `get_my_orders` malformed id ⇒ `Err(RequestError(InvalidOrderId))`, no panic; unknown id ⇒ `Ok([])`. (**R8**)

Integration (`dex_int_tests`): update existing assertions to the `{ kind; message }` shape, asserting the
arm + inner leaf for at least one case per endpoint; new cancel + `get_my_orders` malformed-id cases over
the boundary, asserting no trap. (**R1**, **R7**, **R8**)

Interface: `check_candid_interface_compatibility` passes against the updated `oisy_trade.did`. (**R9**)

Commands: `cargo test --workspace`, `cargo fmt --all -- --check`, `just lint`.

### Delivery / PR sequence

Stacked, bottom-to-top; each compiles and tests independently. PR2 and PR3 each depend only on PR1.

1. **PR1 — Disposition-tagged `{ kind; message }` errors for the four update-endpoint errors.**
   Shape + leaf enums for `AddLimitOrderError`, `CancelLimitOrderError`, `DepositError`, `WithdrawError`;
   map internal→public at the boundary (incl. `message`); `oisy_trade.did` + contract doc block; tests.
   *Accepts*: R1 (these four), R2, R3, R4, R5, R9.
2. **PR2 — Extend to query errors.** Same shape for `GetOrderBookTickerError`, `GetOrderBookDepthError`,
   `GetBalancesError`, `GetBalancesRequestError`; `oisy_trade.did`; tests. *Accepts*: R1 (remainder), R4
   (remainder), R9.
3. **PR3 — Stop conflating / trapping on malformed order IDs.** Bare `InvalidOrderId` on
   `CancelLimitOrderError`; new public `GetMyOrdersError`; `get_my_orders` returns a result (no `panic!`);
   `oisy_trade.did`; tests. *Accepts*: R7, R8, R9.

## Discussed Alternatives

1. **Keep bare flat enums, document retryability** (Binance/Coinbase norm). Rejected: no machine-readable
   signal; every multi-language client re-derives the map.
2. **Rust `is_retryable()`/`category()` helper, no wire change.** Rejected: serves only Rust callers; ours
   are multi-language and uncoordinated.
3. **Numeric `record { code : nat16; detail : opt E }` envelope** (HTTP-status-class codes). Seriously
   considered; rejected for typed disposition tags — clearer for multi-language clients (no code-range →
   meaning doc dependency), avoids the "409 is a 4xx but retryable" tension. Note the chosen shape is
   `record { kind : variant {…}; message }`, i.e. a *typed* disposition, not a numeric code.
4. **Closed flat `variant { Transient; Permanent }` category, or a closed `{ category; error }` wrapper.**
   Rejected: can't gain cases compatibly and the detail loses forward-compatibility; the inner `opt
   variant` solves the latter.
5. **A fourth `Indeterminate`/`Reconcile` arm for `CallFailed`.** Rejected once both ledger calls were
   confirmed guaranteed-response (D3). Revisit if DEFI-2745 switches to bounded-wait.
6. **Finer caller-side arms / verb arms** (`Fix`/`Resolve`/`Stop`, `BadRequest`/`UnprocessableRequest`).
   Rejected: they split on "what's wrong," not "what to do"; the action is the same and the distinction
   belongs in the inner leaf.
7. **Omit the `message` field** (the spec's original stance). Reversed on review: when an old client hits
   a future leaf, `detail` is `null` and `kind` alone is thin for diagnostics — an advisory `message`
   aids logs/UI and flags "update your client," at the cost of a record wrapper (which also buys
   field-level forward-compat). Clients still match only on `kind`/leaf (R3).
8. **Carry `OrderIdParseError` in `InvalidOrderId`.** Rejected: it's an internal, non-Candid unit struct
   with no dynamic data; the bare leaf name is self-describing, so nothing is lost on the wire.
