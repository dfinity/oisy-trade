# Interact with the DEX

Walkthrough of the core DEX flows against the staging canister, using only the [`icp`](https://cli.internetcomputer.org/) CLI:

1. List trading pairs
2. Approve and deposit
3. Place a limit order
4. Check order status
5. Withdraw

Run every command below from the same shell — steps share `export`ed variables (`BASE_LEDGER`, `DEX`, `ORDER_ID`, ...).

## Prerequisites

- [`icp` CLI](https://cli.internetcomputer.org/) installed and on `PATH`
- Run these commands from the **project root** so `--environment staging` resolves the `dex` canister name (defined in `icp.yaml`)
- An identity configured via `icp identity new` or `icp identity import` — every `deposit`, `add_limit_order`, and `withdraw` runs as the currently-selected identity, and the DEX keys its internal balances off that caller principal

## Your identity

Principal that will call the DEX and hold token balances.

```bash
icp identity default
icp identity principal
```

## Limit Orders 101

- **Trading Pair** — two token ledgers: the *base* (what you're buying or selling) and the *quote* (what prices are denominated in). `SOL / ETH` means trading SOL priced in ETH.
- **Quantity** — amount of the *base* token, in its base units (e.g. 1 SOL = 10<sup>9</sup> units at 9 decimals). Must be a positive multiple of `lot_size`.
- **Price** — *quote* base units per one *base* base unit (an integer ratio, no decimals). Must be a positive multiple of `tick_size`. A fill of `quantity` at `price` settles `price × quantity` quote-token base units.
- **Side** — `Buy` (bid) reserves `price × quantity` of the *quote* token from your free balance; `Sell` (ask) reserves `quantity` of the *base* token. The order fills when a crossing order arrives, otherwise it rests in the book until canceled.

## 1. List trading pairs

`get_trading_pairs` returns every listed pair along with its `tick_size` (price must be a positive multiple) and `lot_size` (quantity must be a positive multiple, in base-token base units).

```bash
icp canister call dex get_trading_pairs '()' --environment staging --query --identity anonymous
```

## 2. Pick a pair

Copy the base and quote ledger principals from the output above and export them. The values below are the ckDevnetSOL / ckSepoliaETH pair listed on staging — adjust if you picked a different pair.

```bash
export BASE_LEDGER=la34w-haaaa-aaaar-qb5na-cai   # ckSOL (devnet)
export QUOTE_LEDGER=apia6-jaaaa-aaaar-qabma-cai  # ckSepoliaETH
export DEX=proc5-daaaa-aaaar-qb5va-cai           # staging DEX canister

export TICK_SIZE=10_000
export LOT_SIZE=1_000_000
```

## 3. Approve and deposit

Every limit order reserves one side of the pair before the matching engine can fill it:

- **Sell** orders reserve `quantity` *base* tokens → you need base in your on-DEX *free* balance
- **Buy** orders reserve `price × quantity` *quote* tokens → you need quote in your on-DEX *free* balance

So the token you approve and deposit depends on the side you plan to trade.

`deposit` moves tokens by calling [`icrc2_transfer_from`](https://github.com/dfinity/ICRC-1/tree/main/standards/ICRC-2) on the token ledger, so the DEX must first be approved to spend on your behalf. Two fees are charged to your on-ledger balance:

1. `icrc2_approve` charges the ledger fee once
2. `icrc2_transfer_from` (triggered later by `deposit`) charges the ledger fee again, on top of the amount transferred

So to deposit `N`, approve **at least `N + ledger_fee`** and hold **at least `N + 2 × ledger_fee`** on the ledger. Check your on-ledger balance and the ledger fee for whichever column you pick:

```bash
icp token "$BASE_LEDGER" balance --network ic
icp canister call "$BASE_LEDGER" icrc1_fee '()' --query --identity anonymous --network ic
icp token "$QUOTE_LEDGER" balance --network ic
icp canister call "$QUOTE_LEDGER" icrc1_fee '()' --query --identity anonymous --network ic
```

Then approve and deposit — run the subsection matching the side you plan to trade. `icrc2_approve` calls the token ledger directly (`--network ic`); `deposit` targets the DEX on staging (`--environment staging`).

### Sell orders — deposit *base*

Deposits 1 lot (= `LOT_SIZE` base-base-units), enough for one sell at `quantity = LOT_SIZE`.

```bash
icp canister call "$BASE_LEDGER" icrc2_approve --args-file /dev/stdin --network ic <<EOF
(
    record {
        spender = record { owner = principal "$DEX"; subaccount = null };
        amount  = 1_010_000 : nat
    }
)
EOF

icp canister call dex deposit --args-file /dev/stdin --environment staging <<EOF
(
    record {
        token_id = record { ledger_id = principal "$BASE_LEDGER" };
        amount   = 1_000_000 : nat
    }
)
EOF
```

### Buy orders — deposit *quote*

Deposits `TICK_SIZE × LOT_SIZE` quote-base-units (= 10_000 × 1_000_000), enough for one buy at `price = TICK_SIZE`, `quantity = LOT_SIZE`.

```bash
icp canister call "$QUOTE_LEDGER" icrc2_approve --args-file /dev/stdin --network ic <<EOF
(
    record {
        spender = record { owner = principal "$DEX"; subaccount = null };
        amount  = 10_000_010_000 : nat
    }
)
EOF

icp canister call dex deposit --args-file /dev/stdin --environment staging <<EOF
(
    record {
        token_id = record { ledger_id = principal "$QUOTE_LEDGER" };
        amount   = 10_000_000_000 : nat
    }
)
EOF
```

> Candid's record literals use `{` / `}`, which collide with bash variable-expansion rules. Passing the candid via `--args-file /dev/stdin` and a heredoc sidesteps every shell-quoting pitfall — the unquoted `EOF` terminator still expands `$VAR` inside the body.

## 4. Check your on-DEX balance

The DEX tracks balance per `(caller, token)`:

- `free` — available for new orders or withdrawal
- `reserved` — locked by open orders

```bash
icp canister call dex get_balance --args-file /dev/stdin --environment staging --query <<EOF
(record { ledger_id = principal "$BASE_LEDGER" })
EOF
```

## 5. Place a limit order

`price` must be a positive multiple of `TICK_SIZE`; `quantity` a positive multiple of `LOT_SIZE`.

Submission is asynchronous: the call returns an order ID immediately, the matching engine processes it on the next canister tick.

```bash
icp canister call dex add_limit_order --args-file /dev/stdin --environment staging <<EOF
(
    record {
        pair     = record { base = principal "$BASE_LEDGER"; quote = principal "$QUOTE_LEDGER" };
        side     = variant { Sell };
        price    = $TICK_SIZE : nat64;
        quantity = $LOT_SIZE  : nat
    }
)
EOF
```

The response is a `variant { Ok : text }` whose `text` is the 32-char hex order ID. Export it for the next step:

```bash
export ORDER_ID=<paste-the-order-id-here>
```

## 6. Check order status

```bash
icp canister call dex get_order_status "(\"$ORDER_ID\")" --environment staging --query --identity anonymous
```

| Status     | Meaning                                   |
|------------|-------------------------------------------|
| `Pending`  | Accepted, awaiting the matching engine    |
| `Open`     | Resting in the order book                 |
| `Filled`   | Fully filled                              |
| `Canceled` | Canceled                                  |
| `NotFound` | Unknown order ID                          |

## 7. Withdraw

Debits `amount` from your on-DEX *free* balance and sends `amount − ledger_fee` to your principal on the ledger. Only `free` funds are eligible; `reserved` funds (locked by open orders) aren't withdrawable until the order fills or is canceled.

```bash
icp canister call dex withdraw --args-file /dev/stdin --environment staging <<EOF
(
    record {
        token_id = record { ledger_id = principal "$BASE_LEDGER" };
        amount   = 500_000 : nat
    }
)
EOF
```

## What's next

- Inspect the append-only event log via `get_events` — every state change (listings, deposits, orders) is recorded. See `canister/dex.did` for the full schema.
- Place a **Buy** order: flip `side = variant { Sell }` to `variant { Buy }` in step 5 and make sure you've taken the right-hand column in step 3 to deposit quote tokens.
- See `integration_tests/` for end-to-end scenarios that exercise every endpoint programmatically.
