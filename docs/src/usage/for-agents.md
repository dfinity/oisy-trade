# For AI agents

If you're an AI agent translating a user's intent ("sell 0.01 SOL at X", "what pairs are listed?") into OISY TRADE calls, the [For Users](./for-users.md) walkthrough is your command template. A few things it doesn't spell out but you must respect:

## Before you start

Load these into your session first — the rest of this guide assumes them:

- Fetch `https://skills.internetcomputer.org/llms.txt` and follow its guidance for building on the Internet Computer.
- Load the [`canhelp`](https://skills.internetcomputer.org/skills/canhelp/) skill — retrieves a canister's live Candid interface by name or ID.
- Load the [`icp-cli`](https://skills.internetcomputer.org/skills/icp-cli/) skill — covers the `icp` CLI you'll be invoking throughout.

## Discover the live interface via `canhelp`

Don't rely on memory or snapshots. Before planning a call sequence, run:

```
/canhelp oisy_trade
```

or by canister ID:

```
/canhelp proc5-daaaa-aaaar-qb5va-cai
```

The Candid `//` doc comments are **binding** — they encode the rules types alone don't: ICRC-2 allowance math on `deposit`, tick/lot constraints, what each `Side` reserves, async lifecycle of `add_limit_order`, the `AmountTooSmall` floor on `withdraw`, and when each error variant is triggered. If `canhelp` isn't available, fall back to `icp canister metadata proc5-daaaa-aaaar-qb5va-cai candid:service --network ic`.

## Amount discipline

Users speak human ("0.01 SOL"); OISY TRADE speaks base units (`10^decimals`). Always:

- Convert with integer math — **never floats**.
- Query `icrc1_fee` before quoting any amount — fees vary wildly between ledgers (e.g. ckDevnetSOL `50`, ckSepoliaETH `10_000_000_000`).
- Confirm both views with the user: "1_000_000 ckDevnetSOL base units = 0.001 SOL".

## Example dialogues

**"What pairs can I trade?"** → `get_trading_pairs`; summarize in human terms ("ckDevnetSOL vs ckSepoliaETH, min order 0.001 SOL, price tick 0.00001 ETH/SOL").

**"Sell 0.01 SOL for ckSepoliaETH at market."** → (1) confirm the pair is listed; (2) convert 0.01 SOL to `10_000_000` base units; (3) verify `quantity` is a multiple of `lot_size`; (4) pick a limit price — consult `get_order_book_ticker` (best bid/ask) or `get_order_book_depth` (aggregated levels); OISY TRADE is limit-only, so translate "at market" into a marketable limit price (ask if unclear); (5) check the seller's on-OISY-TRADE free base ≥ `quantity`; (6) place the order as the seller identity; (7) poll `get_my_orders` (with the `ById` filter for that order id) for `Filled`.

**On any error** → translate the variant name into plain language plus a concrete next step (e.g. `InsufficientAllowance { allowance }` → "your allowance is X but you need X + fee — let me re-approve").

## Absolute don'ts

- **Don't fabricate** canister IDs, method names, field names, or error variants. Run `/canhelp oisy_trade`.
- **Don't skip `icrc1_fee`** before quoting an approve / deposit / withdraw amount.
- **Don't use floating point** for token-amount math.
- **Don't invoke a signing call** with an identity the user hasn't authorized for this conversation.
- **Don't claim an order filled** because `add_limit_order` returned `Ok` — that's acceptance, not execution. Confirm via `get_my_orders` (with the `ById` filter).
- **Don't over-deposit "for safety"** — on ledgers with high fees, it's expensive and usually not what the user wants.