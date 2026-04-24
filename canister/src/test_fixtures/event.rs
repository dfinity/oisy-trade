use crate::order::{
    LotSize, OrderBookId, OrderId, OrderSeq, OrderStatus, PairToken, Price, Quantity, Side,
    TickSize, TokenId, TokenMetadata,
};
use crate::state::event::{
    AddLimitOrderEvent, AddTradingPairEvent, BalanceOperation, CancelLimitOrderEvent, DepositEvent,
    Event, EventType, MatchingEvent, OrderStatusTransition, SettlingEvent, WithdrawEvent,
};
use candid::Principal;
use dex_types_internal::{InitArg, Mode, UpgradeArg};

use super::{LOT_SIZE, TICK_SIZE, base_metadata, quote_metadata};

pub fn init_event(mode: Mode) -> Event {
    Event {
        timestamp: 0,
        payload: EventType::Init(InitArg { mode }),
    }
}

pub fn upgrade_event(mode: Option<Mode>) -> Event {
    Event {
        timestamp: 1,
        payload: EventType::Upgrade(UpgradeArg { mode }),
    }
}

pub fn add_trading_pair_event(base: Principal, quote: Principal) -> Event {
    Event {
        timestamp: 2,
        payload: EventType::AddTradingPair(AddTradingPairEvent {
            book_id: OrderBookId::ZERO,
            base: TokenId::new(base),
            quote: TokenId::new(quote),
            tick_size: TICK_SIZE,
            lot_size: LOT_SIZE,
            base_metadata: base_metadata(),
            quote_metadata: quote_metadata(),
        }),
    }
}

/// Adding a new variant to `EventType` will cause a compile error in the `From` impl,
/// reminding you to add corresponding worst-case entries.
#[derive(strum::EnumIter, strum::IntoStaticStr)]
pub enum WorstCaseEvent {
    Init,
    Upgrade,
    AddTradingPair,
    Deposit,
    Withdraw,
    AddLimitOrder,
    CancelLimitOrder,
    Matching,
    Settling,
}

impl From<&EventType> for WorstCaseEvent {
    fn from(event: &EventType) -> Self {
        match event {
            EventType::Init(_) => Self::Init,
            EventType::Upgrade(_) => Self::Upgrade,
            EventType::AddTradingPair(_) => Self::AddTradingPair,
            EventType::Deposit(_) => Self::Deposit,
            EventType::Withdraw(_) => Self::Withdraw,
            EventType::AddLimitOrder(_) => Self::AddLimitOrder,
            EventType::CancelLimitOrder(_) => Self::CancelLimitOrder,
            EventType::Matching(_) => Self::Matching,
            EventType::Settling(_) => Self::Settling,
        }
    }
}

/// Upper bound on orders processed per matching round, used to size the
/// worst-case `Matching` / `Settling` fixtures. Matches the
/// `bench_process_pending_orders_1000` benchmark workload.
pub const MAX_ORDERS_PER_MATCHING_ROUND: usize = 1_000;

impl WorstCaseEvent {
    /// Event that maximizes serialized byte size in stable memory.
    pub fn worst_case_memory_event(&self) -> Event {
        max_timestamp(match self {
            Self::Init => init_restricted(),
            Self::Upgrade => upgrade_restricted(),
            Self::AddTradingPair => add_trading_pair(),
            Self::Deposit => deposit(max_quantity()),
            Self::Withdraw => withdraw(max_quantity()),
            Self::AddLimitOrder => add_limit_order(),
            Self::CancelLimitOrder => cancel_limit_order(),
            Self::Matching => matching(MAX_ORDERS_PER_MATCHING_ROUND),
            Self::Settling => settling(MAX_ORDERS_PER_MATCHING_ROUND),
        })
    }

    /// Event that maximizes instruction count during encoding/decoding.
    pub fn worst_case_instructions_event(&self) -> Event {
        // currently same as worst-case memory event
        self.worst_case_memory_event()
    }

    pub fn expected_memory_size(&self) -> usize {
        match self {
            Self::Init => 328,
            Self::Upgrade => 328,
            Self::AddTradingPair => 136,
            Self::Deposit => 95,
            Self::Withdraw => 104,
            Self::AddLimitOrder => 97,
            Self::CancelLimitOrder => 35,
            Self::Matching => 10_027,
            Self::Settling => 105_330,
        }
    }
}

