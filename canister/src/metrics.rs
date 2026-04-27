use crate::{order, state};
use ic_metrics_encoder::MetricsEncoder;

pub fn encode_metrics(w: &mut MetricsEncoder<Vec<u8>>) -> std::io::Result<()> {
    const WASM_PAGE_SIZE: f64 = 65_536.0;

    // Canister-level metrics
    w.encode_gauge(
        "canister_cycle_balance",
        ic_cdk::api::canister_cycle_balance() as f64,
        "Current cycle balance of the canister.",
    )?;
    let stable_pages = ic_cdk::api::stable_size() as f64;
    w.encode_gauge(
        "canister_stable_memory_pages",
        stable_pages,
        "Number of stable memory pages allocated.",
    )?;
    w.encode_gauge(
        "canister_stable_memory_bytes",
        stable_pages * WASM_PAGE_SIZE,
        "Stable memory size in bytes.",
    )?;

    // Event log
    w.encode_counter(
        "event_total",
        crate::storage::total_event_count() as f64,
        "Total number of events in the stable log.",
    )?;

    state::with_state(|s| -> std::io::Result<()> {
        // Trading pair count
        w.encode_gauge(
            "trading_pair_count",
            s.trading_pair_count() as f64,
            "Number of registered trading pairs.",
        )?;

        // Per-pair order book metrics
        {
            let mut bid_levels = w.gauge_vec(
                "order_book_bid_levels",
                "Number of distinct bid price levels.",
            )?;
            for (pair, book_id) in s.trading_pairs().iter() {
                let book = s.order_book(book_id).expect("BUG: missing order book");
                let pair_label = format_pair(s, pair);
                bid_levels = bid_levels.value(&[("pair", &pair_label)], book.bids_len() as f64)?;
            }
        }
        {
            let mut ask_levels = w.gauge_vec(
                "order_book_ask_levels",
                "Number of distinct ask price levels.",
            )?;
            for (pair, book_id) in s.trading_pairs().iter() {
                let book = s.order_book(book_id).expect("BUG: missing order book");
                let pair_label = format_pair(s, pair);
                ask_levels = ask_levels.value(&[("pair", &pair_label)], book.asks_len() as f64)?;
            }
        }

        Ok(())
    })
}

fn format_pair<MH, MB>(s: &state::State<MH, MB>, pair: &order::TradingPair) -> String
where
    MH: ic_stable_structures::Memory,
    MB: ic_stable_structures::Memory,
{
    let base = s
        .token_metadata(&pair.base)
        .map(|m| m.symbol.as_str())
        .unwrap_or("?");
    let quote = s
        .token_metadata(&pair.quote)
        .map(|m| m.symbol.as_str())
        .unwrap_or("?");
    format!("{base}/{quote}")
}
