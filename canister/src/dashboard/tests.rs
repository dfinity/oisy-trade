use super::{DashboardTemplate, bar_width_percent, format_scaled, saturating_to_u128};
use crate::order::{
    BasisPoint, FeeRates, OrderBookId, OrderId, PendingOrder, Price, Quantity, Side, TimeInForce,
    TradingPair,
};
use crate::state::{StableMemoryOptions, State};
use crate::test_fixtures::mocks::mock_runtime_for;
use crate::test_fixtures::{
    self, LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, ckbtc_metadata,
    ckbtc_token_id, icp_ckbtc_trading_pair, icp_metadata, icp_token_id,
};
use askama::Template;
use candid::Principal;
use ic_stable_structures::VectorMemory;
use scraper::{Html, Selector};

const TEST_CANISTER: Principal = Principal::from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
const MAKER_FEE_BPS: u16 = 7;
const TAKER_FEE_BPS: u16 = 23;

#[test]
fn should_render_metadata() {
    let dom = render(&fresh_state(), 42);

    let title = text(&dom, "h1");
    assert_eq!(title, "OISY TRADE Dashboard");

    let metadata = text(&dom, "h2 + dl");
    assert!(
        metadata.contains(&TEST_CANISTER.to_string()),
        "canister id missing in: {metadata}"
    );
    assert!(
        metadata.contains("GeneralAvailability"),
        "mode missing in: {metadata}"
    );
    assert!(
        metadata.contains("42"),
        "total events missing in: {metadata}"
    );
}

#[test]
fn should_render_registered_tokens() {
    let mut state = fresh_state();
    record_pair(&mut state);

    let dom = render(&state, 0);

    let symbols: Vec<String> = dom
        .select(&sel("table tbody tr td:first-child"))
        .map(|td| td.text().collect::<String>())
        .collect();
    assert_eq!(symbols, vec!["ICP", "ckBTC"]);
}

#[test]
fn should_link_canister_id_to_icp_dashboard() {
    let dom = render(&fresh_state(), 0);

    assert_eq!(
        hrefs(&dom, "h2 + dl a"),
        vec![icp_dashboard_url(&TEST_CANISTER)]
    );
}

#[test]
fn should_link_token_ledgers_to_icp_dashboard() {
    let mut state = fresh_state();
    record_pair(&mut state);

    let dom = render(&state, 0);

    assert_eq!(
        hrefs(&dom, "table tbody td a"),
        vec![
            icp_dashboard_url(icp_token_id().as_principal()),
            icp_dashboard_url(ckbtc_token_id().as_principal()),
        ]
    );
}

#[test]
fn should_render_empty_pair_section_when_no_orders() {
    let mut state = fresh_state();
    record_pair(&mut state);
    let dom = render(&state, 0);
    assert_eq!(text(&dom, "section.pair h3"), "ICP/ckBTC (book #0)");
    assert_eq!(text(&dom, "section.pair p.muted"), "Order book is empty.");
    assert!(
        dom.select(&sel("table.depth-bids")).next().is_none(),
        "no depth tables when book is empty"
    );
}

