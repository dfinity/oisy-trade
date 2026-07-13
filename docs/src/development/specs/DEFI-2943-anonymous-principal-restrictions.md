---
id: DEFI-2943
title: Enforce canister-wide restrictions on the anonymous principal
tags: [security, permissions, api]
---

# Enforce canister-wide restrictions on the anonymous principal

## Motivation

`Principal::anonymous()` is the shared, unauthenticated identity: every caller who has
not authenticated arrives as the same principal. The oisy-trade canister currently
places **no restriction on it** anywhere in non-test source — `deposit`, `withdraw`,
`add_limit_order`, the per-user read endpoints, and the DEFI-2911 trading-account grant
(`validate_add_trading_account`) all accept it, gated only by restricted-mode
allowlisting and the permit layer. Treating a shared identity as a real user/account has
security and privacy implications across the DEX.

This surfaced during review of [DEFI-2911](./DEFI-2911-funding-and-trading-accounts.md)
PR 4 (resolution on reads, [#219](https://github.com/dfinity/oisy-trade/pull/219),
Copilot [discussion_r3558375014](https://github.com/dfinity/oisy-trade/pull/219#discussion_r3558375014)):
a funding account can whitelist `Principal::anonymous()` as a trading account, and once
read-resolution is live **any unauthenticated caller resolves to that funding account
and can read its balances / orders / trades** — a data-privacy leak. The DEFI-2911
review decided this is broader than the trading-account feature and should be handled
canister-wide, here.

### Two constraints shape the rollout

1. **Do not brick the canister.** The anonymous principal is a shared free-for-all: its
   DEX balance can be topped up (`deposit` pulls `from: caller`) or drained (`withdraw`
   sends `to: caller`) by anyone calling as anonymous. If it currently holds a balance
   or open orders, forbidding `withdraw` / `cancel_limit_order` outright would **strand
   those funds forever**. So enforcement is split by direction:
   - **Entry / accumulation** ops (`deposit`, `add_limit_order`, being whitelisted as a
     trading account) — forbidding these can *never* strand anything, only stop new
     state. Blocked immediately (Phase 1).
   - **Exit / read** ops (`withdraw`, `cancel_limit_order`, `get_balances` /
     `get_my_orders` / `get_my_trades`, plus `remove_trading_account`) — forbidding
     these strands residual holdings and removes the operator's own way to observe them.
     Kept open until the anonymous balance is swept to zero and verified, then locked in
     a follow-up upgrade (Phase 2).
2. **The leak is closed in Phase 1 regardless.** The privacy leak needs anonymous to be
   *whitelisted as a trading account*. Rejecting that grant (R2) means an anonymous
   caller only ever resolves to itself, so keeping anonymous reads open during the sweep
   window costs nothing on the security goal. Read-resolution (#219) is also not yet on
   `main`, so the leak is not currently exploitable.

## Requirements

### Phase 1 — block accumulation, preserve exits (this ticket)

- **R1 — Entry ops reject an anonymous caller.** `deposit`, `add_limit_order`, and
  `add_trading_account` reject a caller equal to `Principal::anonymous()` with a typed,
  request-level error, returned before any state mutation or ledger interaction.
- **R2 — Grant rejects an anonymous trading account.** `add_trading_account(T)` rejects
  `T == Principal::anonymous()` with a typed request-level error. This is independent of
  R1: `T` is request data, not the caller, so R1 never inspects it. Extends the
  DEFI-2911 R7 grant preconditions, and is what closes the read-privacy leak at its
  source.
- **R3 — Exit / read ops stay open to anonymous.** `withdraw`, `cancel_limit_order`,
  `remove_trading_account`, `get_balances`, `get_my_orders`, `get_my_trades`, and
  `get_my_trading_accounts` continue to accept the anonymous caller in Phase 1, so any
  residual anonymous balance / orders can be unwound, swept out, and observed. (An
  explicit non-gating requirement — these must *not* be blocked yet.)

### Cross-cutting (all phases)

- **R4 — Open endpoints unaffected.** Endpoints carrying no per-user identity keep
  serving all callers, including anonymous: `get_trading_pairs`,
  `get_order_book_ticker`, `get_order_book_depth`, `list_supported_tokens`, and
  `get_fee_balances` (canister-global fee data, not per-user).
- **R5 — Internal matching path unaffected.** `process_pending_orders` / `drive_matching`
  (the periodic matching timer and post-`add_limit_order` kickoff) take no caller and
  never pass through the anonymous checks; they run regardless.
- **R6 — Enforcement is ingress-only; replay is untouched.** The anonymous checks exist
  solely on the live-call path (endpoint entry + `validate_add_trading_account`).
  `apply_state_transition` and every `record_*` method stay unconditional, so **every
  historical event referencing the anonymous principal — a deposit, an order, even an
  anonymous trading-account grant — replays unchanged**. The checks must never be added
  to the replay path.
- **R7 — Typed, not trapping.** Every rejection under R1, R2 (and later R8) is a typed
  `RequestError` leaf on the endpoint's error envelope
  ([DEFI-2801](./DEFI-2801-error-envelope.md) model), never a trap. Restricted-mode
  enforcement (`assert_caller_is_allowed`, which traps) is a separate, unchanged
  mechanism.

### Phase 2 — complete the lockout (follow-up upgrade)

- **R8 — Lock the exit / read ops once anonymous is empty.** After operational
  verification (see Rollout) that the anonymous principal holds **zero balance, zero
  open orders, and no residual trading-account grant state**, a follow-up upgrade
  extends the R1-style anonymous caller-rejection to the endpoints held open by R3
  (`withdraw`, `cancel_limit_order`, `remove_trading_account`, `get_balances`,
  `get_my_orders`, `get_my_trades`, `get_my_trading_accounts`), making anonymous fully
  inert. Tracked as a follow-up ticket; **not built by this spec** until verification
  confirms it is safe.

## Non-goals

- **Restricted-mode semantics.** `assert_caller_is_allowed` keeps trapping on a
  restricted-mode violation; the anonymous gate is orthogonal and always on, in every
  mode.
- **Controller/admin auth.** `add_trading_pair`, `halt_trading`, `resume_trading` are
  controller-gated; anonymous is not a controller, so they already reject it with
  `NotController`. No change.
- **Read-resolution itself.** Resolving a trading account to its funding account on
  reads is DEFI-2911 PR 4 (#219). This gate is independent and lands on `main` without
  depending on it.
- **A controller sweep endpoint.** None exists and none is added; the anonymous balance
  is drained through the normal (open, in Phase 1) `withdraw` path.
- **Ingress-level (`canister_inspect_message`) rejection.** Rejected as insufficient —
  see Discussed Alternatives.

## Design Decisions

- **Gate at the endpoint entry, not at `FundingAccount` / `TradingAccount`
  construction.** Those newtypes exist only on the grant path
  (`canister/src/user/mod.rs`), so a construction-site check would miss `deposit`,
  `add_limit_order`, and every read endpoint. The newtypes are also reconstructed on the
  event-replay / audit path and in test fixtures, where a hard reject would violate R6.
  And the funding side is always the caller, already covered by the entry gate.
- **Split enforcement by direction (entry vs. exit).** The safety property is: never
  block the only way funds leave before confirming there are none. See Motivation.
- **Typed request error, not a trap.** Consistent with the DEFI-2801 envelope: clients
  get a structured, testable rejection.
- **Two concerns, two checks.** R1 gates *caller identity*; R2 validates *request data*
  (the `trading` principal). The caller gate structurally cannot cover the `trading`
  argument.

## Implementation

### Constraints

- Endpoint error types follow the `Error<Request, Temporary, Internal>` envelope; each
  anonymous rejection is a new `Request` leaf.
- Phase 1 touches only entry endpoints, so the read-side error wrinkles are deferred to
  Phase 2. For completeness they are: `GetMyTradingAccountsError` is currently
  `Error<Never, Never, Never>` (needs a real `GetMyTradingAccountsRequestError`), and
  `get_my_orders` / `get_my_trades` return an *internal* enum mapped to the public
  envelope in `main.rs` (so their Phase-2 check belongs in `main.rs`, leaving the
  internal enums unchanged).
- Adding variants to returned error variants changes the candid interface; the `.did`
  must be regenerated and the backward-compat CI check must pass.

### `oisy_trade_types` (error envelopes)

Phase 1 adds an `AnonymousCaller` request-error variant (with a `thiserror` message) to
`DepositRequestError`, `AddLimitOrderRequestError`, and `AddTradingAccountRequestError`.
For R2, add a dedicated `AddTradingAccountRequestError::AnonymousTradingAccount` variant
(preferred over folding into `InvalidTradingAccount` — anonymous is a distinct,
security-relevant reason).

### `canister` (enforcement points)

- A small helper — e.g. `fn reject_anonymous(caller: Principal) -> Result<(), AnonymousCaller>`
  returning a private marker — keeps the `Principal::anonymous()` comparison in one
  place; each endpoint `.map_err`s it into its own `AnonymousCaller` request variant.
- Phase 1 call sites (`lib.rs`): `deposit`, `add_limit_order`, `add_trading_account` at
  entry, alongside the existing `assert_caller_is_allowed`.
- R2: extend `UserRegistry::validate_add_trading_account` (`canister/src/user/mod.rs`) —
  reject `trading == Principal::anonymous()` with a new `GrantError` variant, mapped to
  `AddTradingAccountRequestError::AnonymousTradingAccount`.
- **No change** to `apply_state_transition` / `record_*` (R6), the matching paths (R5),
  the exit/read endpoints (R3), or the open endpoints (R4).

### Delivery / PR sequence

Phase 1 — two stacked PRs on `main` (base `main`; branch prefix `mathias/DEFI-2943-`).

- **PR 1/2 — Entry caller-gate.** `AnonymousCaller` variants on the three entry
  endpoints, the `reject_anonymous` helper, and the gate at `deposit`,
  `add_limit_order`, `add_trading_account`. Regenerates candid.
  - Covers: **R1**, **R3** (by leaving exit/read endpoints untouched), **R4**, **R5**,
    **R6**, **R7**.
- **PR 2/2 — Grant-specific anonymous trading account.** The `GrantError` /
  `AddTradingAccountRequestError::AnonymousTradingAccount` variant and the
  `trading == anonymous` check in `validate_add_trading_account`. Stacked on PR 1/2.
  - Covers: **R2** (and its slice of **R7**).

Phase 2 (**R8**) is a separate follow-up ticket, implemented only after the Rollout
verification below — not part of this delivery.

## Rollout & operational prerequisites

Before / around Phase 1 deployment (operational, not code):

1. **Check existing anonymous state on mainnet** by calling, as the anonymous principal,
   `get_balances`, `get_my_orders`, `get_my_trades`, and `get_my_trading_accounts`; and
   inspect whether anonymous is whitelisted as a trading account of any funding account
   (the leak source). These reads stay open in Phase 1 precisely so this is possible.
2. **Clean up before #219 merges.** If anonymous is already whitelisted as a trading
   account, revoke that grant (a one-time `remove_trading_account` by the owning funding
   account) so read-resolution never resolves anonymous to a real account.
3. **Sweep.** If anonymous holds a balance or open orders, cancel the orders and
   withdraw the balance (both open in Phase 1) until it is empty.
4. **Gate Phase 2 on verification.** Only after (1) confirms zero balance, zero open
   orders, and no residual grant state may the R8 upgrade ship.

## Discussed Alternatives

- **Check at `FundingAccount` / `TradingAccount` construction.** Rejected: misses
  `deposit` / `add_limit_order` / reads, and sits on the replay & fixture paths where a
  reject violates R6.
- **Immediate full lockout (block withdraw/cancel/reads now too).** Rejected: strands any
  balance or open orders the anonymous principal currently holds, and removes the
  operator's own means of verifying it is empty. Hence the entry/exit split and Phase 2.
- **Trap instead of a typed error.** Rejected per DEFI-2801; a trap is reserved for the
  restricted-mode operational guard.
- **Single `canister_inspect_message` ingress guard.** Rejected: `inspect_message` runs
  only for ingress *update* calls — not queries or inter-canister calls — so it cannot
  protect the read (query) endpoints, exactly where the privacy leak is. A per-endpoint
  gate covers queries too.
