# OISY TRADE admin

Administrative operations that require the caller to be a **controller** of the OISY TRADE canister:

1. Upgrade the canister to a new WASM
2. Add a new trading pair
3. Halt and resume trading
4. Halt and resume a single trading pair

Calls will fail when run from a non-controller identity.

Run every command below from the same shell — steps share `export`ed variables (`IDENTITY`, `PIN_FILE`, `BASE_LEDGER`, ...).

## Prerequisites

- [`icp` CLI](https://cli.internetcomputer.org/) installed and on `PATH`
- Run these commands from the **project root** so `--environment staging` resolves the `oisy_trade` canister name (defined in `icp.yaml`)
- An identity that is a controller of the OISY TRADE canister. The repo convention is the `hsm` identity — adjust `IDENTITY` below if you use a different one.
- Optional: `curl` and `jq` — only needed for the Binance sizing sanity check in §2.

## Setup

### Identity

```bash
export IDENTITY=hsm   # controller identity; matches the default in the justfile
```

### Unlock the identity (HSM PIN / PEM password)

> If your identity is an unencrypted PEM (common in dev), skip this whole subsection and drop `--identity-password-file "$PIN_FILE"` from every command below.

Every signing call below passes `--identity "$IDENTITY"`. If the identity is HSM-linked or password-encrypted, `icp` would prompt on a TTY for its unlock secret. `--identity-password-file <FILE>` reads the secret from a file instead, which is what we use here. Two ways to populate the file:

**A. You already have the unlock secret in a local file** — just point at it:

```bash
export PIN_FILE=~/.config/icp/hsm.pin   # adjust to your file
```

**B. Prompt once and write a chmod-600 temp file** (removed in the `Clean up` step at the end):

```bash
export PIN_FILE=$(mktemp -t icp-identity-XXXXXX)
chmod 600 "$PIN_FILE"
read -rs -p "Unlock $IDENTITY (HSM PIN or PEM password): " pin && echo
printf '%s' "$pin" > "$PIN_FILE"
unset pin
```

### Check you're a controller

`canister status` lists the controllers. Your principal must appear there, otherwise every update call below is rejected.

```bash
icp identity principal --identity "$IDENTITY" --identity-password-file "$PIN_FILE"
icp canister status oisy_trade --environment staging --identity "$IDENTITY" --identity-password-file "$PIN_FILE"
```

## Upgrade the canister

Upgrades preserve stable memory: balances, open orders, listed pairs, and the event log all survive. The canister's `post_upgrade` takes an `opt OisyTradeArg`; passing `(null)` keeps the current configuration. To change the access mode (`GeneralAvailability` ↔ `RestrictedTo`), pass a structured `Upgrade` arg instead.

### Build the WASM

`just build` compiles `oisy_trade_canister` to `wasm32-unknown-unknown` in release mode and produces `wasms/oisy_trade_canister.wasm.gz`. The `icp` CLI picks up that artifact automatically via the recipe declared in `icp.yaml`.

```bash
just build
```

### Deploy

Two equivalent paths:

**A. `just deploy` recipe** — keeps config unchanged (`--args '(null)'`):

```bash
just deploy "$IDENTITY" "$PIN_FILE"
```

**B. Full command** — gives you control over the upgrade arg:

```bash
icp canister install oisy_trade --mode upgrade --args '(null)' \
    --identity "$IDENTITY" --identity-password-file "$PIN_FILE" \
    --environment staging -y
```

## Add a trading pair

`add_trading_pair` is an update call restricted to controllers. The request carries both ledger IDs plus the token metadata (`symbol`, `decimals`).

### Choose the ledgers

Base is the asset being bought/sold; quote is the asset prices are denominated in.

```bash
export BASE_LEDGER=la34w-haaaa-aaaar-qb5na-cai   # ckDevnetSOL
export QUOTE_LEDGER=apia6-jaaaa-aaaar-qabma-cai  # ckSepoliaETH
```

### Fetch ledger metadata

The `symbol` and `decimals` you submit **must** match what each ledger reports via `icrc1_symbol` / `icrc1_decimals` — otherwise OISY TRADE rejects the call (if the token is already registered under different metadata) or, more insidiously, registers the pair with metadata that misrepresents the asset.

```bash
icp canister call "$BASE_LEDGER" icrc1_symbol '()' --query --network ic --identity anonymous
icp canister call "$BASE_LEDGER" icrc1_decimals '()' --query --network ic --identity anonymous
icp canister call "$QUOTE_LEDGER" icrc1_symbol '()' --query --network ic --identity anonymous
icp canister call "$QUOTE_LEDGER" icrc1_decimals '()' --query --network ic --identity anonymous
```

Record what the ledgers reported:

```bash
export BASE_SYMBOL=ckDevnetSOL
export BASE_DECIMALS=9
export QUOTE_SYMBOL=ckSepoliaETH
export QUOTE_DECIMALS=18
```

### Choose tick size and lot size

- `tick_size` — the minimum price increment (in quote-token base units per base-token base unit). All order prices must be a positive multiple.
- `lot_size` — the minimum quantity (in base-token base units). All order quantities must be a positive multiple.

Both are `nat64`, both must be > 0, and both are **fixed for the lifetime of the pair**.

Centralized exchanges have already picked these parameters for most major pairs, balancing price precision against spam-order resistance. Binance's public REST endpoint is a convenient sanity check:

```bash
curl -sSf "https://api.binance.com/api/v3/exchangeInfo?symbol=SOLETH" \
  | jq '{tickSize: (.symbols[0].filters[] | select(.filterType=="PRICE_FILTER") | .tickSize), stepSize: (.symbols[0].filters[] | select(.filterType=="LOT_SIZE") | .stepSize)}'
```

The `filters` array contains a `PRICE_FILTER` (`tickSize`) and a `LOT_SIZE` (`stepSize`). Those values are human-readable decimal token counts — convert to OISY TRADE's integer base units using the ledger decimals you exported above:

- `tick_size = tickSize_binance × 10^(quote_decimals − base_decimals)`
- `lot_size  = stepSize_binance × 10^base_decimals`

For `SOLETH` at the time of writing: `tickSize = 0.00001 ETH/SOL`, `stepSize = 0.001 SOL`. Plugging into the formulas above (ckDevnetSOL `base_decimals = 9`, ckSepoliaETH `quote_decimals = 18`):

- `tick_size = 0.00001 × 10^(18 − 9) = 0.00001 × 10^9 = 10^4 = 10_000`
- `lot_size  = 0.001   × 10^9        = 10^6             = 1_000_000`

```bash
export TICK_SIZE=10_000
export LOT_SIZE=1_000_000
```

### Call `add_trading_pair`

```bash
icp canister call oisy_trade add_trading_pair --args-file /dev/stdin \
    --identity "$IDENTITY" --identity-password-file "$PIN_FILE" \
    --environment staging <<EOF
(
    record {
        base = record {
            id = record { ledger_id = principal "$BASE_LEDGER" };
            metadata = record { symbol = "$BASE_SYMBOL"; decimals = $BASE_DECIMALS : nat8 }
        };
        quote = record {
            id = record { ledger_id = principal "$QUOTE_LEDGER" };
            metadata = record { symbol = "$QUOTE_SYMBOL"; decimals = $QUOTE_DECIMALS : nat8 }
        };
        tick_size = $TICK_SIZE : nat64;
        lot_size  = $LOT_SIZE  : nat64
    }
)
EOF
```

### Verify the listing

```bash
icp canister call oisy_trade get_trading_pairs '()' --environment staging --query --identity anonymous
```

The new pair should appear in the output.

## Halt and resume trading

The canister exposes a controller-gated **global trading halt**: a soft circuit
breaker that pauses new orders and the matching engine across every pair while
always preserving state and letting users exit their positions.

### Mechanism

While the halt is in effect:

- `add_limit_order` is rejected with `TradingHalted` once the request passes
  pair resolution and validation — an unknown pair still returns
  `UnknownTradingPair`, and an order failing the tick/lot or notional checks
  still returns the corresponding validation error, since those run before the
  halt gate.
- The matching engine makes no progress: resting orders are left untouched and
  no crossing fills occur while the halt is in effect.

What the halt itself does not block:

- `cancel_limit_order` — canceling resting orders.
- `withdraw` and `deposit` — moving available balance.

These endpoints are not gated by the halt, so they are not rejected with
`TradingHalted`. Other canister-wide access controls (e.g. the `Mode`
restriction) still apply and can independently reject a caller.

The global halt is a single persisted flag. Each change is recorded as a
`SetHalt` event in the audit log (with no pair filter, i.e. `book_ids = null`),
so it is reproduced exactly on replay, and the flag is included in the upgrade
snapshot, so it survives canister upgrades.

Both endpoints take an optional pair filter (see section 4); passing `null`
targets the global halt. They are idempotent: halting an already-halted canister
(or resuming an already-active one) is a no-op success that still emits an event
for the audit trail. A global resume (`resume_trading(null)`) additionally clears
every per-pair halt. Non-controller callers are rejected with `NotController`.

### When to use it

Use a global halt when a problem affects the whole exchange and trading must
stop everywhere until it is resolved — for example, a **suspected
matching-engine bug**. Halt trading, investigate and fix, then resume. Because
cancels and withdrawals stay open, users can exit their positions while the
halt is in effect.

### Halt

```bash
icp canister call oisy_trade halt_trading '(null)' \
    --identity "$IDENTITY" --identity-password-file "$PIN_FILE" \
    --environment staging
```

### Resume

```bash
icp canister call oisy_trade resume_trading '(null)' \
    --identity "$IDENTITY" --identity-password-file "$PIN_FILE" \
    --environment staging
```

## 4. Halt and resume a single trading pair

A per-pair halt stops new orders and matching on a single trading pair while
every other pair keeps trading normally.

### Mechanism

A per-pair halt reuses the same `halt_trading` / `resume_trading` endpoints as
the global halt, but with a list of trading pairs instead of `null`.

While a pair is halted:

- `add_limit_order` on the halted pair is rejected with `TradingHalted` (the same
  error as a global halt).
- The matching engine skips the halted pair: its resting orders are left
  untouched and no crossing fills occur on it, while other pairs continue to
  match.

What stays open for the halted pair:

- `cancel_limit_order` — users can always cancel resting orders on the pair.
- `withdraw` and `deposit` — balances are never tied to a single pair and stay
  movable.

Halted pairs are tracked as a set of order-book identifiers. Each change is
recorded as a `SetHalt` event in the audit log (with the targeted pairs in
`book_ids`), so it is reproduced exactly on replay, and the set is included in
the upgrade snapshot, so it survives canister upgrades.

A pair is considered halted when the global flag is on **or** the pair is in the
set, and `get_trading_pairs` reports it as `Halted` in either case. Passing a
list of pairs to `halt_trading` adds them to the set; passing the same list to
`resume_trading` removes them; `resume_trading(null)` clears the entire set at
once (along with the global flag).

The endpoints are controller-gated; non-controller callers are rejected with
`NotController`. They **trap** if any listed pair is not a registered trading
pair (all pairs are validated up front, before anything is recorded), and are
idempotent: re-halting an already-halted pair is a no-op success that still emits
an event for the audit trail. A single call may list at most 100 pairs; passing
more **traps**.

### When to use it

Use a per-pair halt when a problem is confined to one market rather than the
whole exchange — for example, a **compromised or suspect ledger** backing one
pair's token. Halt just that pair so trading on every other pair continues
uninterrupted, investigate, and resume the pair once it is safe. Because cancels
and withdrawals stay open, users holding orders on the halted pair can still
exit.

### Halt a pair

```bash
icp canister call oisy_trade halt_trading \
    "(opt vec { record { base = principal \"$BASE_LEDGER\"; quote = principal \"$QUOTE_LEDGER\" } })" \
    --identity "$IDENTITY" --identity-password-file "$PIN_FILE" \
    --environment staging
```

### Resume a pair

```bash
icp canister call oisy_trade resume_trading \
    "(opt vec { record { base = principal \"$BASE_LEDGER\"; quote = principal \"$QUOTE_LEDGER\" } })" \
    --identity "$IDENTITY" --identity-password-file "$PIN_FILE" \
    --environment staging
```

## Clean up

If you used option **B** in Setup (prompt + temp file), remove the file:

```bash
rm -f "$PIN_FILE"
```

If you used option **A** (pointed at an existing file), leave it alone.