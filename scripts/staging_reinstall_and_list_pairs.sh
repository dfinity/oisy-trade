#!/usr/bin/env bash
#
# Reinstall the staging OISY TRADE canister and re-list the launch pairs.
#
# Mirrors docs/src/usage/for-admins.md. Steps:
#   1) Reinstall the canister (WIPES stable memory).
#   2) Add trading pair ckDevnetSOL/ckSepoliaETH.
#   3) Add trading pair TESTICP/ckSepoliaUSDC.
#   4) List trading pairs (verify both appear).
#
# Requires: the `icp` CLI on PATH, and an identity that is a controller of the
# canister. Run from anywhere — the script cd's to the repo root so
# `--environment staging` resolves the `oisy_trade` canister name from icp.yaml.
#
# Config via environment variables:
#   IDENTITY     controller identity              (default: hsm)
#   ENVIRONMENT  icp.yaml environment             (default: staging)
#   PIN_FILE     HSM PIN / PEM password file      (option A; else prompted)
#   NO_PASSWORD  set to 1 for an unencrypted PEM  (drops --identity-password-file)
#   SKIP_BUILD   set to 1 to skip `just build`
#   FORCE        set to 1 to skip the reinstall confirmation prompt

set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

IDENTITY="${IDENTITY:-hsm}"
ENVIRONMENT="${ENVIRONMENT:-staging}"

# This script hardcodes testnet ledgers, params, and pairs. Refuse to run
# against any other environment (e.g. prod) regardless of FORCE.
if [[ "$ENVIRONMENT" != "staging" ]]; then
    echo "ERROR: this script only supports ENVIRONMENT=staging (got '$ENVIRONMENT')." >&2
    echo "It hardcodes testnet ledgers and pairs and would wipe/mislist any other environment." >&2
    exit 1
fi

PIN_FILE="${PIN_FILE:-}"

# Build the auth flags shared by every signing call. Unlock secret handling
# follows the doc: point at an existing file (PIN_FILE), prompt once into a
# chmod-600 temp file, or drop the flag entirely for an unencrypted PEM.
cleanup_pin=false
cleanup() { if [[ "$cleanup_pin" == true ]]; then rm -f "$PIN_FILE"; fi; }
trap cleanup EXIT

declare -a AUTH=(--identity "$IDENTITY")
if [[ "${NO_PASSWORD:-0}" != "1" ]]; then
    if [[ -z "$PIN_FILE" ]]; then
        PIN_FILE="$(mktemp -t icp-identity-XXXXXX)"
        chmod 600 "$PIN_FILE"
        cleanup_pin=true
        read -rs -p "Unlock $IDENTITY (HSM PIN or PEM password): " pin && echo
        printf '%s' "$pin" > "$PIN_FILE"
        unset pin
    fi
    AUTH+=(--identity-password-file "$PIN_FILE")
fi

# Adds a trading pair from the exported BASE_*/QUOTE_*/TICK_SIZE/... variables.
add_trading_pair() {
    icp canister call oisy_trade add_trading_pair --args-file /dev/stdin \
        "${AUTH[@]}" --environment "$ENVIRONMENT" <<EOF
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
        tick_size      = $TICK_SIZE      : nat;
        lot_size       = $LOT_SIZE       : nat;
        maker_fee_bps  = $MAKER_FEE_BPS  : nat16;
        taker_fee_bps  = $TAKER_FEE_BPS  : nat16;
        min_notional   = $MIN_NOTIONAL   : nat;
        max_notional   = $MAX_NOTIONAL
    }
)
EOF
}

# 1) Reinstall the canister (wipes stable memory).
echo "==> [1/4] Reinstall oisy_trade on '$ENVIRONMENT' (WIPES stable memory)"
if [[ "${FORCE:-0}" != "1" ]]; then
    read -r -p "Reinstall WIPES all balances, orders, and listed pairs on '$ENVIRONMENT'. Type 'yes' to continue: " confirm
    [[ "$confirm" == "yes" ]] || { echo "Aborted."; exit 1; }
fi
if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
    just build
fi
icp deploy oisy_trade --mode reinstall "${AUTH[@]}" --environment "$ENVIRONMENT" -y

# 2) Add trading pair ckDevnetSOL/ckSepoliaETH.
echo "==> [2/4] Add trading pair ckDevnetSOL/ckSepoliaETH"
export BASE_LEDGER=la34w-haaaa-aaaar-qb5na-cai   # ckDevnetSOL
export QUOTE_LEDGER=apia6-jaaaa-aaaar-qabma-cai  # ckSepoliaETH
export BASE_SYMBOL=ckDevnetSOL
export BASE_DECIMALS=9
export QUOTE_SYMBOL=ckSepoliaETH
export QUOTE_DECIMALS=18
export TICK_SIZE=10_000_000_000_000                                  # 0.00001 ETH × 10^18
export LOT_SIZE=1_000_000                                            # 0.001 SOL × 10^9
export MAKER_FEE_BPS=0
export TAKER_FEE_BPS=20
export MIN_NOTIONAL=1_000_000_000_000_000                            # 0.001 ETH × 10^18
export MAX_NOTIONAL='opt (9_000_000_000_000_000_000_000_000 : nat)'  # 9_000_000 ETH × 10^18
add_trading_pair

# 3) Add trading pair TESTICP/ckSepoliaUSDC.
echo "==> [3/4] Add trading pair TESTICP/ckSepoliaUSDC"
export BASE_LEDGER=xafvr-biaaa-aaaai-aql5q-cai   # TESTICP
export QUOTE_LEDGER=yfumr-cyaaa-aaaar-qaela-cai  # ckSepoliaUSDC
export BASE_SYMBOL=TESTICP
export BASE_DECIMALS=8
export QUOTE_SYMBOL=ckSepoliaUSDC
export QUOTE_DECIMALS=6
export TICK_SIZE=1_000          # 0.001 USDC × 10^6
export LOT_SIZE=1_000_000       # 0.01 TESTICP × 10^8
export MAKER_FEE_BPS=0
export TAKER_FEE_BPS=20
export MIN_NOTIONAL=5_000_000   # $5 × 10^6
export MAX_NOTIONAL='null'      # no ceiling
add_trading_pair

# 4) List trading pairs (verify both appear).
echo "==> [4/4] List trading pairs"
icp canister call oisy_trade get_trading_pairs '()' \
    --environment "$ENVIRONMENT" --query --identity anonymous
