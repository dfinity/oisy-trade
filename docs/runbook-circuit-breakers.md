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
