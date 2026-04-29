use super::{DashboardTemplate, saturating_to_u128};
use crate::order::{OrderBookId, OrderId, PendingOrder, Price, Quantity, Side, TradingPair};
use crate::state::{StableMemoryOptions, State};
use crate::test_fixtures::mocks::mock_runtime_for;
use crate::test_fixtures::{
    self, LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata,
};
use askama::Template;
use candid::Principal;
use dex_types_internal::Mode;
use ic_stable_structures::VectorMemory;
use scraper::{Html, Selector};

const TEST_CANISTER: Principal = Principal::from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

#[test]
fn should_render_title() {
    let dom = render(&fresh_state(), 0);
    assert_eq!(text(&dom, "h1"), "DEX Dashboard");
}

#[test]
fn should_render_canister_id_mode_and_event_count() {
    let dom = render(&fresh_state(), 42);
    let dl_text = text(&dom, "h2 + dl");
    assert!(
        dl_text.contains(&TEST_CANISTER.to_string()),
        "canister id missing in: {dl_text}"
    );
    assert!(
        dl_text.contains("GeneralAvailability"),
        "mode missing in: {dl_text}"
    );
    assert!(dl_text.contains("42"), "total events missing in: {dl_text}");
}

#[test]
fn should_render_restricted_mode_with_principals() {
    let mut state = fresh_state();
    state.set_mode(Mode::restricted_to(vec![
        Principal::from_slice(&[0x01]),
        Principal::from_slice(&[0x02]),
    ]));
    let dom = render(&state, 0);
    let dl_text = text(&dom, "h2 + dl");
    assert!(
        dl_text.contains("RestrictedTo"),
        "expected RestrictedTo in: {dl_text}"
    );
    assert!(
        dl_text.contains(&Principal::from_slice(&[0x01]).to_string()),
        "expected first principal in: {dl_text}"
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
    assert!(symbols.contains(&"ICP".to_string()), "{symbols:?}");
    assert!(symbols.contains(&"ckBTC".to_string()), "{symbols:?}");
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
    state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

    let dom = render(&state, 0);
    let dl_text = text(&dom, "section.pair dl");
    assert!(dl_text.contains(&format!("{}", TICK_SIZE.get())));
    assert!(dl_text.contains(&format!("{}", LOT_SIZE.get())));
    assert!(dl_text.contains("100"), "best bid 100 in: {dl_text}");
    assert!(dl_text.contains("110"), "best ask 110 in: {dl_text}");
    assert!(
        dl_text.contains(&format!("{}", 110u64 - 100u64)),
        "spread 10 in: {dl_text}"
    );
}

#[test]
fn should_render_depth_chart_for_resting_orders() {
    let mut state = fresh_state();
    record_pair(&mut state);
    place(&mut state, principal(0x01), Side::Buy, 100, lot(1));
    place(&mut state, principal(0x02), Side::Sell, 110, lot(1));
    state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

    let dom = render(&state, 0);

    let bid_prices = column(&dom, "table.depth-bids td.price");
    assert_eq!(bid_prices, vec!["100"]);
    let ask_prices = column(&dom, "table.depth-asks td.price");
    assert_eq!(ask_prices, vec!["110"]);
    let bid_qtys = column(&dom, "table.depth-bids tbody tr td:nth-child(2)");
    assert_eq!(bid_qtys, vec![candid::Nat::from(lot(1)).to_string()]);

    let bar_widths: Vec<String> = dom
        .select(&sel("td.bar div"))
        .map(|d| d.value().attr("style").unwrap_or("").to_string())
        .collect();
    assert_eq!(bar_widths, vec!["width: 100%", "width: 100%"]);
}

#[test]
fn should_normalize_depth_bar_widths_against_max_quantity() {
    let mut state = fresh_state();
    record_pair(&mut state);
    place(&mut state, principal(0x01), Side::Buy, 90, lot(1));
    place(&mut state, principal(0x02), Side::Buy, 80, lot(2));
    place(&mut state, principal(0x03), Side::Sell, 110, lot(4));
    state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

    let dom = render(&state, 0);
    let bar_widths: Vec<String> = dom
        .select(&sel("td.bar div"))
        .map(|d| d.value().attr("style").unwrap_or("").to_string())
        .collect();
    assert_eq!(
        bar_widths,
        vec!["width: 25%", "width: 50%", "width: 100%"],
        "bid 1 lot → 25%, bid 2 lots → 50%, ask 4 lots → 100%"
    );
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
    price: u64,
    quantity: u64,
) -> OrderId {
    let pair: TradingPair = icp_ckbtc_trading_pair();
    let pending = PendingOrder {
        side,
        price: Price::new(price),
        quantity: Quantity::from(quantity),
    };
    let (token, required) = match pending.side {
        Side::Buy => (
            pair.quote,
            pending
                .price
                .checked_mul_quantity(&pending.quantity)
                .unwrap(),
        ),
        Side::Sell => (pair.base, pending.quantity),
    };
    state.deposit(user, token, required, StableMemoryOptions::Write);
    let (order_id, order) = state.validate_limit_order(user, pair, pending).unwrap();
    state.record_limit_order(user, order_id.book_id(), order, StableMemoryOptions::Write);
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
