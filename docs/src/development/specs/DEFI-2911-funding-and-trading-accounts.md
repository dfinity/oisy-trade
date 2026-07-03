---
id: DEFI-2911
title: Separate funding and trading accounts
tags: [accounts, permissions, security, api]
---

# Separate funding and trading accounts

## Motivation

Programmatic traders (feedback from a market maker integrating with the DEX) must keep a
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
- **R2 — Orders act on the funding account.** An order placed by `T` is economically `F`'s
  order: validated against and reserved from `F`'s free balance, recorded with `owner = F`,
  visible in `F`'s `get_my_orders`, and its fills settle into `F`'s balances. For matching and
  settlement it behaves exactly as if `F` had placed it (order events carry `F` as the owner, so
  event-log replay is unaffected); the acting key stays visible for audit (R13).
- **R3 — Funding operations denied.** `deposit` and `withdraw` called by a currently whitelisted
  `T` fail synchronously with a dedicated error, before any ledger interaction. Combined with the
  grant preconditions (R7), a trading account can never hold DEX balances.
- **R4 — Cancel authority.** `F` and any of `F`'s trading accounts may cancel `F`'s open orders.
  Any other principal — including a trading account of a *different* funding account — gets
  `NotOrderOwner`, exactly as today. Cancel authority is deliberately shared across `F`'s keys
  regardless of which key placed the order — matching every surveyed venue (keys over one
  balance share order authority; see the comparison) and keeping key rotation safe: a revoked
  key's open orders stay cancellable by the remaining keys. Interference between sibling keys is
  visible through attribution (R13).
- **R5 — Reads resolve.** `get_balances`, `get_my_orders`, and `get_my_trades` called by `T`
  return `F`'s data, exactly as if `F` had called.
- **R6 — Revocation is immediate.** After `remove_trading_account(T)`, calls by `T` are treated
  as coming from an ordinary unknown principal. `F`'s open orders — including those `T` placed —
  are unaffected and stay open.
