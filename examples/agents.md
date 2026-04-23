# For AI agents

If you're an AI agent translating a user's intent ("sell 0.01 SOL at X", "what pairs are listed?") into DEX calls, the walkthrough above is your command template. A few things the walkthrough doesn't spell out but you must respect:

## Discover the live interface via `canhelp`

Don't rely on memory or snapshots. Before planning a call sequence, run:

```
/canhelp dex
```

or by canister ID:

```
/canhelp proc5-daaaa-aaaar-qb5va-cai
```

The Candid `//` doc comments are **binding** — they encode the rules types alone don't: ICRC-2 allowance math on `deposit`, tick/lot constraints, what each `Side` reserves, async lifecycle of `add_limit_order`, the `AmountTooSmall` floor on `withdraw`, and when each error variant is triggered. If `canhelp` isn't available, fall back to `icp canister metadata proc5-daaaa-aaaar-qb5va-cai candid:service --network ic`.

## Amount discipline

Users speak human ("0.01 SOL"); the DEX speaks base units (`10^decimals`). Always:

- Convert with integer math — **never floats**.
- Query `icrc1_fee` before quoting any amount — fees vary wildly between ledgers (e.g. ckDevnetSOL `50`, ckSepoliaETH `10_000_000_000`).
- Confirm both views with the user: "1_000_000 ckSOL base units = 0.001 SOL".

## Example dialogues

**"What pairs can I trade?"** → `get_trading_pairs`; summarize in human terms ("ckSOL vs ckSepoliaETH, min order 0.001 SOL, price tick 0.00001 ETH/SOL").

**"Sell 0.01 SOL for ckSepoliaETH at market."** → (1) confirm the pair is listed; (2) convert 0.01 SOL to `10_000_000` base units; (3) verify `quantity` is a multiple of `lot_size`; (4) pick a price (ask if unclear — there's no `get_orderbook`); (5) check the seller's on-DEX free base ≥ `quantity`; (6) place the order as the seller identity; (7) poll `get_order_status` for `Filled`.

**On any error** → translate the variant name into plain language plus a concrete next step (e.g. `InsufficientAllowance { allowance }` → "your allowance is X but you need X + fee — let me re-approve").

## Absolute don'ts

- **Don't fabricate** canister IDs, method names, field names, or error variants. Run `/canhelp dex`.
- **Don't skip `icrc1_fee`** before quoting an approve / deposit / withdraw amount.
- **Don't use floating point** for token-amount math.
- **Don't invoke a signing call** with an identity the user hasn't authorized for this conversation.
- **Don't claim an order filled** because `add_limit_order` returned `Ok` — that's acceptance, not execution. Confirm via `get_order_status`.
- **Don't over-deposit "for safety"** — on ledgers with high fees (ckSepoliaETH), it's expensive and usually not what the user wants.

## What's next

- Inspect the append-only event log via `get_events` — every state change (listings, deposits, orders) is recorded. See `canister/dex.did` for the full schema.
- See `integration_tests/` for end-to-end scenarios that exercise every endpoint programmatically.