#[test]
fn should_render_per_pair_metadata() {
    let mut state = fresh_state();
    record_pair(&mut state);
    place(&mut state, principal(0x01), Side::Buy, 100, lot(1));
    place(&mut state, principal(0x02), Side::Sell, 110, lot(1));
    crate::EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

    let dom = render(&state, 0);
    let dl_text = text(&dom, "section.pair dl");
    let dts = column(&dom, "section.pair dl dt");
    let dds = column(&dom, "section.pair dl dd");
    let value_for = |label: &str| {
        dts.iter()
            .zip(&dds)
            .find(|(dt, _)| dt.trim() == label)
            .map(|(_, dd)| dd.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    let expected_maker = format!("{MAKER_FEE_BPS} bps");
    let expected_taker = format!("{TAKER_FEE_BPS} bps");
    assert_eq!(
        value_for("Maker fee").as_deref(),
        Some(expected_maker.as_str())
    );
    assert_eq!(
        value_for("Taker fee").as_deref(),
        Some(expected_taker.as_str())
    );
    assert_eq!(
        value_for("Tick size").as_deref(),
        Some("0.000001 ckBTC/ICP raw 100")
    );
    assert_eq!(
        value_for("Lot size").as_deref(),
        Some("0.01 ICP raw 1000000")
    );
    assert_eq!(
        value_for("Best bid").as_deref(),
        Some("100 ckBTC/ICP (0.01 ICP) raw 10000000000 / 1000000")
    );
    assert_eq!(
        value_for("Best ask").as_deref(),
        Some("110 ckBTC/ICP (0.01 ICP) raw 11000000000 / 1000000")
    );
    assert_eq!(
        value_for("Spread").as_deref(),
        Some("10 ckBTC/ICP raw 1000000000")
    );
    assert!(
        dl_text.contains("0.000001"),
        "formatted tick size in: {dl_text}"
    );
}

#[test]
fn should_render_depth_chart_for_resting_orders() {
    let mut state = fresh_state();
    record_pair(&mut state);
    place(&mut state, principal(0x01), Side::Buy, 100, lot(1));
    place(&mut state, principal(0x02), Side::Sell, 110, lot(1));
    crate::EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

    let dom = render(&state, 0);

    let bid_prices = cells(&dom, "table.depth-bids td.price");
    assert_eq!(bid_prices, vec!["100 10000000000"]);
    let ask_prices = cells(&dom, "table.depth-asks td.price");
    assert_eq!(ask_prices, vec!["110 11000000000"]);
    let bid_qtys = cells(&dom, "table.depth-bids tbody tr td:nth-child(2)");
    assert_eq!(bid_qtys, vec!["0.01 1000000"]);

    assert_eq!(bar_widths(&dom), vec!["width: 100%", "width: 100%"]);
}

#[test]
fn should_normalize_depth_bar_widths_against_max_quantity() {
    let mut state = fresh_state();
    record_pair(&mut state);
    place(&mut state, principal(0x01), Side::Buy, 90, lot(1));
    place(&mut state, principal(0x02), Side::Buy, 80, lot(2));
    place(&mut state, principal(0x03), Side::Sell, 110, lot(4));
    crate::EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

    let dom = render(&state, 0);
    assert_eq!(
        bar_widths(&dom),
        vec!["width: 25%", "width: 50%", "width: 100%"],
        "bid 1 lot → 25%, bid 2 lots → 50%, ask 4 lots → 100%"
    );
}

#[test]
fn should_compute_bar_width_percent_without_overflow() {
    assert_eq!(bar_width_percent(0, 0), 0);
    assert_eq!(bar_width_percent(50, 0), 0);
    assert_eq!(bar_width_percent(0, 100), 0);
    assert_eq!(bar_width_percent(50, 100), 50);
    assert_eq!(bar_width_percent(100, 100), 100);
    assert_eq!(bar_width_percent(150, 100), 100);

    assert_eq!(bar_width_percent(u128::MAX, u128::MAX), 100);
    let half = bar_width_percent(u128::MAX / 2, u128::MAX);
    assert!((49..=50).contains(&half), "expected ~50%, got {half}");
}

#[test]
fn should_saturate_quantity_to_u128() {
    assert_eq!(saturating_to_u128(&Quantity::ZERO), 0);
    assert_eq!(saturating_to_u128(&Quantity::from(1u64)), 1);
    assert_eq!(
        saturating_to_u128(&Quantity::from(u64::MAX)),
        u128::from(u64::MAX)
    );
    assert_eq!(
        saturating_to_u128(&Quantity::from_u128(u128::MAX)),
        u128::MAX
    );
    assert_eq!(saturating_to_u128(&Quantity::new(1, 0)), u128::MAX);
    assert_eq!(saturating_to_u128(&Quantity::MAX), u128::MAX);
}

#[test]
fn should_format_scaled_with_zero_decimals_as_passthrough() {
    assert_eq!(format_scaled("12345", 0), "12345");
}

#[test]
fn should_format_scaled_sub_one_with_leading_zero() {
    assert_eq!(format_scaled("1000000", 9), "0.001");
}

#[test]
fn should_format_scaled_trimming_trailing_zeros() {
    assert_eq!(format_scaled("1000000000000000000", 18), "1");
}

#[test]
fn should_format_scaled_exact_mid_value() {
    assert_eq!(format_scaled("50000000000000000", 18), "0.05");
}

#[test]
fn should_format_scaled_u256_quantity_without_precision_loss() {
    let raw = Quantity::new(1, 0).to_nat().to_string().replace('_', "");
    assert_eq!(
        raw, "340282366920938463463374607431768211456",
        "Quantity::new(1, 0) is 2^128"
    );
    assert_eq!(
        format_scaled(&raw, 18),
        "340282366920938463463.374607431768211456"
    );
}

#[test]
fn should_format_scaled_stripping_underscores() {
    assert_eq!(format_scaled("1_000_000", 6), "1");
}

fn fresh_state() -> State<VectorMemory, VectorMemory> {
    test_fixtures::state()
}

fn record_pair(state: &mut State<VectorMemory, VectorMemory>) {
    state.record_trading_pair(
        OrderBookId::ZERO,
        icp_ckbtc_trading_pair(),
        icp_metadata(),
        ckbtc_metadata(),
        TICK_SIZE,
        LOT_SIZE,
        MIN_NOTIONAL,
        Some(MAX_NOTIONAL),
        FeeRates {
            maker: BasisPoint::new(MAKER_FEE_BPS).unwrap(),
            taker: BasisPoint::new(TAKER_FEE_BPS).unwrap(),
        },
    );
}

fn render(state: &State<VectorMemory, VectorMemory>, total_events: u64) -> Html {
    let html = DashboardTemplate::from_state(state, TEST_CANISTER, total_events)
        .render()
        .expect("dashboard template should render");
    Html::parse_document(&html)
}

fn place(
    state: &mut State<VectorMemory, VectorMemory>,
    user: Principal,
    side: Side,
    price: u128,
    quantity: u64,
) -> OrderId {
    let pair: TradingPair = icp_ckbtc_trading_pair();
    let pending = PendingOrder {
        side,
        price: Price::new(price * PRICE_SCALE),
        quantity: Quantity::from(quantity),
        time_in_force: TimeInForce::GoodTilCanceled,
    };
    let (token, required) = match pending.side {
        Side::Buy => (
            pair.quote,
            pending
                .price
                .checked_mul_quantity_scaled(&pending.quantity, state.base_scale(&pair.base))
                .unwrap(),
        ),
        Side::Sell => (pair.base, pending.quantity),
    };
    state.deposit(user, token, required, StableMemoryOptions::Write);
    let (order_id, order) = state.validate_limit_order(user, pair, pending).unwrap();
    state.record_limit_order(
        user,
        order_id.book_id(),
        order,
        crate::Timestamp::EPOCH,
        StableMemoryOptions::Write,
    );
    order_id
}

fn principal(byte: u8) -> Principal {
    Principal::from_slice(&[byte])
}

fn lot(n: u64) -> u64 {
    n * u64::from(LOT_SIZE)
}

fn sel(selector: &str) -> Selector {
    Selector::parse(selector).unwrap_or_else(|e| panic!("bad selector `{selector}`: {e:?}"))
}

fn text(dom: &Html, selector: &str) -> String {
    dom.select(&sel(selector))
        .next()
        .unwrap_or_else(|| panic!("no element matched `{selector}`"))
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn column(dom: &Html, selector: &str) -> Vec<String> {
    dom.select(&sel(selector))
        .map(|e| e.text().collect::<String>())
        .collect()
}

fn cells(dom: &Html, selector: &str) -> Vec<String> {
    dom.select(&sel(selector))
        .map(|e| {
            e.text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn bar_widths(dom: &Html) -> Vec<String> {
    dom.select(&sel("td.bar div"))
        .map(|d| d.value().attr("style").unwrap_or("").to_string())
        .collect()
}

fn hrefs(dom: &Html, selector: &str) -> Vec<String> {
    dom.select(&sel(selector))
        .filter_map(|a| a.value().attr("href").map(str::to_string))
        .collect()
}

fn icp_dashboard_url(principal: &Principal) -> String {
    format!("https://dashboard.internetcomputer.org/canister/{principal}")
}