- **R7 — Grant preconditions.** `add_trading_account(T)` fails if any of:
  - `F` is not a registered user (has never deposited) — granting requires an existing,
    economically established account and never creates one;
  - `F`'s free balance is below the configured minimum in every token that has one (see
    [Design Decisions](#design-decisions) and the Candid section) — e.g. "at least 1 ICP free";
  - `T == F` (self-grant);
  - `T` is already a trading account, of `F` or of anyone else — a trading account maps to
    **exactly one** funding account;
  - `T` is already a registered user (has ever deposited) — a trading key must be a fresh
    principal;
  - `F` is itself a trading account (no delegation chains; subsumed by the registration
    requirement once R3 is enforced, but kept explicit because the whitelist PR ships before
    the funding-denial PR);
  - `F` already has `MAX_TRADING_ACCOUNTS_PER_USER` trading accounts.
  `remove_trading_account(T)` fails only if `T` is not currently `F`'s trading account.
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
- **R13 — Attribution.** Every caller-initiated order action records the acting key:
  `OrderRecord` and the order-placement event gain `placed_by` (optional principal; absent =
  placed by the funding account itself), and the cancel event records `canceled_by` likewise.
  `placed_by` is exposed through `get_my_orders`, so after a key compromise `F` can list — and
  cancel — exactly the orders that key placed; the events preserve the same trail durably.
  Attribution is forensic only: it grants no authority and does not restrict cancel (R4).
- **R14 — Grant rate bound.** Successful grants are rate-limited per funding account:
  `add_trading_account` fails with a retryable error (the envelope's `TemporaryError` class) if
  less than `TRADING_ACCOUNT_GRANT_COOLDOWN` has elapsed since `F`'s previous successful grant.
  `remove_trading_account` is **never** rate-limited — revocation is the emergency response to
  a compromised key — and is inherently bounded by prior grants. Failed validations (including
  cooldown rejections) record no event, so only rate-bounded successful mutations grow the
  event log.

## Non-goals

- **Expiry / TTL on grants.** Hyperliquid caps agent validity at 180 days; we ship
  revocation-only and note expiry as a follow-up if operational experience warrants it.
- **Scoped grants** — per-pair restrictions, notional caps (e.g. "T may trade only these pairs,
  at most X per 24 h"), expiry, read-only keys — the dYdX authenticator-filter and MetaMask
  delegation-caveat analogues, and the natural shape for an AI-managed (agentic) trading key.
  Out of scope now, but the design leaves a deliberate evolution path: the registry value grows
  from the funding principal into a grant/policy object (additive CBOR fields, decoding absent
  as "unrestricted"), `add_trading_account` gains an optional policy argument (additive Candid),
  and enforcement slots into the existing admission point (`permit_trading` /
  `validate_limit_order`). Volume caps additionally need per-key consumption accounting, which
  is why order attribution (R13) ships now: a rolling per-key window has `placed_by` to hang off
  when caps arrive, instead of a migration.
- **Many funding accounts per trading key.** The mapping is 1:1 by design (see Design
  Decisions). Principals are free — a service trading for several funders creates one key per
  funder.
- **Consent by the trading principal (two-step grant).** `add_trading_account` is unilateral,
  like Hyperliquid's `approveAgent`. Accepted residual: `F` can claim any *unregistered*
  principal as its trading key. The consequence falls on `F` (the claimed key gains trading
  power over `F`'s own funds); a principal claimed against its will simply cannot deposit while
  whitelisted and its owner would use a different principal. R7's registered-user check ensures
  no principal with existing funds or history can ever be claimed, and claiming is not free —
  the claimer must itself be a deposited funding account meeting the minimum-balance bar, one
  grant per cooldown (R7, R14).
- **Subaccounts.** They partition *funds* under one key; this feature separates *keys* over one
  fund. Orthogonal (the `UserRegistry` doc already anticipates subaccounts as a key-type
  change).

## Design Decisions

- **Implicit caller resolution via a 1:1 registry (Hyperliquid model), not an explicit
  `on_behalf_of` argument (dYdX model).** The canister keeps a `trading → funding` map; order
  and read endpoints resolve the effective account from the caller alone.
  *Pros*: zero API churn — every existing endpoint keeps its signature, and an integrator
  switches over by simply swapping the key its bot signs with; no per-call disambiguation; the
  hot path costs one map lookup. *Cons*: one key cannot serve two funding accounts — mitigated by minting another
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
  are exactly the registered users (a `UserId` is acquired on first deposit, the only
  registering path); trading accounts can never deposit (R3), can never grant (R7), and must be
  unregistered at grant time (R7), so they never acquire one.
  This exclusivity is what makes *implicit* resolution unambiguous — "whose balance does `T`'s
  order draw?" always has exactly one answer.
- **Resolution at the endpoint boundary; `owner = F` everywhere downstream.** The trading /
  cancel / read entry points resolve `caller → effective account` once, up front; order records,
  order events, settlement, and the trades feed are untouched and keep operating on the funding
  principal. Replay needs no delegation state for orders because events already carry the
  resolved owner (R2).
- **Grants are economically gated and rate-bounded; revocation never is.** Every surveyed venue
  prices whitelist mutations (dYdX authenticator ops cost gas, Hyperliquid's `approveAgent` is
  an on-chain action, CEX keys sit behind authenticated, rate-limited UIs) — while an IC update
  call costs its caller nothing, so an ungated grant would be the canister's cheapest
  write-amplification surface (each successful mutation appends an event and writes the
  whitelist maps). Three layers close it: grant requires a **registered** funding account (the
  only registering path, deposit, has a real-token cost — R7), requires a controller-configured
  **minimum free balance** in some token (skin in the game, checked at grant time; deliberately
  a threshold, not a fee, and not locked afterwards — R7), and successful grants are separated
  by a **cooldown** (R14). `remove_trading_account` is exempt from all three: it must stay
  instantly available as the compromise response, and it is inherently bounded by prior grants.
- **Attribute, don't restrict.** Order authority is account-scoped (any of `F`'s keys can place
  and cancel `F`'s orders, R4) exactly as on every surveyed venue, but — going one step beyond
  the venues, none of which expose which API key placed an order — each action records the
  acting key (R13). This serves the compromise-forensics story that motivates the ticket (list
  and cancel precisely the rogue key's orders) and is the prerequisite for future per-key
  volume caps (see the scoped-grants non-goal), while keeping rotation simple: authority never
  fragments per key, so revoking a key strands nothing.
- **Reuse the `Permissions` permit layer for admission; keep the whitelist data inside
  `UserRegistry`.** `permit_deposit` / `permit_withdraw` already thread the
  caller and today grant unconditionally — they become caller-aware and denying, so the existing
  capability-token discipline (a state change proves its admission check ran) extends to this
  feature at its natural seam. Grant / revoke follow the `SetHalt` event pattern (controller-side
  admin ops recorded as events). The whitelist maps live as new fields **on `UserRegistry`**
  (each in its own stable-memory region), not as a separate struct: the grant invariant spans
  both the `users` map ("`F` is registered", "`T` is unregistered") and the whitelist ("`T` is
  not already a delegate"), so one type enforces *registered ⇔ funding account* where the data
  lives — and a multi-map domain struct is the repo idiom (`OrderHistory`, `TokenBalance`). The
  whitelist does **not** live in the snapshot-persisted `Permissions` struct: that struct holds
  a handful of global flags, while the whitelist grows with the user count (see Discussed
  Alternatives).

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
| Cross-key cancel on one balance | yes (any trade-scope key) | yes (any agent) | yes (same subaccount) | yes (R4) |
| Orders attributed to the acting credential | no | no | not exposed | yes, `placed_by` (R13) |
| Cost to mutate the whitelist | authenticated UI + rate limits | on-chain action | gas per op | registered + min free balance + cooldown (R7, R14) |

Sources:

- Hyperliquid:
  [Nonces and API wallets](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets)
  (agents hold no funds, master-signed `approveAgent`, 1 + 3 cap, ≤ 180-day validity,
  deregistration) ·
  [Exchange endpoint](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint)
  (the agent-signable vs user-signed action split that makes withdrawals structurally
  unsignable) ·
  [Privy recipe](https://docs.privy.io/recipes/hyperliquid/agents-and-subaccounts)
- dYdX v4:
  [Permissioned keys](https://docs.dydx.xyz/interaction/permissioned-keys)
  (authenticators: signature check composed with message / subaccount / market filters) ·
  [How-to guide](https://docs.dydx.exchange/api_integration-guides/how_to_permissioned_keys) ·
  [Foundation blog](https://www.dydx.foundation/blog/permissioned-keys)
- Binance:
  [API key permissions](https://developers.binance.com/docs/wallet/account/api-key-permission)
  (trade / withdraw / internal-transfer as separate flags) ·
  [2021 permission rule update](https://www.binance.com/en/support/announcement/updates-to-api-key-permission-rules-2021-07-26-11e4c2f44e7a47b9b5fc0e479c0b256f)
  (withdrawals require an IP allowlist; 90-day auto-downgrade) ·
  [Sub-account FAQ](https://www.binance.com/en/support/faq/binance-sub-account-functions-and-frequently-asked-questions-360020632811) ·
  [Universal Transfer](https://developers.binance.com/docs/sub_account/asset-management/Universal-Transfer)
- Kraken:
  [API key permissions](https://docs.kraken.com/exchange/guides/rest/api-keys)
  (deposit / withdraw / create / cancel as independent grants) ·
  [Creating an API key](https://support.kraken.com/articles/360000919966-how-to-create-an-api-key)
  (optional expiry, IP allowlist, withdrawals only to pre-approved addresses)
- Coinbase:
  [Advanced Trade key permissions](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/data-api/get-api-key-permissions)
  (`can_view` / `can_trade` / `can_transfer`) ·
  [Portfolios](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/portfolios)
  (keys bound to one portfolio) ·
  [Prime API overview](https://help.coinbase.com/en/prime/coinbase-prime-api/coinbase-prime-api) ·
  [Prime transfers](https://docs.cdp.coinbase.com/prime/concepts/transactions/transfers)
  (consensus approval on transfers)

## Implementation

### Constraints

- The canister is event-sourced: state changes are recorded as events and replayed at
  `post_upgrade`; stable-memory writes are gated by `StableMemoryOptions::Write` so replay does
  not double-apply. Grant / revoke must follow the same pattern (R10).
- Admission is proven by non-clonable permit tokens from `state/permissions`
  (`permit_deposit(_caller)` / `permit_withdraw(_caller)` currently ignore the caller and always
  grant — this feature makes them caller-aware).
- Per-user state is keyed by the compact `UserId` minted by `UserRegistry`
  (`canister/src/user`); registration (`get_or_register`) happens **only on deposit** — the one
  economically gated entry point (a real ICRC-2 transfer with a ledger fee) — and reads use the
  non-registering `lookup`. Granting requires prior registration (R7) and never registers
  anyone, so the invariant R3/R7 rely on is *registered ⇔ has deposited ⇔ funding account*, and
  the whitelist cannot be used to grow `UserRegistry` (whose entries are never removed) for
  free.
- Update endpoints assert `Mode::RestrictedTo` on the raw caller (`assert_caller_is_allowed`);
  this check stays raw (R12).
- `MemoryId`s 0–8 are in use (`canister/src/storage`); the whitelist takes the next free ids.

### Candid API — `canister/oisy_trade.did`, `libs/types`

```candid
add_trading_account : (principal) -> (variant { Ok; Err : AddTradingAccountError });
remove_trading_account : (principal) -> (variant { Ok; Err : RemoveTradingAccountError });
get_my_trading_accounts : () -> (variant { Ok : vec principal; Err : GetMyTradingAccountsError }) query;
```

All three are DEFI-2801 error envelopes. `AddTradingAccountError` carries one request-error
variant per R7 precondition (granter not registered, free balance below the minimum,
self-grant, already a trading account, already a registered user, caller is a trading account,
too many trading accounts) plus the R14 cooldown in its `TemporaryError` class;
`RemoveTradingAccountError` covers "not your trading account". `DepositError` and `WithdrawError` gain a variant denying funding operations
to trading accounts (R3), added *inside* the envelope's `opt variant` request-error class —
the extension point DEFI-2801 built in exactly for this: a client compiled against the old
interface decodes the unknown variant as `null` and falls back to the envelope's `message`,
so the addition is backward compatible (the system is launched; no Candid-breaking change is
acceptable). The new endpoints are purely additive. The implementation PR updates the repo's
candid backward-compat expected diff accordingly.

`MAX_TRADING_ACCOUNTS_PER_USER = 4` (Hyperliquid grants 1 unnamed + 3 named agents; no known
integrator needs more — trivially raisable later). `TRADING_ACCOUNT_GRANT_COOLDOWN` is a code
constant (proposed: 10 minutes — key rotation is a per-weeks operation).

The per-token grant minimums (R7) are controller-configured: `InitArg` / `UpgradeArg` gain an
optional `min_grant_balance : vec record { token_id : TokenId; amount : nat }` (an additive
`opt` field, like `mode`). A token **without** a configured minimum never qualifies a granter,
so depositing a worthless token cannot satisfy the check; an empty configuration disables
granting entirely until set. Example configuration: 1 ICP or 10 ckUSDT.

For attribution (R13), `OrderRecord` gains `placed_by : opt principal` (absent = placed by the
owner itself), surfaced by `get_my_orders`. Adding an `opt` field to a returned record is a
backward-compatible Candid evolution; on the stable-memory side it is a new trailing minicbor
field decoding absent as `None` (an `Option` field — minicbor's absent-field behavior, as
`last_updated_at` already relies on; codec `icrc_cbor::principal::option`), so records written before this
feature still decode — the post-launch requirement.

### Whitelist registry — `canister/src/user`

`UserRegistry<M>` gains two fields, each in its own new stable region (`MemoryId` 9 and 10),
following the repo's multi-map domain-struct idiom (`OrderHistory`, `TokenBalance`):

- `trading_accounts: StableBTreeMap<PrincipalKey, TradingGrant, M>` — trading principal →
  grant. The hot lookup: one `get` per order / read call. `TradingGrant` is a one-field minicbor
  struct holding the **funding principal** (a struct, not a bare principal, so the scoped-grants
  evolution adds `#[cbor(default)]` fields instead of migrating the value type; a principal
  rather than a `UserId` because everything downstream — ownership checks, order records,
  events — compares principals, and the funding `UserId` is then resolved exactly as today).
- `trading_accounts_by_funding: StableBTreeMap<UserId, TradingAccountList, M>` — funding
  `UserId` → the bounded list (≤ `MAX_TRADING_ACCOUNTS_PER_USER`) of its trading principals
  plus `last_granted_at` (the R14 cooldown anchor), serving `get_my_trading_accounts`, the R7
  cap check, and the cooldown check. A bounded inline list, not a `(UserId, principal)`
  range-scan index: the `CompositeId` machinery assumes fixed-width components (a `Principal`
  is variable-length), and a cardinality of at most 4 does not earn a scan index. Grant
  requires `F` to be registered already (R7), so it always has a `UserId` to key by.

The whitelist lives on `UserRegistry` (not in a separate struct) because the grant invariant
spans both maps and `users`: `grant` checks "`F` is registered", "`T` is unregistered", and
"`T` / `F` are not delegates" in one place, keeping *registered ⇔ funding account* enforced by
the type that owns the data. Registration itself stays deposit-only — grant reads `users` but
never writes it.

API on `UserRegistry`: `grant(funding: Principal, trading: Principal, now: Timestamp) ->
Result<(), GrantError>` (the identity, cap, and cooldown checks — R7 and R14; the
minimum-free-balance check layers in `State`, which owns `TokenBalance`),
`revoke(funding: Principal, trading: Principal) -> Result<(), RevokeError>`,
`resolve_account(caller: Principal) -> Principal` (delegate → funding principal, else the caller),
`is_trading_account(&Principal) -> bool` (the R3 deny check),
`trading_accounts_of(funding: Principal) -> Vec<Principal>` (R9).

### State wiring — `canister/src/state`

- **Resolution.** `State` delegates to `user_registry.resolve_account(caller)`. Applied once,
  at the entry of:
  `validate_limit_order` / `record_limit_order` (order owner, balance reservation — R2),
  `validate_cancel_limit_order` (ownership check — R4), `get_balances`, `get_user_order(s)`,
  `get_user_trades` / `get_user_order_trades` (R5). Everything downstream is unchanged.
- **Admission.** `permit_deposit` / `permit_withdraw` take the caller's delegation status and
  return `Result<PreAsyncPermit, UnauthorizedError>` with a new `UnauthorizedError` variant;
  `deposit` / `withdraw` map it into the R3 error before any ledger call. `permit_trading` is
  unchanged (halt checks are caller-agnostic).
- **Grant / revoke.** New event types `AddTradingAccountEvent { funding, trading }` and
  `RemoveTradingAccountEvent { funding, trading }`, handled in `state/audit` like `SetHalt`:
  the endpoint validates the preconditions synchronously — the identity, cap, and cooldown
  checks in `UserRegistry`, the minimum free balance against `TokenBalance` (R7, R14) — records
  the event, and the handler applies it to the `UserRegistry` whitelist maps under the `Write`
  gate (R10). Rejected calls record nothing.
- **Attribution (R13).** The order-placement path threads the raw caller alongside the resolved
  owner: `AddLimitOrderEvent` gains `placed_by: Option<Principal>` (`None` when the caller *is*
  the owner) and `record_limit_order` stores it on the `OrderRecord`; the cancel event gains
  `canceled_by: Option<Principal>` likewise. Optional trailing fields (absent decodes as `None`)
  keep the event log and order history decoding pre-existing entries (post-launch
  compatibility); replay is byte-faithful since the events carry the attribution themselves.

### Endpoints — `canister/src/lib.rs`, `canister/src/main.rs`

Thin `#[ic_cdk::update]` / `#[ic_cdk::query]` wrappers in `main.rs` over business functions in
`lib.rs`, as for every existing endpoint. The management endpoints act on the raw caller (R9)
and assert restricted mode like other updates (R12).

### Test plan

Unit (`*/tests.rs`, fixtures per repo convention):

- `user` (registry): grant / revoke round-trip; `resolve_account` resolution; every R7
  precondition rejected (unregistered granter, self-grant, double-grant across funding
  accounts, registered user, granting by a trading account, cap); a second grant inside
  `TRADING_ACCOUNT_GRANT_COOLDOWN` rejected, allowed once elapsed, and revoke unaffected by the
  cooldown (R14); listing isolated between funding accounts (R9); registry survives reload of
  the stable structures (R10).
- `state`: order placed by `T` reserves `F`'s balance and records `owner = F`; the order event
  carries `F` (R2); cancel by `T` succeeds — including of an order placed by a *sibling* trading
  key — while another funding account's trading key fails with `NotOrderOwner` (R4); reads
  resolve (R5); after revoke, `T`'s calls act as a stranger and `F`'s open orders stay open
  (R6); `permit_deposit` / `permit_withdraw` deny a trading account (R3); `F` itself still
  passes all admissions (R8); replay with stable writes skipped does not double-apply grants
  (R10); restricted mode checks the raw caller (R12); a grant below the configured minimum free
  balance is rejected, one meeting it in a single configured token passes, and an empty
  configuration means no grants (R7); a rejected grant appends no event (R14); an order placed
  by `T` records and
  exposes `placed_by = T`, one placed by `F` records none, a cancel by `T` records
  `canceled_by = T`, and an `OrderRecord` persisted without the attribution field still decodes
  (R13).

Integration (`integration_tests/tests/tests.rs`, PocketIC):

- Lifecycle: `F` deposits and whitelists `T`; `T` places an order that draws `F`'s balance; a
  fill settles into `F`'s balances; `T` reads `F`'s balances / orders / trades and
  `get_my_orders` shows `placed_by = T` on `T`'s order (R13); `T`'s `deposit`
  and `withdraw` are rejected with the dedicated error; `T` cancels an order of `F`; `F` revokes
  `T`; `T`'s next call is rejected as a stranger while `F`'s remaining open orders are intact
  (R1–R8).
- `get_my_trading_accounts` before / after grant and revoke (R9); each grant error surfaced
  through the envelope, the cooldown as a `TemporaryError` (R7, R11, R14); `min_grant_balance`
  set at init and changed via upgrade takes effect (R7); whitelist and enforcement survive a
  canister upgrade (R10).

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

1. **Whitelist registry + management endpoints.** The `UserRegistry` whitelist extension (two
   new memory regions), `add_trading_account` / `remove_trading_account` /
   `get_my_trading_accounts` with all grant preconditions (including the `min_grant_balance`
   configuration and the grant cooldown), the two event types with replay handling, envelope
   errors. The whitelist is recorded but not yet enforced anywhere.
   **Acceptance: R1 (mechanics), R7, R9, R10, R11, R14.**
2. **Funding-operation denial.** `permit_deposit` / `permit_withdraw` become caller-aware;
   `deposit` / `withdraw` by a trading account fail with the dedicated error. Lands *before*
   resolution so a whitelisted principal can never acquire a balance of its own that resolution
   would later strand. **Acceptance: R3.**
3. **Caller resolution on trading and read paths.** `add_limit_order` / `cancel_limit_order` /
   `get_balances` / `get_my_orders` / `get_my_trades` resolve the effective account; attribution
   (`placed_by` on the record, event and `get_my_orders`; `canceled_by` on the cancel event);
   end-to-end integration lifecycle. **Acceptance: R2, R4, R5, R6, R8, R12, R13.**

## Discussed Alternatives

- **Explicit `on_behalf_of` argument (dYdX-style).** Order and read calls name the funding
  account; the canister checks the caller against that account's whitelist. *Pros*: one trading
  key can serve several funding accounts; the trading principal could keep its own separate
  account; no claim-a-principal grief (acting for `F` requires `F`'s grant *and* naming `F`).
  *Cons*: every order and read endpoint changes signature (or grows an optional field), all
  clients must thread the argument, and the common case (one funder) pays per-call
  disambiguation for a flexibility nobody asked for — the requester's ask is exactly the
  Hyperliquid shape, and extra keys are free. Rejected; the registry chosen here does not preclude adding an
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
  permits — only the data lives on `UserRegistry`.
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
