use crate::Setup;
use crate::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
use candid::Principal;
use dex_types::{AddTradingPairRequest, LimitOrderRequest, Side, TokenId};

/// Outputs of one fee-charging fill, used by tests covering both the
/// `get_fee_balances` Candid query and the `/metrics` `fee_balance` gauge.
pub struct FeeFillOutcome {
    pub base: TokenId,
    pub quote: TokenId,
    pub base_fee_raw: u64,
    pub quote_fee_raw: u64,
    base_decimals: u8,
    quote_decimals: u8,
}

impl FeeFillOutcome {
    pub fn base_fee_whole(&self) -> String {
        format!(
            "{}",
            self.base_fee_raw as f64 / 10f64.powi(self.base_decimals as i32)
        )
    }

    pub fn quote_fee_whole(&self) -> String {
        format!(
            "{}",
            self.quote_fee_raw as f64 / 10f64.powi(self.quote_decimals as i32)
        )
    }
}

/// Stand up a trading pair with non-zero, non-trivial maker/taker fees
/// (chosen so the fee math has a non-zero remainder — distinguishes
/// `mul_ceil` from `mul_floor`), and run one cross so both sides accrue.
/// Returns the expected fee outputs and the `Setup`.
pub async fn fill_one_cross_with_fees() -> (FeeFillOutcome, Setup) {
    let setup = Setup::new().await;
    let request = AddTradingPairRequest {
        // 10 bps maker, 23 bps taker — `qty * 23` is not a multiple of
        // 10_000 below, so `mul_ceil` and `mul_floor` would disagree.
        maker_fee_bps: 10,
        taker_fee_bps: 23,
        // lot_size=1 so the non-lot-aligned `qty` below validates.
        lot_size: 1,
        ..setup.add_trading_pair_request()
    };
    setup
        .dex_client_with_caller(setup.controller())
        .add_trading_pair(request)
        .await
        .unwrap();

    let seller = Principal::from_slice(&[0x01]);
    let buyer = Principal::from_slice(&[0x02]);
    let base = setup.base_token_id();
    let quote = setup.quote_token_id();
    // qty not a multiple of 10_000 so `qty * taker_bps / 10_000` has a
    // remainder, exposing `mul_ceil` vs `mul_floor`.
    let qty = 1_000_001u64;
    let price = 100u64;
    let notional = price * qty;
    setup
        .deposit_flow(seller, base.clone())
        .mint(qty + 2 * BASE_LEDGER_FEE)
        .approve(qty + BASE_LEDGER_FEE)
        .deposit(qty)
        .execute()
        .await;
    setup
        .deposit_flow(buyer, quote.clone())
        .mint(notional + 2 * QUOTE_LEDGER_FEE)
        .approve(notional + QUOTE_LEDGER_FEE)
        .deposit(notional)
        .execute()
        .await;
    setup
        .dex_client_with_caller(seller)
        .add_limit_order(LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Sell,
            price,
            quantity: qty.into(),
        })
        .await
        .unwrap();
    setup
        .dex_client_with_caller(buyer)
        .add_limit_order(LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Buy,
            price,
            quantity: qty.into(),
        })
        .await
        .unwrap();
    setup.env().tick().await;

    // Buyer crossed → base (CKSOL) fee at taker rate, quote (CKBTC) at
    // maker rate. Ceiling math per spec — `qty * 23` produces a
    // non-zero remainder mod 10_000.
    let base_fee_raw = (qty * 23).div_ceil(10_000);
    let quote_fee_raw = (notional * 10).div_ceil(10_000);

    let outcome = FeeFillOutcome {
        base: base.clone(),
        quote: quote.clone(),
        base_fee_raw,
        quote_fee_raw,
        // Token symbol "ckSOL" (decimals 9) for base, "ckBTC" (decimals 8) for quote
        // — see `integration_tests/src/lib.rs::add_trading_pair_request`.
        base_decimals: 9,
        quote_decimals: 8,
    };
    (outcome, setup)
}
