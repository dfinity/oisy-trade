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

        // Unique users
        w.encode_gauge(
            "unique_user_count",
            s.unique_user_count() as f64,
            "Number of principals with a balance entry.",
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

        // Order count by pair and status (from OrderHistory)
        {
            use std::collections::HashMap;
            let mut counts: HashMap<(order::TradingPair, &'static str), u64> = HashMap::new();
            for (id, record) in s.order_history().iter() {
                let book_id = id.book_id();
                let Some(pair) = s.trading_pairs().get_pair(&book_id) else {
                    continue;
                };
                let status_label = format_status(record.status.into());
                *counts.entry((pair.clone(), status_label)).or_default() += 1;
            }
            let mut sorted: Vec<_> = counts.into_iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let mut order_count = w.gauge_vec(
                "order_count",
                "Number of orders by trading pair and status.",
            )?;
            for ((pair, status), count) in &sorted {
                let pair_label = format_pair(s, pair);
                order_count = order_count
                    .value(&[("pair", &pair_label), ("status", status)], *count as f64)?;
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

fn format_status(status: dex_types::OrderStatus) -> &'static str {
    match status {
        dex_types::OrderStatus::NotFound => "not_found",
        dex_types::OrderStatus::Pending => "pending",
        dex_types::OrderStatus::Open => "open",
        dex_types::OrderStatus::Filled => "filled",
        dex_types::OrderStatus::Canceled => "canceled",
    }
}
