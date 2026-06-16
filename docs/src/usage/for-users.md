# Interact with OISY TRADE

Walkthrough of the core OISY TRADE flows against the staging canister, using only the [`icp`](https://cli.internetcomputer.org/) CLI:

1. List trading pairs
2. Approve and deposit
3. Place a limit order
4. Check order status
5. Withdraw

The walkthrough uses two configurable identities — one for buy-side operations, one for sell-side — so the flow can exercise both halves of a matched trade. Run every command below from the same shell: steps share `export`ed variables (`BUYER_IDENTITY`, `SELLER_IDENTITY`, `BASE_LEDGER`, `OISY_TRADE`, ...).

## Prerequisites

- [`icp` CLI](https://cli.internetcomputer.org/) installed and on `PATH`
- Run these commands from the **project root** so `--environment staging` resolves the `oisy_trade` canister name (defined in `icp.yaml`)
- Two identities configured via `icp identity new` or `icp identity import` — one acts as the buyer, one as the seller. They may be the same identity if you just want to see the commands run end-to-end against yourself; using two distinct identities lets you demonstrate an actual matched trade.

## Identities

Export a buyer and a seller identity. OISY TRADE keys internal balances and order ownership off the caller principal, so every signing call targets one of these two.

```bash
export BUYER_IDENTITY=<buyer-name>
export SELLER_IDENTITY=<seller-name>

icp identity principal --identity "$BUYER_IDENTITY"
icp identity principal --identity "$SELLER_IDENTITY"
```

## Limit Orders 101

- **Trading Pair** — two token ledgers: the *base* (what you're buying or selling) and the *quote* (what prices are denominated in). `SOL / ETH` means trading SOL priced in ETH.
- **Quantity** — amount of the *base* token, in its base units (e.g. 1 SOL = 10<sup>9</sup> units at 9 decimals). Must be a positive multiple of `lot_size`.
- **Price** — *quote* base units per one *base* base unit (an integer ratio, no decimals). Must be a positive multiple of `tick_size`. A fill of `quantity` at `price` settles `price × quantity` quote-token base units.
- **Side** — `Buy` (bid) reserves `price × quantity` of the *quote* token from your free balance; `Sell` (ask) reserves `quantity` of the *base* token. The order fills when a crossing order arrives, otherwise it rests in the book until canceled.

## List trading pairs

`get_trading_pairs` returns every listed pair along with its `tick_size` (price must be a positive multiple) and `lot_size` (quantity must be a positive multiple, in base-token base units).

```bash
icp canister call oisy_trade get_trading_pairs '()' --environment staging --query --identity anonymous
```

## Pick a pair

Copy the base and quote ledger principals from the output above and export them. The values below are the ckDevnetSOL / ckSepoliaETH pair listed on staging — adjust if you picked a different pair.

```bash
export BASE_LEDGER=la34w-haaaa-aaaar-qb5na-cai   # ckDevnetSOL
export QUOTE_LEDGER=apia6-jaaaa-aaaar-qabma-cai  # ckSepoliaETH
export OISY_TRADE=proc5-daaaa-aaaar-qb5va-cai    # staging OISY TRADE canister

export TICK_SIZE=10_000
export LOT_SIZE=1_000_000
```

## Approve and deposit

Every limit order reserves one side of the pair before the matching engine can fill it:

- **Sell** orders reserve `quantity` *base* tokens → the seller needs base in their on-OISY-TRADE *free* balance
- **Buy** orders reserve `price × quantity` *quote* tokens → the buyer needs quote in their on-OISY-TRADE *free* balance

So which identity approves and deposits which token depends on the side it plans to trade.

`deposit` moves tokens by calling [`icrc2_transfer_from`](https://github.com/dfinity/ICRC-1/tree/main/standards/ICRC-2) on the token ledger, so OISY TRADE must first be approved to spend on the caller's behalf. Two fees are charged to the on-ledger balance:

1. `icrc2_approve` charges the ledger fee once
2. `icrc2_transfer_from` (triggered later by `deposit`) charges the ledger fee again, on top of the amount transferred

So to deposit `N`, approve **at least `N + ledger_fee`** and hold **at least `N + 2 × ledger_fee`** on the ledger. Fees vary widely per ledger — this walkthrough assumes **ckDevnetSOL = `50`** base units (≈ 5 × 10<sup>-8</sup> SOL) and **ckSepoliaETH = `10_000_000_000`** (= 10<sup>-8</sup> ETH). If the live `icrc1_fee` values below differ from these, adjust the literal `amount` fields in the approve / deposit calls accordingly before running them:

```bash
# Seller — base ledger (needs base to sell)
icp token "$BASE_LEDGER" balance --network ic --identity "$SELLER_IDENTITY"
icp canister call "$BASE_LEDGER" icrc1_fee '()' --query --identity anonymous --network ic

# Buyer — quote ledger (needs quote to buy)
icp token "$QUOTE_LEDGER" balance --network ic --identity "$BUYER_IDENTITY"
icp canister call "$QUOTE_LEDGER" icrc1_fee '()' --query --identity anonymous --network ic
```

Then approve and deposit — run each subsection as the appropriate identity. `icrc2_approve` calls the token ledger directly (`--network ic`); `deposit` targets OISY TRADE on staging (`--environment staging`).

### Seller — approve + deposit *base*

Deposits 10 lots (= `10 × LOT_SIZE` base-token base units — the token's smallest indivisible unit, not to be confused with the *base* token in the trading pair), enough for one sell at `quantity = 10_000_000`. Approval is `10 × LOT_SIZE + ckDevnetSOL_fee = 10_000_000 + 50`.

```bash
icp canister call "$BASE_LEDGER" icrc2_approve --args-file /dev/stdin --network ic --identity "$SELLER_IDENTITY" <<EOF
(
    record {
        spender = record { owner = principal "$OISY_TRADE"; subaccount = null };
        amount  = 10_000_050 : nat
    }
)
EOF

icp canister call oisy_trade deposit --args-file /dev/stdin --environment staging --identity "$SELLER_IDENTITY" <<EOF
(
    record {
        token_id = record { ledger_id = principal "$BASE_LEDGER" };
        amount   = 10_000_000 : nat
    }
)
EOF
```

### Buyer — approve + deposit *quote*

Deposits `price × quantity = TICK_SIZE × 10_000_000 = 10_000 × 10_000_000 = 100_000_000_000` quote-base-units, enough for one buy at `price = TICK_SIZE`, `quantity = 10_000_000`. Approval is `deposit + ckSepoliaETH_fee = 100_000_000_000 + 10_000_000_000`. The buyer's ckSepoliaETH on-ledger balance must be at least `deposit + 2 × fee = 120_000_000_000` (≈ 1.2 × 10<sup>-7</sup> ETH) to cover both the `icrc2_approve` fee and the `icrc2_transfer_from` fee on top of the deposit itself.

```bash
icp canister call "$QUOTE_LEDGER" icrc2_approve --args-file /dev/stdin --network ic --identity "$BUYER_IDENTITY" <<EOF
(
    record {
        spender = record { owner = principal "$OISY_TRADE"; subaccount = null };
        amount  = 110_000_000_000 : nat
    }
)
EOF

icp canister call oisy_trade deposit --args-file /dev/stdin --environment staging --identity "$BUYER_IDENTITY" <<EOF
(
    record {
        token_id = record { ledger_id = principal "$QUOTE_LEDGER" };
        amount   = 100_000_000_000 : nat
    }
)
EOF
```

> Candid's record literals use `{` / `}`, which collide with bash variable-expansion rules. Passing the candid via `--args-file /dev/stdin` and a heredoc sidesteps every shell-quoting pitfall — the unquoted `EOF` terminator still expands `$VAR` inside the body.

## Check on-OISY-TRADE balances

OISY TRADE tracks balance per `(caller, token)`:

- `free` — available for new orders or withdrawal
- `reserved` — locked by open orders

```bash
# Seller's base balance
icp canister call oisy_trade get_balances --args-file /dev/stdin --environment staging --query --identity "$SELLER_IDENTITY" <<EOF
(opt vec { variant { ById = record { ledger_id = principal "$BASE_LEDGER" } } })
EOF

# Buyer's quote balance
icp canister call oisy_trade get_balances --args-file /dev/stdin --environment staging --query --identity "$BUYER_IDENTITY" <<EOF
(opt vec { variant { ById = record { ledger_id = principal "$QUOTE_LEDGER" } } })
EOF
```

## Place a limit order

`price` must be a positive multiple of `TICK_SIZE`; `quantity` a positive multiple of `LOT_SIZE`.

Submission is asynchronous: the call returns an order ID immediately, the matching engine processes it on the next canister tick. Two resting orders with crossing prices fill as soon as they meet.

### Sell (as seller)

```bash
icp canister call oisy_trade add_limit_order --args-file /dev/stdin --environment staging --identity "$SELLER_IDENTITY" <<EOF
(
    record {
        pair     = record { base = principal "$BASE_LEDGER"; quote = principal "$QUOTE_LEDGER" };
        side     = variant { Sell };
        price    = $TICK_SIZE : nat64;
        quantity = 10_000_000  : nat
    }
)
EOF
```

Paste the returned 32-char hex order ID:

```bash
export SELL_ORDER_ID=<paste-the-order-id-here>
```

### Buy (as buyer)

Same `price`/`quantity`, so this order crosses the seller's resting ask and fills both.

```bash
icp canister call oisy_trade add_limit_order --args-file /dev/stdin --environment staging --identity "$BUYER_IDENTITY" <<EOF
(
    record {
        pair     = record { base = principal "$BASE_LEDGER"; quote = principal "$QUOTE_LEDGER" };
        side     = variant { Buy };
        price    = $TICK_SIZE : nat64;
        quantity = 10_000_000  : nat
    }
)
EOF
```

```bash
export BUY_ORDER_ID=<paste-the-order-id-here>
```

## Check order status

```bash
icp canister call oisy_trade get_my_orders --args-file /dev/stdin --environment staging --query --identity "$SELLER_IDENTITY" <<EOF
(opt record { filter = variant { ById = "$SELL_ORDER_ID" } })
EOF
icp canister call oisy_trade get_my_orders --args-file /dev/stdin --environment staging --query --identity "$BUYER_IDENTITY" <<EOF
(opt record { filter = variant { ById = "$BUY_ORDER_ID" } })
EOF
```

`get_my_orders` returns the caller's orders; the `ById` filter selects a single order by id and returns it (or an empty vector if the caller does not own it). Each returned order carries a `status` field:

| Status     | Meaning                                   |
|------------|-------------------------------------------|
| `Pending`  | Accepted, awaiting the matching engine    |
| `Open`     | Resting in the order book                 |
| `Filled`   | Fully filled                              |
| `Canceled` | Canceled                                  |

Both orders should reach `Filled` once the matching engine has ticked. The engine typically processes within a few seconds — if you see `Pending`, wait a moment and re-run the query.

Re-check balances to confirm the trade settled. The seller's newly received quote `free` should be `price × quantity = 100_000_000_000`, and the buyer's newly received base `free` should be `quantity = 10_000_000`. The pre-trade balances from §4 (seller's base, buyer's quote) should now both read 0.

```bash
# Seller's quote (credited by the fill)
icp canister call oisy_trade get_balances --args-file /dev/stdin --environment staging --query --identity "$SELLER_IDENTITY" <<EOF
(opt vec { variant { ById = record { ledger_id = principal "$QUOTE_LEDGER" } } })
EOF

# Buyer's base (credited by the fill)
icp canister call oisy_trade get_balances --args-file /dev/stdin --environment staging --query --identity "$BUYER_IDENTITY" <<EOF
(opt vec { variant { ById = record { ledger_id = principal "$BASE_LEDGER" } } })
EOF
```

## Withdraw

Debits `amount` from your on-OISY-TRADE *free* balance and sends `amount − ledger_fee` to your principal on the ledger. `amount` must be **strictly greater** than the current ledger fee — otherwise the call returns `AmountTooSmall`; re-check `icrc1_fee` if unsure. Only `free` funds are eligible; `reserved` funds (locked by open orders) aren't withdrawable until the order fills or is canceled.

After the matched trade in §5 the seller now holds quote and the buyer now holds base. Each withdraws what they received:

### Seller withdraws quote

Withdraws the seller's full `100_000_000_000` quote-base-units balance; after the ckSepoliaETH fee (`10_000_000_000`), the seller receives `90_000_000_000` on-ledger.

```bash
icp canister call oisy_trade withdraw --args-file /dev/stdin --environment staging --identity "$SELLER_IDENTITY" <<EOF
(
    record {
        token_id = record { ledger_id = principal "$QUOTE_LEDGER" };
        amount   = 100_000_000_000 : nat
    }
)
EOF
```

### Buyer withdraws base

Withdraws the buyer's full `10_000_000` base-base-units balance; after the ckDevnetSOL fee (`50`), the buyer receives `9_999_950` on-ledger.

```bash
icp canister call oisy_trade withdraw --args-file /dev/stdin --environment staging --identity "$BUYER_IDENTITY" <<EOF
(
    record {
        token_id = record { ledger_id = principal "$BASE_LEDGER" };
        amount   = 10_000_000 : nat
    }
)
EOF
```