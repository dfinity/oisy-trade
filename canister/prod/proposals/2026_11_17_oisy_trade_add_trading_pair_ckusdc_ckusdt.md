# Proposal to add the ckUSDC/ckUSDT trading pair to OISY TRADE

Repository: `https://github.com/dfinity/oisy-trade.git`

Git hash of the deployed canister: `5ce6b8cb347e33ee085e06950e79233d01f9babb` ([v0.4.0](https://github.com/dfinity/oisy-trade/releases/tag/oisy_trade_canister-v0.4.0))

Target canister: `sy2xe-miaaa-aaaar-qb7sq-cai`

Method: `add_trading_pair`

Args hash: `a2e837e443baa0b17ff4070bb0eb19c2e80ee2ee0e42b4e307e362578b79dd4a`

---

## Motivation

List ckUSDC against ckUSDT, the quote token of all existing pairs, so that users can swap between the two USD stablecoins.
The parameters are the ones of the ckUSDC/ckUSDT row of the launch basket in [`docs/src/usage/for-admins.md`](../../../docs/src/usage/for-admins.md#launch-basket), reviewed in [#140](https://github.com/dfinity/oisy-trade/pull/140).
Fees and notional bounds are the same as for the existing ICP/ckUSDT and ckBTC/ckUSDT pairs.

## Parameters

| Parameter     | Value                                                                    |
|---------------|--------------------------------------------------------------------------|
| Base          | ckUSDC, ledger `xevnm-gaaaa-aaaar-qafnq-cai`, 6 decimals                 |
| Quote         | ckUSDT, ledger `cngnf-vqaaa-aaaar-qag4q-cai`, 6 decimals                 |
| Tick size     | `0.00001` USDT = `10`                                                    |
| Lot size      | `1` USDC = `1_000_000`                                                   |
| Min notional  | `5` USDT = `5_000_000`                                                   |
| Max notional  | `9_000_000` USDT = `9_000_000_000_000`                                   |
| Maker fee     | `0` bps                                                                  |
| Taker fee     | `20` bps                                                                 |

Settlement exactness: `tick_size × lot_size = 10^7` is a multiple of `10^base_decimals = 10^6`.

Comparison with centralized exchanges (as of 2026-11-17):

| Source                 | Tick (price increment) | Lot (qty increment) | Min notional |
|------------------------|------------------------|---------------------|--------------|
| OISY TRADE             | `0.00001`              | `1`                 | `$5`         |
| Binance `USDCUSDT`     | `0.00001`              | `1`                 | `$5`         |
| Coinbase `USDT-USDC` ‡ | `0.0001` (inverted)    | `0.01`              | `$1`         |
| Kraken `USDCUSDT`      | `0.0001`               | `0.00000001`        | `$0.50`      |

‡ Coinbase's `USDT-USDC` quotes USDT *in USDC*, the inverse direction of ckUSDC/ckUSDT.

## Arguments

```candid
(
  record {
    base = record {
      id = record { ledger_id = principal "xevnm-gaaaa-aaaar-qafnq-cai" };
      metadata = record { symbol = "ckUSDC"; decimals = 6 : nat8 };
    };
    quote = record {
      id = record { ledger_id = principal "cngnf-vqaaa-aaaar-qag4q-cai" };
      metadata = record { symbol = "ckUSDT"; decimals = 6 : nat8 };
    };
    tick_size = 10 : nat;
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
icp canister call xevnm-gaaaa-aaaar-qafnq-cai icrc1_symbol '()' --query --network ic --identity anonymous
icp canister call xevnm-gaaaa-aaaar-qafnq-cai icrc1_decimals '()' --query --network ic --identity anonymous
icp canister call cngnf-vqaaa-aaaar-qag4q-cai icrc1_symbol '()' --query --network ic --identity anonymous
icp canister call cngnf-vqaaa-aaaar-qag4q-cai icrc1_decimals '()' --query --network ic --identity anonymous
```

Verify that the hash of the encoded arguments matches the args hash above.
The arguments are read from the Candid block of this file at `REF`, and the interface from the deployed commit.
`didc` is available from the [`didc-v0.6.2`](https://github.com/dfinity/candid/releases/tag/didc-v0.6.2) release of `dfinity/candid`.

```
git fetch origin
REF=origin/main  # while this proposal is under review: the PR branch, e.g. origin/<branch>
PROPOSAL=canister/prod/proposals/2026_11_17_oisy_trade_add_trading_pair_ckusdc_ckusdt.md
didc encode \
  -d <(git show 5ce6b8cb347e33ee085e06950e79233d01f9babb:canister/oisy_trade.did) \
  -t '(AddTradingPairRequest)' \
  "$(git show "$REF:$PROPOSAL" | sed -n '/^```candid$/,/^```$/{//!p;}')" \
  > args.hex
xxd -r -p args.hex | sha256sum
```

## Execution

`sy2xe-miaaa-aaaar-qb7sq-cai` is controlled by the Orbit station `cmqvo-qqaaa-aaaai-q3waa-cai` and the NNS root, so the call is submitted as an Orbit request to call an external canister, which executes once approved.

A requester produces `args.hex` as in the verification step and submits exactly those bytes,
with [`dfx-orbit`](https://github.com/dfinity/orbit/tree/main/tools/dfx-orbit) or in the Orbit web UI.
The arguments are passed with `--raw-arg` and not with `--arg-file`, which `dfx-orbit` reads as Candid text and re-encodes without the interface.

```
dfx-orbit request canister call sy2xe-miaaa-aaaar-qb7sq-cai add_trading_pair --raw-arg "$(cat args.hex)"
```

Each approver produces `args.hex` from the merged proposal (`REF=origin/main`), and checks that the request targets `add_trading_pair` of `sy2xe-miaaa-aaaar-qb7sq-cai` and that its argument checksum equals the args hash above:

```
dfx-orbit verify <REQUEST_ID> canister call sy2xe-miaaa-aaaar-qb7sq-cai add_trading_pair --raw-arg "$(cat args.hex)"
```

After execution, check that the pair is listed:

```
icp canister call sy2xe-miaaa-aaaar-qb7sq-cai get_trading_pairs '()' --query --network ic --identity anonymous
```
