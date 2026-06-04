use crate::order::{Price, TokenMetadata};
use crate::state;
use crate::state::State;
use ic_metrics_encoder::MetricsEncoder;

pub fn encode_metrics(w: &mut MetricsEncoder<Vec<u8>>) -> std::io::Result<()> {
    const WASM_PAGE_SIZE: f64 = 65_536.0;

    w.encode_gauge(
        "cycle_balance",
        ic_cdk::api::canister_cycle_balance() as f64,
        "Current cycle balance of the canister.",
    )?;

    w.encode_gauge(
        "stable_memory_bytes",
        ic_cdk::api::stable_size() as f64 * WASM_PAGE_SIZE,
        "Stable memory size in bytes.",
    )?;

    w.encode_gauge(
        "heap_memory_bytes",
        heap_memory_size_bytes() as f64,
        "Size of the heap memory allocated by this canister.",
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

        let order_book_metrics = OrderBookMetrics::from_state(s);
        encode_order_book_metrics(w, &order_book_metrics)?;

        encode_fee_balances(w, s)?;

        Ok(())
    })
}

/// Per-token canister-owned fee pool. Emitted as a gauge (not a counter)
/// because withdrawals can decrease the value.
fn encode_fee_balances<MH, MB>(
    w: &mut MetricsEncoder<Vec<u8>>,
    state: &State<MH, MB>,
) -> std::io::Result<()>
where
    MH: ic_stable_structures::Memory,
    MB: ic_stable_structures::Memory,
{
    let mut metric = w.gauge_vec(
        "fee_balance",
        "Per-token canister-owned fee pool balance, accrued from maker/taker fees on fills.",
    )?;
    for (token, amount) in state.iter_fee_balances() {
        let symbol = state
            .token_metadata(&token)
            .map(format_token_symbol)
            .unwrap_or_else(|| token.as_principal().to_text());
        metric = metric.value(&[("token", &symbol)], amount_to_f64(amount))?;
    }
    Ok(())
}

/// Lossy narrowing for metrics only. Real value lives in stable memory and
/// is queried via Candid where the full `Nat` precision is preserved.
fn amount_to_f64(q: crate::order::Quantity) -> f64 {
    let nat: candid::Nat = q.into();
    // `candid::Nat`'s `Display` impl emits underscore separators (e.g.
    // "1_000_000") which `f64::from_str` rejects → NaN. Strip them so the
    // metric carries the actual value.
    nat.to_string()
        .replace('_', "")
        .parse::<f64>()
        .unwrap_or(f64::NAN)
}

/// Returns the amount of heap memory in bytes that has been allocated.
#[cfg(target_arch = "wasm32")]
pub fn heap_memory_size_bytes() -> usize {
    const WASM_PAGE_SIZE_BYTES: usize = 65536;
    core::arch::wasm32::memory_size(0) * WASM_PAGE_SIZE_BYTES
}

#[cfg(not(any(target_arch = "wasm32")))]
pub fn heap_memory_size_bytes() -> usize {
    0
}

pub struct OrderBookMetrics {
    pub base_token: String,
    pub quote_token: String,
    pub bid: Option<Price>,
    pub ask: Option<Price>,
    pub pending_orders_len: usize,
    pub resting_orders_len: usize,
}

impl OrderBookMetrics {
    pub fn from_state<MH, MB>(state: &State<MH, MB>) -> Vec<OrderBookMetrics>
    where
        MH: ic_stable_structures::Memory,
        MB: ic_stable_structures::Memory,
    {
        let mut metrics = Vec::with_capacity(state.trading_pairs().len());
        for (pair, book_id) in state.trading_pairs().iter() {
            let book = state.order_book(book_id).expect("BUG: missing order book");
            let base = state
                .token_metadata(&pair.base)
                .expect("BUG: missing token metadata");
            let quote = state
                .token_metadata(&pair.quote)
                .expect("BUG: missing token metadata");
            metrics.push(OrderBookMetrics {
                base_token: format_token_symbol(base),
                quote_token: format_token_symbol(quote),
                bid: book.bid_levels(1).next().map(|(price, _depth)| price),
                ask: book.ask_levels(1).next().map(|(price, _depth)| price),
                pending_orders_len: book.pending_orders_len(),
                resting_orders_len: book.resting_orders_len(),
            });
        }
        metrics
    }

    pub fn labels(&self) -> [(&str, &str); 2] {
        [("base", &self.base_token), ("quote", &self.quote_token)]
    }
}

pub fn encode_order_book_metrics(
    w: &mut MetricsEncoder<Vec<u8>>,
    books: &[OrderBookMetrics],
) -> std::io::Result<()> {
    {
        let mut bid_metric = w.gauge_vec("bid", "Best bid (highest-priced buy level)")?;
        for book in books {
            if let Some(bid) = &book.bid {
                bid_metric = bid_metric.value(&book.labels(), bid.get() as f64)?;
            }
        }
    }
    {
        let mut ask_metric = w.gauge_vec("ask", "Best ask (lowest-priced sell level)")?;
        for book in books {
            if let Some(ask) = &book.ask {
                ask_metric = ask_metric.value(&book.labels(), ask.get() as f64)?;
            }
        }
    }
    {
        let mut pending_metric = w.gauge_vec(
            "pending_orders",
            "Number of pending orders waiting to be matched.",
        )?;
        for book in books {
            pending_metric =
                pending_metric.value(&book.labels(), book.pending_orders_len as f64)?;
        }
    }
    {
        let mut resting_metric =
            w.gauge_vec("resting_orders", "Number of resting orders on the book.")?;
        for book in books {
            resting_metric =
                resting_metric.value(&book.labels(), book.resting_orders_len as f64)?;
        }
    }
    Ok(())
}

fn format_token_symbol(token: &TokenMetadata) -> String {
    token.symbol.to_ascii_uppercase()
}
