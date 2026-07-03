---
id: DEFI-2911
title: Separate funding and trading accounts
tags: [accounts, permissions, security, api]
---

# Separate funding and trading accounts

## Motivation

Programmatic traders (feedback from G20, a market maker integrating with the DEX) must keep a
private key available at runtime to sign orders. Today the same principal both holds funds
(deposit / withdraw) and trades, so a leak of that hot key exposes the entire balance sitting on
the account — a growing concern as balances scale to hundreds of thousands and beyond.

The ask is a two-role model, explicitly compared to Hyperliquid's API/agent wallets:

- **Funding account** — an ordinary account: deposits, withdraws, holds balances, and may also
  trade.
- **Trading account** — a principal whitelisted by a funding account. It can only place and
  cancel orders *acting on the funding account's balance*; it cannot deposit or withdraw, and it
  never holds funds itself.

This isolates risk: a compromised trading key exposes only open trading positions (it can place
bad orders against the funding account's balance until revoked), never the ability to move funds
out. Every major venue offers this separation — CEXes via permission-scoped API keys, DEXes
(Hyperliquid, dYdX v4) via structurally restricted delegate keys (see
[Cross-exchange comparison](#cross-exchange-comparison)).

## Requirements

- **R1 — Grant.** A funding account `F` can whitelist a principal `T` via
  `add_trading_account`. From then on `T` may place and cancel orders acting on `F`'s account.
  `F` can revoke via `remove_trading_account`.
- **R2 — Orders act on the funding account.** An order placed by `T` is in every respect `F`'s
  order: validated against and reserved from `F`'s free balance, recorded with `owner = F`,
  visible in `F`'s `get_my_orders`, and its fills settle into `F`'s balances. After admission it
  is indistinguishable from an order `F` placed itself (order events carry `F`, so event-log
  replay is unaffected).
- **R3 — Funding operations denied.** `deposit` and `withdraw` called by a currently whitelisted
  `T` fail synchronously with a dedicated error, before any ledger interaction. Combined with the
  grant preconditions (R7), a trading account can never hold DEX balances.
- **R4 — Cancel authority.** `F` and any of `F`'s trading accounts may cancel `F`'s open orders.
  Any other principal — including a trading account of a *different* funding account — gets
  `NotOrderOwner`, exactly as today.
- **R5 — Reads resolve.** `get_balances`, `get_my_orders`, and `get_my_trades` called by `T`
  return `F`'s data, exactly as if `F` had called.
- **R6 — Revocation is immediate.** After `remove_trading_account(T)`, calls by `T` are treated
  as coming from an ordinary unknown principal. `F`'s open orders — including those `T` placed —
  are unaffected and stay open.
- **R7 — Grant preconditions.** `add_trading_account(T)` fails if any of:
  - `T == F` (self-grant);
  - `T` is already a trading account, of `F` or of anyone else — a trading account maps to
    **exactly one** funding account;
  - `T` is already a registered user (has a `UserId`, i.e. has ever deposited) — a trading key
    must be a fresh principal;
  - `F` is itself a trading account (no delegation chains);
  - `F` already has `MAX_TRADING_ACCOUNTS_PER_USER` trading accounts.
  `remove_trading_account(T)` fails if `T` is not currently `F`'s trading account.
- **R8 — Funding account unaffected.** `F` retains full authority — deposit, withdraw, trade,
  cancel, queries — regardless of how many trading accounts it has whitelisted.
- **R9 — Introspection.** `get_my_trading_accounts` returns the caller's current whitelist
  (empty for a principal with none). The whitelist-management surface (`add` / `remove` / `get`)
  never resolves delegation — it always acts on the raw caller as a funding account.
- **R10 — Persistence & audit.** The whitelist survives upgrade. Grants and revocations are
  recorded as events, visible via `get_events`, and replay-safe (no double-apply when the event
  log is replayed with stable-memory writes skipped).
- **R11 — Error envelope.** The new endpoints are non-trapping and return the DEFI-2801 error
  envelope (`docs/src/development/specs/DEFI-2801-error-envelope.md`); the deposit / withdraw
  denial is a new variant of the existing error envelopes.
- **R12 — Restricted-mode interplay.** Under `Mode::RestrictedTo`, the **raw caller** must be
  allowlisted; delegation does not bypass the mode check. A trading account needs its own entry
  in the restricted-mode allowlist to call anything.

## Non-goals

- **Expiry / TTL on grants.** Hyperliquid caps agent validity at 180 days; we ship
  revocation-only and note expiry as a follow-up if operational experience warrants it.
- **Scoped grants** — per-pair restrictions, notional caps, read-only keys (dYdX's
  `ClobPairIdFilter` / `MessageFilter` analogues). A trading account has all-or-nothing trading
  authority over the funding account.
- **Many funding accounts per trading key.** The mapping is 1:1 by design (see Design
  Decisions). Principals are free — a service trading for several funders creates one key per
  funder.
- **Consent by the trading principal (two-step grant).** `add_trading_account` is unilateral,
  like Hyperliquid's `approveAgent`. Accepted residual: `F` can claim any *unregistered*
  principal as its trading key. The consequence falls on `F` (the claimed key gains trading
  power over `F`'s own funds); a principal claimed against its will simply cannot deposit while
  whitelisted and its owner would use a different principal. R7's registered-user check ensures
  no principal with existing funds or history can ever be claimed.
- **Attributing individual orders to the specific trading key** in events or order records
  (forensics after a key compromise). Possible additive follow-up; today the event carries the
  resolved owner only (R2).
- **Subaccounts.** They partition *funds* under one key; this feature separates *keys* over one
  fund. Orthogonal (the `UserRegistry` doc already anticipates subaccounts as a key-type
  change).

## Design Decisions

- **Implicit caller resolution via a 1:1 registry (Hyperliquid model), not an explicit
  `on_behalf_of` argument (dYdX model).** The canister keeps a `trading → funding` map; order
  and read endpoints resolve the effective account from the caller alone.
  *Pros*: zero API churn — every existing endpoint keeps its signature, and G20 integrates by
  simply swapping the key its bot signs with; no per-call disambiguation; the hot path costs one
  map lookup. *Cons*: one key cannot serve two funding accounts — mitigated by minting another
  principal, which is free. The explicit-argument alternative and its trade-offs are in
  [Discussed Alternatives](#discussed-alternatives).
- **Structural denial, not a permission mask.** There is no per-key "can withdraw" flag that
  happens to be off: deposit / withdraw *refuse* trading accounts at the admission layer, and
  grants *refuse* principals that already hold an account. No reachable configuration lets a
  trading key move funds — mirroring Hyperliquid, where withdrawals are user-signed actions an
  agent's signature can never satisfy. The CEX permission-mask alternative exists (Binance /
  Kraken / Coinbase) but every venue compensates for its misconfiguration risk with extra gates
  (IP allowlists, address allowlists, consensus approvals); with exactly one scope to express,
  the structural rule is smaller and safer.
- **A principal is either a funding account or a trading account, never both.** Funding accounts
  are exactly the registered users (they acquire a `UserId` on first deposit); trading accounts
  can never deposit (R3) and must be unregistered at grant time (R7), so they never acquire one.
  This exclusivity is what makes *implicit* resolution unambiguous — "whose balance does `T`'s
  order draw?" always has exactly one answer.
- **Resolution at the endpoint boundary; `owner = F` everywhere downstream.** The trading /
  cancel / read entry points resolve `caller → effective account` once, up front; order records,
  order events, settlement, and the trades feed are untouched and keep operating on the funding
  principal. Replay needs no delegation state for orders because events already carry the
  resolved owner (R2).
- **Reuse the `Permissions` permit layer for admission; keep the whitelist data beside
  `UserRegistry` in stable memory.** `permit_deposit` / `permit_withdraw` already thread the
  caller and today grant unconditionally — they become caller-aware and denying, so the existing
  capability-token discipline (a state change proves its admission check ran) extends to this
  feature at its natural seam. Grant / revoke follow the `SetHalt` event pattern (controller-side
  admin ops recorded as events). The whitelist itself does **not** live in the snapshot-persisted
  `Permissions` struct: that struct holds a handful of global flags, while the whitelist grows
  with the user count — it belongs in its own stable-memory region like every other per-user
  structure (see Discussed Alternatives).

## Cross-exchange comparison

How the proposal lines up with the funding/trading separation on the surveyed venues. The
takeaway: the chosen shape is Hyperliquid's (the model the requester referenced), with the same
structural guarantees; expiry is the one Hyperliquid feature deliberately deferred.

| Capability | Binance / Kraken / Coinbase | Hyperliquid | dYdX v4 | This spec |
|---|---|---|---|---|
| Mechanism | permission-scoped API keys | master approves agent keypairs | on-chain authenticators | funding account whitelists principals |
| Trading-only credential | key with trade scope only | agent (all it can do) | `MsgPlaceOrder` filter | trading account |
| Withdrawal by trading credential | possible if misconfigured | **structurally impossible** | excluded by default | **structurally impossible** |
| Credential holds funds | n/a (same account) | no | no | no (R3 + R7) |
| Acting identity → owner | key ↦ its account | agent registry, 1 master/agent | trader names owner per call | registry, 1 funding account/key (R7) |
| Grant / revoke | account UI | `approveAgent` (master-signed) | add/remove authenticator | `add`/`remove_trading_account` (F only) |
| Cap on credentials | ~30 keys | 1 + 3 named agents | unbounded | `MAX_TRADING_ACCOUNTS_PER_USER` |
| Expiry | 90-day auto-downgrade (Binance) | mandatory ≤ 180 d | none | none (non-goal) |
| Reads by trading credential | with read scope | no (query by master address) | n/a | yes, resolve to `F` (R5) |

## Implementation

### Constraints

- The canister is event-sourced: state changes are recorded as events and replayed at
  `post_upgrade`; stable-memory writes are gated by `StableMemoryOptions::Write` so replay does
  not double-apply. Grant / revoke must follow the same pattern (R10).
- Admission is proven by non-clonable permit tokens from `state/permissions`
  (`permit_deposit(_caller)` / `permit_withdraw(_caller)` currently ignore the caller and always
  grant — this feature makes them caller-aware).
- Per-user state is keyed by the compact `UserId` minted by `UserRegistry`
  (`canister/src/user`); registration (`get_or_register`) happens only on deposit, reads use the
  non-registering `lookup`. This gives the invariant R3/R7 rely on: *registered ⇔ can hold
  funds*.
- Update endpoints assert `Mode::RestrictedTo` on the raw caller (`assert_caller_is_allowed`);
  this check stays raw (R12).
- `MemoryId`s 0–8 are in use (`canister/src/storage`); the whitelist takes the next free ids.

### Candid API — `canister/oisy_trade.did`, `libs/types`

```candid
add_trading_account : (principal) -> (variant { Ok; Err : AddTradingAccountError });
remove_trading_account : (principal) -> (variant { Ok; Err : RemoveTradingAccountError });
get_my_trading_accounts : () -> (variant { Ok : vec principal; Err : GetMyTradingAccountsError }) query;
```

All three are DEFI-2801 error envelopes. `AddTradingAccountError` carries one variant per R7
precondition (self-grant, already a trading account, already a registered user, caller is a
trading account, too many trading accounts); `RemoveTradingAccountError` covers "not your
trading account". `DepositError` and `WithdrawError` gain a variant denying funding operations
to trading accounts (R3). Adding variants to returned error types is a Candid-breaking change —
acceptable pre-launch; the repo's candid backward-compat check gets its expected-diff updated in
the same PR.

`MAX_TRADING_ACCOUNTS_PER_USER = 4` (Hyperliquid grants 1 unnamed + 3 named agents; no known
integrator needs more — trivially raisable later).

### Whitelist registry — `canister/src/user`

A `TradingAccounts<M>` registry beside `UserRegistry`, in two new stable regions
(`MemoryId` 9 and 10):

- `by_trading: StableBTreeMap<PrincipalKey, Principal, M>` — trading principal → **funding
  principal**. The hot lookup: one `get` per order / read call. The value is the principal (not
  the `UserId`) because everything downstream — ownership checks, order records, events —
  compares principals; the funding principal's `UserId` is then resolved exactly as today.
- a per-funding index (funding `UserId`-prefixed key → trading principal) for `get_my_trading_accounts`
  and the R7 cap check, mirroring the `by_user` index pattern of `OrderHistory` /
  `TradeHistory`. The funding account is registered (`get_or_register`) at its first grant, so
  it always has a `UserId` to key by.

API: `grant(funding: Principal, funding_id: UserId, trading: Principal)`,
`revoke(funding_id: UserId, trading: Principal)`, `funding_of(trading: &Principal) ->
Option<Principal>`, `list(funding_id: UserId) -> Vec<Principal>`, `count(funding_id: UserId)`.

### State wiring — `canister/src/state`

- **Resolution.** `State::resolve_account(caller: Principal) -> Principal`:
  `trading_accounts.funding_of(&caller).unwrap_or(caller)`. Applied once, at the entry of:
  `validate_limit_order` / `record_limit_order` (order owner, balance reservation — R2),
  `validate_cancel_limit_order` (ownership check — R4), `get_balances`, `get_user_order(s)`,
  `get_user_trades` / `get_user_order_trades` (R5). Everything downstream is unchanged.
- **Admission.** `permit_deposit` / `permit_withdraw` take the caller's delegation status and
  return `Result<PreAsyncPermit, UnauthorizedError>` with a new `UnauthorizedError` variant;
  `deposit` / `withdraw` map it into the R3 error before any ledger call. `permit_trading` is
  unchanged (halt checks are caller-agnostic).
- **Grant / revoke.** New event types `AddTradingAccountEvent { funding, trading }` and
  `RemoveTradingAccountEvent { funding, trading }`, handled in `state/audit` like `SetHalt`:
  the endpoint validates the R7 preconditions synchronously, records the event, and the handler
  applies it to `TradingAccounts` under the `Write` gate (R10).

### Endpoints — `canister/src/lib.rs`, `canister/src/main.rs`

Thin `#[ic_cdk::update]` / `#[ic_cdk::query]` wrappers in `main.rs` over business functions in
`lib.rs`, as for every existing endpoint. The management endpoints act on the raw caller (R9)
and assert restricted mode like other updates (R12).

### Test plan

Unit (`*/tests.rs`, fixtures per repo convention):

- `user` (registry): grant / revoke round-trip; `funding_of` resolution; every R7 precondition
  rejected (self-grant, double-grant across funding accounts, registered user, granting by a
  trading account, cap); listing isolated between funding accounts (R9); registry survives
  reload of the stable structures (R10).
- `state`: order placed by `T` reserves `F`'s balance and records `owner = F`; the order event
  carries `F` (R2); cancel by `T` succeeds, by another funding account's trading key fails with
  `NotOrderOwner` (R4); reads resolve (R5); after revoke, `T`'s calls act as a stranger and
  `F`'s open orders stay open (R6); `permit_deposit` / `permit_withdraw` deny a trading account
  (R3); `F` itself still passes all admissions (R8); replay with stable writes skipped does not
  double-apply grants (R10); restricted mode checks the raw caller (R12).

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Lifecycle: `F` deposits and whitelists `T`; `T` places an order that draws `F`'s balance; a
  fill settles into `F`'s balances; `T` reads `F`'s balances / orders / trades; `T`'s `deposit`
  and `withdraw` are rejected with the dedicated error; `T` cancels an order of `F`; `F` revokes
  `T`; `T`'s next call is rejected as a stranger while `F`'s remaining open orders are intact
  (R1–R8).
- `get_my_trading_accounts` before / after grant and revoke (R9); each grant error surfaced
  through the envelope (R7, R11); whitelist and enforcement survive a canister upgrade (R10).

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
# + the repo's candid backward-compat check (see justfile / CI)
```

### Delivery / PR sequence

Three stacked PRs, each independently mergeable / compilable / testable.

1. **Whitelist registry + management endpoints.** `TradingAccounts` stable registry (two new
   memory regions), `add_trading_account` / `remove_trading_account` /
   `get_my_trading_accounts` with all grant preconditions, the two event types with replay
   handling, envelope errors. The whitelist is recorded but not yet enforced anywhere.
   **Acceptance: R1 (mechanics), R7, R9, R10, R11.**
2. **Funding-operation denial.** `permit_deposit` / `permit_withdraw` become caller-aware;
   `deposit` / `withdraw` by a trading account fail with the dedicated error. Lands *before*
   resolution so a whitelisted principal can never acquire a balance of its own that resolution
   would later strand. **Acceptance: R3.**
3. **Caller resolution on trading and read paths.** `add_limit_order` / `cancel_limit_order` /
   `get_balances` / `get_my_orders` / `get_my_trades` resolve the effective account; end-to-end
   integration lifecycle. **Acceptance: R2, R4, R5, R6, R8, R12.**

## Discussed Alternatives

- **Explicit `on_behalf_of` argument (dYdX-style).** Order and read calls name the funding
  account; the canister checks the caller against that account's whitelist. *Pros*: one trading
  key can serve several funding accounts; the trading principal could keep its own separate
  account; no claim-a-principal grief (acting for `F` requires `F`'s grant *and* naming `F`).
  *Cons*: every order and read endpoint changes signature (or grows an optional field), all
  clients must thread the argument, and the common case (one funder) pays per-call
  disambiguation for a flexibility nobody asked for — G20's ask is exactly the Hyperliquid
  shape, and extra keys are free. Rejected; the registry chosen here does not preclude adding an
  explicit-argument path later if a one-key-many-funders need materializes.
- **Alias in `UserRegistry` — register `T` under `F`'s `UserId`.** Seductively small: resolution
  would fall out of the existing `lookup`. Rejected: revocation requires *removing* a registry
  entry, and the registry's invariants forbid removal (dense `len()`-derived id assignment,
  "identities are never removed"); moreover ownership checks and order records compare
  *principals*, not `UserId`s, so cancel-by-`T` would still need its own resolution — the alias
  buys less than it appears to.
- **Store the whitelist inside the `Permissions` struct.** Keeps all authorization data in one
  place, but `Permissions` is heap state serialized into the snapshot and sized for a handful of
  global flags; a per-user whitelist grows with the user count and belongs in stable memory like
  balances and order history. The *admission decision* still flows through `Permissions`
  permits — only the data lives beside `UserRegistry`.
- **Per-key permission mask (CEX-style `can_withdraw` flag).** More general (arbitrary scope
  combinations later), but one misconfiguration away from fund loss — which is why every CEX
  layers compensating gates on top (IP allowlists, withdrawal-address allowlists, Coinbase
  Prime's consensus approvals). We need exactly one scope; the structural rule is strictly
  smaller and cannot be misconfigured.
- **ICRC-2-style allowance.** Allowances grant a spender the right to *move a bounded amount of
  funds* — the opposite of the requirement (standing order-placement authority with **no**
  fund-movement rights). Notional caps could later layer on top of the whitelist (non-goal).
- **Two-step grant (trading principal must accept).** Closes the "claim a fresh principal"
  grief but adds a pending-grant state machine (accept / decline / expire) for a residual that
  R7 already bounds to unregistered principals and whose cost falls on the granter.
  Hyperliquid's `approveAgent` is likewise unilateral. Rejected; documented as an accepted
  residual (see Non-goals).
- **Client-side key separation only (no canister change).** Impossible today: whatever key signs
  orders *is* the account holding the funds — there is no separation to achieve without the
  canister distinguishing the two roles.
