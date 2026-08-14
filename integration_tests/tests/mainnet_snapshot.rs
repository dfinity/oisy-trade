use candid::{Decode, Encode, Principal};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{StableLog, VectorMemory};
use oisy_trade_canister::balance::TokenBalance;
use oisy_trade_canister::order::{self, OrderHistory, TradeHistory};
use oisy_trade_canister::state::StableMemoryOptions;
use oisy_trade_canister::state::audit::replay_events;
use oisy_trade_canister::state::event::Event as StoredEvent;
use oisy_trade_canister::user::UserRegistry;
use oisy_trade_client::OisyTradeClient;
use oisy_trade_int_tests::{
    CONTROLLER, PocketIcRuntime, download_and_extract_snapshot, load_snapshot_into_pocketic,
    oisy_trade_wasm,
};
use oisy_trade_types::{
    GetBalancesError, GetMyOrdersArgs, GetMyOrdersError, GetOrderBookDepthError,
    GetOrderBookDepthRequest, MAX_DEPTH_LIMIT, OrderBookDepth, PriceLevel, UserOrder,
    UserTokenBalance,
};
use oisy_trade_types_internal::OisyTradeArg;
use oisy_trade_types_internal::event::{Event, EventType, GetEventsArgs, GetEventsResult};
use pocket_ic::nonblocking::PocketIc;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

const EVENT_LOG_INDEX: MemoryId = MemoryId::new(0);
const EVENT_LOG_DATA: MemoryId = MemoryId::new(1);
const ORDER_HISTORY: MemoryId = MemoryId::new(2);
const BALANCES: MemoryId = MemoryId::new(3);
const USER_REGISTRY: MemoryId = MemoryId::new(5);
const USER_ORDERS: MemoryId = MemoryId::new(6);
const TRADES: MemoryId = MemoryId::new(7);
const TRADES_BY_USER: MemoryId = MemoryId::new(8);
const TRADING_ACCOUNTS: MemoryId = MemoryId::new(9);
const TRADING_ACCOUNTS_BY_FUNDING: MemoryId = MemoryId::new(10);

type Mem = VirtualMemory<VectorMemory>;

type Collections = (
    OrderHistory<Mem>,
    TradeHistory<Mem>,
    UserRegistry<Mem>,
    TokenBalance<Mem>,
);

/// The per-user state we require to survive the upgrade unchanged.
type UserState = (
    Result<Vec<UserTokenBalance>, GetBalancesError>,
    Result<Vec<UserOrder>, GetMyOrdersError>,
);

#[tokio::test]
async fn should_load_mainnet_snapshot_and_upgrade_to_current_wasm() {
    let snapshot_dir = download_and_extract_snapshot();
    let (env, canister_id) = load_snapshot_into_pocketic(snapshot_dir).await;

    let users = users_from_events(&all_events(&env, canister_id).await);
    assert!(
        !users.is_empty(),
        "mainnet snapshot should reference at least one user"
    );
    let state_before = collect_user_state(&env, canister_id, &users).await;

    env.stop_canister(canister_id, Some(CONTROLLER))
        .await
        .expect("failed to stop canister before upgrade");
    env.upgrade_canister(
        canister_id,
        oisy_trade_wasm(),
        Encode!(&OisyTradeArg::Upgrade(None)).unwrap(),
        Some(CONTROLLER),
    )
    .await
    .expect("failed to upgrade mainnet snapshot to the current wasm");
    env.start_canister(canister_id, Some(CONTROLLER))
        .await
        .expect("failed to start canister after upgrade");

    let state_after = collect_user_state(&env, canister_id, &users).await;
    assert_eq!(
        state_before, state_after,
        "user balances and orders must be identical after the upgrade"
    );

    env.drop().await;
}

