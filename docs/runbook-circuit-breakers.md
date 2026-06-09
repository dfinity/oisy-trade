# Circuit-breaker runbook

The DEX canister exposes controller-gated **soft halts** that pause activity
while always preserving state and letting users exit. This runbook documents
each mechanism: what it does, who may invoke it, and when to use it.

## Global trading halt

### Mechanism

A global trading halt stops all new orders and the matching engine across every
trading pair:

- `add_limit_order` is rejected with `TradingHalted`.
- The matching engine makes no progress: resting orders are left untouched and
  no crossing fills occur while the halt is in effect.

What stays open while halted:

- `cancel_limit_order` — users can always cancel resting orders.
- `withdraw` and `deposit` — users can always move their available balance.

The halt is a single persisted flag. It is recorded as a `SetGlobalHalt` event
in the audit log, so it is reproduced exactly on replay, and it is included in
the upgrade snapshot, so it survives canister upgrades.

### Who may invoke it

Only a **canister controller**. Non-controller callers are rejected with
`NotController`.

- `halt_trading` — turn the halt on.
- `resume_trading` — turn the halt off.

Both endpoints are idempotent: halting an already-halted DEX (or resuming an
already-active one) is a no-op success that still emits an event for the audit
trail.

### When to use it

Use a global halt when a problem affects the whole exchange and trading must
stop everywhere until it is resolved — for example, a **suspected
matching-engine bug**. Halt trading, investigate and fix, then `resume_trading`.
Because cancels and withdrawals stay open, users can exit their positions while
the halt is in effect.

## Per-pair halt

### Mechanism

A per-pair halt stops new orders and matching on a single trading pair while
every other pair keeps trading normally:

- `add_limit_order` on the halted pair is rejected with `PairHalted`.
- The matching engine skips the halted pair: its resting orders are left
  untouched and no crossing fills occur on it, while other pairs continue to
  match.

What stays open for the halted pair:

- `cancel_limit_order` — users can always cancel resting orders on the pair.
- `withdraw` and `deposit` — balances are never tied to a single pair and stay
  movable.

Halted pairs are tracked as a set of order-book identifiers. Each status change
is recorded as a `SetPairStatus` event in the audit log, so it is reproduced
exactly on replay, and the set is included in the upgrade snapshot, so it
survives canister upgrades.

### Who may invoke it

Only a **canister controller**. Non-controller callers are rejected with
`NotController`.

- `set_pair_status(pair, Halted)` — halt one pair.
- `set_pair_status(pair, Active)` — resume one pair.

The endpoint returns `UnknownTradingPair` for a pair that is not registered. It
is idempotent: setting a pair to its current status is a no-op success that
still emits an event for the audit trail.

### When to use it

Use a per-pair halt when a problem is confined to one market rather than the
whole exchange — for example, a **compromised or suspect ledger** backing one
pair's token. Halt just that pair so trading on every other pair continues
uninterrupted, investigate, and resume the pair once it is safe. Because
cancels and withdrawals stay open, users holding orders on the halted pair can
still exit.

## Per-account freeze

### Mechanism

A per-account freeze blocks a single principal's caller-facing actions while
leaving every other account untouched:

- `add_limit_order` is rejected with `AccountFrozen`.
- `deposit` and `withdraw` are rejected with `AccountFrozen`.

What stays open for the frozen account:

- `cancel_limit_order` — the account can always cancel its own resting orders.
- All read endpoints (e.g. `get_balances`, `get_order_status`).

The freeze deliberately does **not** touch matching: a frozen account's
existing resting orders keep filling for counterparties. Proceeds from those
fills land in the frozen account's balance and are visible via `get_balances`,
but stay non-withdrawable until the account is unfrozen. This keeps the order
book honest — a counterparty's incoming order never silently fails to match
against resting liquidity just because the resting account was frozen.

Because `deposit` and `withdraw` cross an asynchronous ledger call, the freeze
is checked twice: once before the ledger call (which rejects the operation
outright) and once after it returns. If a freeze lands while a deposit or
withdrawal is mid-flight, the post-call re-check cannot undo the committed
ledger transfer — the operation completes, but the race is recorded in the
canister logs for visibility.

Frozen accounts are tracked as a set of principals. Each change is recorded as
a `SetAccountFrozen` event in the audit log, so it is reproduced exactly on
replay, and the set is included in the upgrade snapshot, so it survives
canister upgrades.

### Who may invoke it

Only a **canister controller**. Non-controller callers are rejected with
`NotController`.

- `freeze_account(principal)` — freeze one account.
- `unfreeze_account(principal)` — restore one account's full access.

Both endpoints are idempotent: freezing an already-frozen account (or
unfreezing one that is not frozen) is a no-op success that still emits an event
for the audit trail.

### When to use it

Use a per-account freeze to contain a single principal rather than a market or
the whole exchange — for example, an account flagged by **compliance** or one
identified as an **attacker**. Freeze the account to stop it opening new orders
and moving funds in or out, while its resting liquidity keeps honouring
counterparties. Investigate, then `unfreeze_account` once it is cleared.
