use crate::order::{
    LotSize, OrderBook, OrderBookId, PendingOrder, Price, Quantity, Side, TickSize,
};
use canbench_rs::bench;
use serde::Deserialize;
use std::num::NonZeroU64;

/// Minimum price increment for ICP/USDT on Binance: 0.001 USDT with 8 decimal places.
const TICK_SIZE: TickSize = TickSize::new(NonZeroU64::new(100_000).unwrap());
/// Minimum order quantity for ICP/USDT on Binance: 0.01 ICP with 8 decimal places.
const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());
const BOOK_ID: OrderBookId = OrderBookId::ZERO;

#[derive(Deserialize)]
struct DepthSnapshot {
    bids: Vec<(String, String)>,
    asks: Vec<(String, String)>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct AggTrade {
    p: String,
    q: String,
    m: bool,
}

/// Parse a Binance decimal string (e.g. "2.30400000") into a u64 assuming 8 decimal places.
/// Uses only integer arithmetic to avoid floating-point imprecision.
fn parse_decimal_8(s: &str) -> u64 {
    let (integer_part, fractional_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let integer: u64 = integer_part.parse().expect("invalid integer part");
    // Pad or truncate fractional part to exactly 8 digits.
    let mut frac_digits = [b'0'; 8];
    for (i, byte) in fractional_part.bytes().take(8).enumerate() {
        frac_digits[i] = byte;
    }
    let fractional: u64 = std::str::from_utf8(&frac_digits)
        .expect("ascii digits")
        .parse()
        .expect("invalid fractional part");
    integer * 100_000_000 + fractional
}

fn load_depth() -> DepthSnapshot {
    let json = include_str!("../../docs/trading_data/2026_04_04_binance_depth_ICPUSDT.json");
    serde_json::from_str(json).expect("failed to parse depth snapshot")
}

fn load_trades() -> Vec<AggTrade> {
    let json = include_str!("../../docs/trading_data/2026_04_04_binance_agg_trades_ICPUSDT.json");
    serde_json::from_str(json).expect("failed to parse trades")
}

/// Pre-populate an order book with resting orders from the Binance depth snapshot.
/// Best bid (2.304) < best ask (2.305), so no fills occur during population.
fn populate_book(book: &mut OrderBook, depth: &DepthSnapshot) {
    for (price_str, qty_str) in &depth.bids {
        let pending = PendingOrder {
            side: Side::Buy,
            price: Price::new(parse_decimal_8(price_str)),
            quantity: Quantity::new(parse_decimal_8(qty_str)),
        };
        book.add_pending_order(pending).expect("valid bid order");
    }
    for (price_str, qty_str) in &depth.asks {
        let pending = PendingOrder {
            side: Side::Sell,
            price: Price::new(parse_decimal_8(price_str)),
            quantity: Quantity::new(parse_decimal_8(qty_str)),
        };
        book.add_pending_order(pending).expect("valid ask order");
    }
    let fills = book.process_pending_orders();
    assert!(fills.is_empty(), "no fills expected during book population");
}

/// Benchmark processing 1000 incoming orders against a fully populated order book
/// using real Binance ICP/USDT data (697 bid levels + 5000 ask levels).
#[bench(raw)]
fn bench_process_1000_orders() -> canbench_rs::BenchResult {
    let depth = load_depth();
    let trades = load_trades();
    let mut book = OrderBook::new(BOOK_ID, TICK_SIZE, LOT_SIZE);

    populate_book(&mut book, &depth);

    // Queue 1000 pending orders from aggregated trades.
    // Binance `m` field: true = buyer is maker, so the taker is a seller.
    for trade in &trades {
        let pending = PendingOrder {
            side: if trade.m { Side::Sell } else { Side::Buy },
            price: Price::new(parse_decimal_8(&trade.p)),
            quantity: Quantity::new(parse_decimal_8(&trade.q)),
        };
        book.add_pending_order(pending).expect("valid trade order");
    }

    canbench_rs::bench_fn(|| {
        book.process_pending_orders();
    })
}

/// Benchmark processing 1000 orders that all rest without matching.
/// Wide spread between buys (2.000) and sells (3.000) ensures zero fills.
#[bench(raw)]
fn bench_process_1000_orders_no_fills() -> canbench_rs::BenchResult {
    let mut book = OrderBook::new(BOOK_ID, TICK_SIZE, LOT_SIZE);

    for i in 0..500u64 {
        let pending = PendingOrder {
            side: Side::Buy,
            price: Price::new(200_000_000), // 2.000 USDT
            quantity: Quantity::new((i + 1) * LOT_SIZE.get()),
        };
        book.add_pending_order(pending).expect("valid buy order");
    }
    for i in 0..500u64 {
        let pending = PendingOrder {
            side: Side::Sell,
            price: Price::new(300_000_000), // 3.000 USDT
            quantity: Quantity::new((i + 1) * LOT_SIZE.get()),
        };
        book.add_pending_order(pending).expect("valid sell order");
    }

    canbench_rs::bench_fn(|| {
        book.process_pending_orders();
    })
}
