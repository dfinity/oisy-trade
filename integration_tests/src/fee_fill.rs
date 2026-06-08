use crate::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
use crate::{LOT_SIZE, Setup};
use candid::Principal;
use dex_types::{AddTradingPairRequest, LimitOrderRequest, Side, Token};

/// Outputs of one fee-charging fill, used by tests covering both the
/// `get_fee_balances` Candid query and the `/metrics` `fee_balance` gauge.
pub struct FeeFillOutcome {
    pub base: Token,
    pub quote: Token,
    pub base_fee_raw: u64,
    pub quote_fee_raw: u64,
}

impl FeeFillOutcome {
    pub fn base_fee_whole(&self) -> String {
        format!(
            "{}",
            self.base_fee_raw as f64 / 10f64.powi(self.base.metadata.decimals as i32)
        )
    }

    pub fn quote_fee_whole(&self) -> String {
        format!(
            "{}",
            self.quote_fee_raw as f64 / 10f64.powi(self.quote.metadata.decimals as i32)
        )
    }
}

/// Stand up a trading pair with non-zero maker/taker fees and run one cross
/// so both sides accrue. Uses the default, invariant-satisfying tick/lot;
/// the `mul_ceil` rounding itself is covered by `BasisPoint` unit tests.
/// Returns the expected fee outputs and the `Setup`.
pub async fn fill_one_cross_with_fees() -> (FeeFillOutcome, Setup) {
    let setup = Setup::new().await;
    let request = AddTradingPairRequest {
        maker_fee_bps: 10,
        taker_fee_bps: 23,
        ..setup.add_trading_pair_request()
    };
    let base = request.base.clone();
    let quote = request.quote.clone();
    setup
        .dex_client_with_caller(setup.controller())
        .add_trading_pair(request)
        .await
        .unwrap();

    let seller = Principal::from_slice(&[0x01]);
    let buyer = Principal::from_slice(&[0x02]);
    let qty = LOT_SIZE; // one lot of the base token
    // Settlement divides by 10^base_decimals (ckSOL = 9 decimals), so pick a
    // price (a multiple of the tick) large enough that the settled notional
    // stays well above the fee denominator and the accrued fees are non-trivial.
    let price = 1_000_000_000_000u64;
    let notional = price * qty / 1_000_000_000;
    setup
        .deposit_flow(seller, base.id.clone())
        .mint(qty + 2 * BASE_LEDGER_FEE)
        .approve(qty + BASE_LEDGER_FEE)
        .deposit(qty)
        .execute()
        .await;
    setup
        .deposit_flow(buyer, quote.id.clone())
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

    // Buyer crossed → base (CKSOL) fee at the taker rate, quote (CKBTC) at
    // the maker rate (`mul_ceil`, matching production).
    let base_fee_raw = (qty * 23).div_ceil(10_000);
    let quote_fee_raw = (notional * 10).div_ceil(10_000);

    let outcome = FeeFillOutcome {
        base,
        quote,
        base_fee_raw,
        quote_fee_raw,
    };
    (outcome, setup)
}
