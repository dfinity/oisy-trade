#[cfg(test)]
mod tests;

use crate::order::{OrderBook, Price, Quantity};
use crate::state::State;
use askama::Template;
use candid::Principal;
use ic_stable_structures::Memory;
use oisy_trade_types_internal::Mode;

const DEPTH_LEVELS: usize = 20;

#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub canister_id: Principal,
    pub mode: String,
    pub total_events: u64,
    pub tokens: Vec<DashboardToken>,
    pub pairs: Vec<DashboardPair>,
}

pub struct DashboardToken {
    pub ledger_id: Principal,
    pub symbol: String,
    pub decimals: u8,
}

pub struct DashboardPair {
    pub book_id: u64,
    pub base_symbol: String,
    pub quote_symbol: String,
    pub tick_size: u128,
    pub lot_size: u64,
    pub maker_fee_bps: u16,
    pub taker_fee_bps: u16,
    pub bids_len: usize,
    pub asks_len: usize,
    pub pending_orders_len: usize,
    pub resting_orders_len: usize,
    pub best_bid: Option<DashboardLevel>,
    pub best_ask: Option<DashboardLevel>,
    pub spread: Option<u128>,
    pub depth: DashboardDepth,
}

pub struct DashboardLevel {
    pub price: u128,
    pub quantity: String,
}

pub struct DashboardDepth {
    pub bids: Vec<DashboardDepthLevel>,
    pub asks: Vec<DashboardDepthLevel>,
}

impl DashboardDepth {
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }
}

pub struct DashboardDepthLevel {
    pub price: u128,
    pub quantity: String,
    pub bar_width_percent: u8,
}

impl DashboardTemplate {
    pub fn from_state<MH: Memory, MB: Memory>(
        state: &State<MH, MB>,
        canister_id: Principal,
        total_events: u64,
    ) -> Self {
        let tokens = state
            .tokens()
            .iter()
            .map(|(token_id, metadata)| DashboardToken {
                ledger_id: *token_id.as_principal(),
                symbol: metadata.symbol.clone(),
                decimals: metadata.decimals,
            })
            .collect();
        let pairs = state
            .trading_pairs()
            .iter()
            .map(|(pair, book_id)| {
                let book = state
                    .order_book(book_id)
                    .expect("BUG: trading pair registered but order book missing");
                let base_symbol = state
                    .token_metadata(&pair.base)
                    .expect("BUG: base token metadata missing")
                    .symbol
                    .clone();
                let quote_symbol = state
                    .token_metadata(&pair.quote)
                    .expect("BUG: quote token metadata missing")
                    .symbol
                    .clone();
                build_pair(book.id().get(), base_symbol, quote_symbol, book)
            })
            .collect();
        Self {
            canister_id,
            mode: format_mode(state.mode()),
            total_events,
            tokens,
            pairs,
        }
    }
}

fn build_pair(
    book_id: u64,
    base_symbol: String,
    quote_symbol: String,
    book: &OrderBook,
) -> DashboardPair {
    let bids: Vec<(Price, Quantity)> = book.bid_levels(DEPTH_LEVELS).collect();
    let asks: Vec<(Price, Quantity)> = book.ask_levels(DEPTH_LEVELS).collect();
    let best_bid_level = bids.first().copied();
    let best_ask_level = asks.first().copied();
    let best_bid = best_bid_level.map(level);
    let best_ask = best_ask_level.map(level);
    let spread = match (best_bid_level, best_ask_level) {
        (Some((bid, _)), Some((ask, _))) => ask.checked_sub(bid).map(Price::get),
        _ => None,
    };
    DashboardPair {
        book_id,
        base_symbol,
        quote_symbol,
        tick_size: book.tick_size().get(),
        lot_size: book.lot_size().get(),
        maker_fee_bps: book.fee_rates().maker.get(),
        taker_fee_bps: book.fee_rates().taker.get(),
        bids_len: book.bids_len(),
        asks_len: book.asks_len(),
        pending_orders_len: book.pending_orders_len(),
        resting_orders_len: book.resting_orders_len(),
        best_bid,
        best_ask,
        spread,
        depth: build_depth(&bids, &asks),
    }
}

fn build_depth(bids: &[(Price, Quantity)], asks: &[(Price, Quantity)]) -> DashboardDepth {
    let max = bids
        .iter()
        .chain(asks.iter())
        .map(|(_, q)| saturating_to_u128(q))
        .max()
        .unwrap_or(0);
    DashboardDepth {
        bids: depth_levels(bids, max),
        asks: depth_levels(asks, max),
    }
}

fn depth_levels(levels: &[(Price, Quantity)], max: u128) -> Vec<DashboardDepthLevel> {
    levels
        .iter()
        .map(|(price, qty)| DashboardDepthLevel {
            price: price.get(),
            quantity: qty.to_nat().to_string(),
            bar_width_percent: bar_width_percent(saturating_to_u128(qty), max),
        })
        .collect()
}

fn bar_width_percent(qty: u128, max: u128) -> u8 {
    if max == 0 {
        return 0;
    }
    let percent = match qty.checked_mul(100) {
        Some(scaled) => scaled / max,
        None => qty / (max / 100),
    };
    percent.min(100) as u8
}

fn level((price, quantity): (Price, Quantity)) -> DashboardLevel {
    DashboardLevel {
        price: price.get(),
        quantity: quantity.to_nat().to_string(),
    }
}

fn saturating_to_u128(q: &Quantity) -> u128 {
    if q.high() != 0 { u128::MAX } else { q.low() }
}

fn format_mode(mode: &Mode) -> String {
    match mode {
        Mode::GeneralAvailability => "GeneralAvailability".to_string(),
        Mode::RestrictedTo(principals) if principals.is_empty() => {
            "RestrictedTo: (none)".to_string()
        }
        Mode::RestrictedTo(principals) => {
            let list = principals
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("RestrictedTo: {list}")
        }
    }
}
