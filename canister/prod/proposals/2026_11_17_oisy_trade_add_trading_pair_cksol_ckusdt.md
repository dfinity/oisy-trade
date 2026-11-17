# Proposal to add the ckSOL/ckUSDT trading pair to OISY TRADE

Repository: `https://github.com/dfinity/oisy-trade.git`

Git hash of the deployed canister: `5ce6b8cb347e33ee085e06950e79233d01f9babb` ([v0.4.0](https://github.com/dfinity/oisy-trade/releases/tag/oisy_trade_canister-v0.4.0))

Target canister: `sy2xe-miaaa-aaaar-qb7sq-cai`

Method: `add_trading_pair`

Args hash: `5eb177ec25b95b11c0fc478f9ccee2953eea6b1ea77ce1b2cd8387caf0b9182e`

---

## Motivation

List ckSOL against ckUSDT, the quote token of all existing pairs.
The tick and lot sizes follow the sizing guidelines in [`docs/src/usage/for-admins.md`](../../../docs/src/usage/for-admins.md#guidelines-for-picking-values) and match Binance `SOLUSDT`:
at a SOL price of about $72, the tick is about 1.4 bp and one lot is worth about $0.07, so the `$5` min notional does the dust filtering.
Fees and notional bounds are the same as for the existing ICP/ckUSDT and ckBTC/ckUSDT pairs.

## Parameters

| Parameter     | Value                                                                    |
|---------------|--------------------------------------------------------------------------|
| Base          | ckSOL, ledger `ls5lp-lqaaa-aaaar-qb5oa-cai`, 9 decimals                  |
| Quote         | ckUSDT, ledger `cngnf-vqaaa-aaaar-qag4q-cai`, 6 decimals                 |
| Tick size     | `0.01` USDT = `10_000`                                                   |
| Lot size      | `0.001` SOL = `1_000_000`                                                |
| Min notional  | `5` USDT = `5_000_000`                                                   |
| Max notional  | `9_000_000` USDT = `9_000_000_000_000`                                   |
| Maker fee     | `0` bps                                                                  |
| Taker fee     | `20` bps                                                                 |

Settlement exactness: `tick_size × lot_size = 10^10` is a multiple of `10^base_decimals = 10^9`.

Comparison with centralized exchanges (as of 2026-11-17):

| Source             | Tick (price increment) | Lot (qty increment) | Min notional |
|--------------------|------------------------|---------------------|--------------|
| OISY TRADE         | `0.01`                 | `0.001`             | `$5`         |
| Binance `SOLUSDT`  | `0.01`                 | `0.001`             | `$5`         |
| Coinbase `SOL-USD` | `0.01`                 | `0.00000001`        | `$1`         |
| Kraken `SOLUSD`    | `0.01`                 | `0.00000001`        | `$0.50`      |

## Arguments

```candid
(
  record {
    base = record {
      id = record { ledger_id = principal "ls5lp-lqaaa-aaaar-qb5oa-cai" };
      metadata = record { symbol = "ckSOL"; decimals = 9 : nat8 };
    };
    quote = record {
      id = record { ledger_id = principal "cngnf-vqaaa-aaaar-qag4q-cai" };
      metadata = record { symbol = "ckUSDT"; decimals = 6 : nat8 };
    };
    tick_size = 10_000 : nat;
    lot_size = 1_000_000 : nat;
    maker_fee_bps = 0 : nat16;
    taker_fee_bps = 20 : nat16;
    min_notional = 5_000_000 : nat;
    max_notional = opt (9_000_000_000_000 : nat);
  },
)
```

## Verification

Verify that the ledgers report the submitted symbols and decimals.

```
icp canister call ls5lp-lqaaa-aaaar-qb5oa-cai icrc1_symbol '()' --query --network ic --identity anonymous
icp canister call ls5lp-lqaaa-aaaar-qb5oa-cai icrc1_decimals '()' --query --network ic --identity anonymous
icp canister call cngnf-vqaaa-aaaar-qag4q-cai icrc1_symbol '()' --query --network ic --identity anonymous
icp canister call cngnf-vqaaa-aaaar-qag4q-cai icrc1_decimals '()' --query --network ic --identity anonymous
```

Verify that the hash of the encoded arguments matches the args hash above.
The arguments are read from the Candid block of this file, and the interface from the deployed commit.

```
git fetch
PROPOSAL=canister/prod/proposals/2026_11_17_oisy_trade_add_trading_pair_cksol_ckusdt.md
didc encode \
  -d <(git show 5ce6b8cb347e33ee085e06950e79233d01f9babb:canister/oisy_trade.did) \
  -t '(AddTradingPairRequest)' \
  "$(sed -n '/^```candid$/,/^```$/{//!p}' "$PROPOSAL")" \
  | xxd -r -p > args.bin
sha256sum args.bin
```

## Execution

A controller produces `args.bin` as in the verification step, checks its hash, and submits exactly those bytes.

```
icp canister call sy2xe-miaaa-aaaar-qb7sq-cai add_trading_pair --args-file args.bin --args-format bin --network ic --identity "$IDENTITY"
icp canister call sy2xe-miaaa-aaaar-qb7sq-cai get_trading_pairs '()' --query --network ic --identity anonymous
```