fn max_timestamp(payload: EventType) -> Event {
    Event {
        timestamp: u64::MAX,
        payload,
    }
}

fn restricted_principals() -> std::collections::BTreeSet<Principal> {
    (0u8..10).map(max_principal).collect()
}

fn init_restricted() -> EventType {
    EventType::Init(InitArg {
        mode: Mode::RestrictedTo(restricted_principals()),
    })
}

fn upgrade_restricted() -> EventType {
    EventType::Upgrade(UpgradeArg {
        mode: Some(Mode::RestrictedTo(restricted_principals())),
    })
}

fn add_trading_pair() -> EventType {
    EventType::AddTradingPair(AddTradingPairEvent {
        book_id: OrderBookId::new(u64::MAX),
        base: TokenId::new(max_principal(0)),
        quote: TokenId::new(max_principal(1)),
        tick_size: TickSize::new(std::num::NonZeroU64::new(u64::MAX).unwrap()),
        lot_size: LotSize::new(std::num::NonZeroU64::new(u64::MAX).unwrap()),
        base_metadata: TokenMetadata {
            symbol: max_symbol(),
            decimals: u8::MAX,
        },
        quote_metadata: TokenMetadata {
            symbol: max_symbol(),
            decimals: u8::MAX,
        },
    })
}

fn add_limit_order() -> EventType {
    EventType::AddLimitOrder(AddLimitOrderEvent {
        user: max_principal(0),
        order_id: OrderId::new(OrderBookId::new(u64::MAX), OrderSeq::new(u64::MAX)),
        side: Side::Buy,
        price: Price::new(u64::MAX),
        quantity: max_quantity(),
    })
}

fn cancel_limit_order() -> EventType {
    EventType::CancelLimitOrder(CancelLimitOrderEvent {
        order_id: OrderId::new(OrderBookId::new(u64::MAX), OrderSeq::new(u64::MAX)),
    })
}

fn deposit(amount: Quantity) -> EventType {
    EventType::Deposit(DepositEvent {
        user: max_principal(0),
        token: TokenId::new(max_principal(1)),
        amount,
    })
}

fn withdraw(amount: Quantity) -> EventType {
    EventType::Withdraw(WithdrawEvent {
        block_index: u64::MAX,
        user: max_principal(0),
        token: TokenId::new(max_principal(1)),
        amount,
    })
}

fn matching(order_count: usize) -> EventType {
    EventType::Matching(MatchingEvent {
        book_id: OrderBookId::new(u64::MAX),
        orders: (0..order_count as u64)
            .map(|i| OrderSeq::new(u64::MAX - i))
            .collect(),
    })
}

fn settling(order_count: usize) -> EventType {
    // Each fill produces 3 operations (quote transfer, quote unreserve for
    // buy-taker price improvement, base transfer) — the maximum per fill.
    let mut balance_operations = Vec::with_capacity(order_count * 3);
    for i in 0..order_count as u64 {
        let taker = OrderSeq::new(2 * i);
        let maker = OrderSeq::new(2 * i + 1);
        balance_operations.push(BalanceOperation::Transfer {
            from_order: taker,
            to_order: maker,
            token: PairToken::Quote,
            amount: max_quantity(),
        });
        balance_operations.push(BalanceOperation::Unreserve {
            order: taker,
            token: PairToken::Quote,
            amount: max_quantity(),
        });
        balance_operations.push(BalanceOperation::Transfer {
            from_order: maker,
            to_order: taker,
            token: PairToken::Base,
            amount: max_quantity(),
        });
    }
    let transitions = (0..order_count as u64)
        .map(|i| OrderStatusTransition {
            seq: OrderSeq::new(u64::MAX - i),
            status: OrderStatus::Filled,
        })
        .collect();
    EventType::Settling(SettlingEvent {
        book_id: OrderBookId::new(u64::MAX),
        balance_operations,
        transitions,
    })
}

fn max_principal(seed: u8) -> Principal {
    Principal::from_slice(&[seed; 29])
}

fn max_symbol() -> String {
    "A".repeat(10)
}

fn max_quantity() -> Quantity {
    // EVM-based chains use theoretically u256,
    // but note that for example ETH has a supply of 120 million,
    // which comfortably fits in a u128 (18 decimals).
    Quantity::try_from(candid::Nat::from(u128::MAX)).unwrap()
}
