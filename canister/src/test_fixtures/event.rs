use crate::order::{
    LotSize, OrderBookId, OrderId, OrderSeq, Price, Quantity, Side, TickSize, TokenId,
    TokenMetadata,
};
use crate::state::event::{
    AddLimitOrderEvent, AddTradingPairEvent, DepositEvent, Event, EventType,
};
use candid::Principal;
use dex_types_internal::{InitArg, Mode, UpgradeArg};

/// Adding a new variant to `EventType` will cause a compile error in the `From` impl,
/// reminding you to add corresponding worst-case entries.
#[derive(strum::EnumIter, strum::IntoStaticStr)]
pub enum WorstCaseEvent {
    Init,
    Upgrade,
    AddTradingPair,
    Deposit,
    AddLimitOrder,
}

impl From<&EventType> for WorstCaseEvent {
    fn from(event: &EventType) -> Self {
        match event {
            EventType::Init(_) => Self::Init,
            EventType::Upgrade(_) => Self::Upgrade,
            EventType::AddTradingPair(_) => Self::AddTradingPair,
            EventType::Deposit(_) => Self::Deposit,
            EventType::AddLimitOrder(_) => Self::AddLimitOrder,
        }
    }
}

impl WorstCaseEvent {
    /// Event that maximizes serialized byte size in stable memory.
    pub fn worst_case_memory_event(&self) -> Event {
        max_timestamp(match self {
            Self::Init => init_restricted(),
            Self::Upgrade => upgrade_restricted(),
            Self::AddTradingPair => add_trading_pair(),
            Self::Deposit => deposit(max_quantity()),
            Self::AddLimitOrder => add_limit_order(),
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
            Self::AddLimitOrder => 97,
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

fn deposit(amount: Quantity) -> EventType {
    EventType::Deposit(DepositEvent {
        user: max_principal(0),
        token: TokenId::new(max_principal(1)),
        amount,
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
    Quantity::from(candid::Nat::from(u128::MAX))
}