#[tokio::test]
async fn should_replay_mainnet_events_and_reconstruct_state() {
    let snapshot_dir = download_and_extract_snapshot();

    let stable_memory = std::fs::read(snapshot_dir.join("stable_memory.bin"))
        .expect("the snapshot must contain stable_memory.bin");
    let snapshot_mem: VectorMemory = Rc::new(RefCell::new(stable_memory));
    let snapshot_mm = MemoryManager::init(snapshot_mem);

    let event_log: StableLog<StoredEvent, Mem, Mem> = StableLog::init(
        snapshot_mm.get(EVENT_LOG_INDEX),
        snapshot_mm.get(EVENT_LOG_DATA),
    );
    assert!(
        !event_log.is_empty(),
        "the snapshot event log must not be empty"
    );

    let (snap_orders, snap_trades, snap_users, snap_balances) = open_collections(&snapshot_mm);

    let recon_mem: VectorMemory = Rc::new(RefCell::new(Vec::new()));
    let recon_mm = MemoryManager::init(recon_mem);
    let (order_history, trade_history, user_registry, balances) = open_collections(&recon_mm);

    let state = replay_events(
        event_log.iter(),
        order_history,
        trade_history,
        user_registry,
        balances,
        StableMemoryOptions::Write,
    );

    let (recon_orders, recon_trades, recon_users, recon_balances) = open_collections(&recon_mm);

    assert_entries_eq(
        "token balances",
        recon_balances.iter().collect(),
        snap_balances.iter().collect(),
    );
    assert_entries_eq(
        "order history",
        recon_orders.iter().collect(),
        snap_orders.iter().collect(),
    );
    assert_entries_eq(
        "user orders",
        recon_orders.iter_by_user().collect(),
        snap_orders.iter_by_user().collect(),
    );
    assert_entries_eq(
        "trades",
        recon_trades.iter().collect(),
        snap_trades.iter().collect(),
    );
    assert_entries_eq(
        "trades by user",
        recon_trades.iter_by_user().collect(),
        snap_trades.iter_by_user().collect(),
    );
    assert_entries_eq(
        "user registry",
        recon_users.iter_users().collect(),
        snap_users.iter_users().collect(),
    );
    assert_entries_eq(
        "trading accounts",
        recon_users.iter_trading_accounts().collect(),
        snap_users.iter_trading_accounts().collect(),
    );
    assert_entries_eq(
        "trading accounts by funding",
        recon_users.iter_trading_accounts_by_funding().collect(),
        snap_users.iter_trading_accounts_by_funding().collect(),
    );

    assert!(
        !state.trading_pairs().is_empty(),
        "the mainnet snapshot must have at least one trading pair for the order-book cross-check"
    );

    let (env, canister_id) = load_snapshot_into_pocketic(snapshot_dir).await;

    for (pair, book_id) in state.trading_pairs().iter() {
        let book = state
            .order_book(book_id)
            .expect("BUG: trading pair registered but order book missing");
        let limit = MAX_DEPTH_LIMIT as usize;
        let reconstructed = OrderBookDepth {
            bids: book.bid_levels(limit).map(to_price_level).collect(),
            asks: book.ask_levels(limit).map(to_price_level).collect(),
        };
        let request = GetOrderBookDepthRequest {
            trading_pair: oisy_trade_types::TradingPair::from(pair.clone()),
            limit: Some(MAX_DEPTH_LIMIT),
        };
        let bytes = env
            .query_call(
                canister_id,
                Principal::anonymous(),
                "get_order_book_depth",
                Encode!(&request).unwrap(),
            )
            .await
            .expect("get_order_book_depth query failed");
        let from_canister = Decode!(&bytes, Result<OrderBookDepth, GetOrderBookDepthError>)
            .unwrap()
            .expect("get_order_book_depth returned an error");
        assert_eq!(
            reconstructed, from_canister,
            "order-book depth mismatch for pair {pair:?}"
        );
    }

    env.drop().await;
}

async fn collect_user_state(
    env: &PocketIc,
    canister_id: Principal,
    users: &BTreeSet<Principal>,
) -> BTreeMap<Principal, UserState> {
    let mut state = BTreeMap::new();
    for &user in users {
        let client = OisyTradeClient::new(PocketIcRuntime::new(env, user), canister_id);
        let balances = client.get_balances(None).await;
        let orders = client.get_my_orders(GetMyOrdersArgs::default()).await;
        state.insert(user, (balances, orders));
    }
    state
}

fn users_from_events(events: &[Event]) -> BTreeSet<Principal> {
    let mut users = BTreeSet::new();
    for event in events {
        match &event.payload {
            EventType::Deposit(e) => {
                users.insert(e.user);
            }
            EventType::Withdraw(e) => {
                users.insert(e.user);
            }
            EventType::AddLimitOrder(e) => {
                users.insert(e.user);
                users.extend(e.placed_by);
            }
            EventType::AddTradingAccount(e) => {
                users.insert(e.funding);
                users.insert(e.trading);
            }
            EventType::RemoveTradingAccount(e) => {
                users.insert(e.funding);
                users.insert(e.trading);
            }
            _ => {}
        }
    }
    users
}

async fn all_events(env: &PocketIc, canister_id: Principal) -> Vec<Event> {
    const BATCH: u64 = 2000;
    let mut events = Vec::new();
    loop {
        let GetEventsResult {
            events: batch,
            total_event_count,
        } = get_events(env, canister_id, events.len() as u64, BATCH).await;
        events.extend(batch);
        if events.len() as u64 >= total_event_count {
            break;
        }
    }
    events
}

async fn get_events(
    env: &PocketIc,
    canister_id: Principal,
    start: u64,
    length: u64,
) -> GetEventsResult {
    let bytes = env
        .query_call(
            canister_id,
            Principal::anonymous(),
            "get_events",
            Encode!(&GetEventsArgs { start, length }).unwrap(),
        )
        .await
        .expect("get_events query failed");
    Decode!(&bytes, GetEventsResult).unwrap()
}

fn open_collections(mm: &MemoryManager<VectorMemory>) -> Collections {
    (
        OrderHistory::new(mm.get(ORDER_HISTORY), mm.get(USER_ORDERS)),
        TradeHistory::new(mm.get(TRADES), mm.get(TRADES_BY_USER)),
        UserRegistry::new(
            mm.get(USER_REGISTRY),
            mm.get(TRADING_ACCOUNTS),
            mm.get(TRADING_ACCOUNTS_BY_FUNDING),
        ),
        TokenBalance::new(mm.get(BALANCES)),
    )
}

fn assert_entries_eq<T: PartialEq + std::fmt::Debug>(
    name: &str,
    reconstructed: Vec<T>,
    snapshot: Vec<T>,
) {
    assert_eq!(
        reconstructed.len(),
        snapshot.len(),
        "{name}: reconstructed has {} entries but the snapshot has {}",
        reconstructed.len(),
        snapshot.len()
    );
    for (index, (recon, snap)) in reconstructed.iter().zip(snapshot.iter()).enumerate() {
        assert_eq!(
            recon, snap,
            "{name}: entry {index} differs (reconstructed vs snapshot)"
        );
    }
}

fn to_price_level((price, quantity): (order::Price, order::Quantity)) -> PriceLevel {
    PriceLevel {
        price: candid::Nat::from(price),
        quantity: quantity.into(),
    }
}
