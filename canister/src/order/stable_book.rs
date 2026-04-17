use super::{
    Fill, LotSize, MatchOrderError, MatchResult, MatchingOutput, Order, OrderBookId, OrderSeq,
    PendingOrder, Price, Quantity, Side, TickSize,
};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeSet, VecDeque};

// ---------------------------------------------------------------------------
// Storable key/value types
// ---------------------------------------------------------------------------

/// Composite key for bid-side entries: sorts by price **descending**, then by
/// sequence number ascending (FIFO within a price level).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BidKey {
    pub price: u64,
    pub seq: u64,
}

impl Ord for BidKey {
    fn cmp(&self, other: &Self) -> Ordering {
        other.price.cmp(&self.price).then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for BidKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Storable for BidKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&self.price.to_be_bytes());
        buf[8..].copy_from_slice(&self.seq.to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&self.price.to_be_bytes());
        buf[8..].copy_from_slice(&self.seq.to_be_bytes());
        buf.to_vec()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let price = u64::from_be_bytes(bytes[..8].try_into().unwrap());
        let seq = u64::from_be_bytes(bytes[8..].try_into().unwrap());
        Self { price, seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 16,
        is_fixed_size: true,
    };
}

/// Composite key for ask-side entries: sorts by price ascending, then by
/// sequence number ascending (FIFO within a price level).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskKey {
    pub price: u64,
    pub seq: u64,
}

impl Ord for AskKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.price.cmp(&other.price).then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for AskKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Storable for AskKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&self.price.to_be_bytes());
        buf[8..].copy_from_slice(&self.seq.to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&self.price.to_be_bytes());
        buf[8..].copy_from_slice(&self.seq.to_be_bytes());
        buf.to_vec()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let price = u64::from_be_bytes(bytes[..8].try_into().unwrap());
        let seq = u64::from_be_bytes(bytes[8..].try_into().unwrap());
        Self { price, seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 16,
        is_fixed_size: true,
    };
}

/// A [`Quantity`] stored in stable memory with leading-zero stripping.
///
/// Small values (≤ u64) use only 1–8 bytes instead of the full 32.
/// `Quantity::from_be_bytes` already handles variable-length input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorableQuantity(pub Quantity);

/// Strip leading zero bytes from a big-endian byte slice.
fn strip_leading_zeros(bytes: &[u8]) -> &[u8] {
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[first_nonzero..]
}

impl Storable for StorableQuantity {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let full = self.0.to_be_bytes();
        Cow::Owned(strip_leading_zeros(&full).to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        let full = self.0.to_be_bytes();
        strip_leading_zeros(&full).to_vec()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        StorableQuantity(Quantity::from_be_bytes(&bytes).expect("invalid Quantity bytes"))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 32,
        is_fixed_size: false,
    };
}

/// Encodes `(Side, Price)` for the resting-order index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorableSidePrice {
    pub side: Side,
    pub price: Price,
}

impl Storable for StorableSidePrice {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; 9];
        buf[0] = match self.side {
            Side::Buy => 0,
            Side::Sell => 1,
        };
        buf[1..].copy_from_slice(&self.price.get().to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let side = match bytes[0] {
            0 => Side::Buy,
            1 => Side::Sell,
            other => panic!("invalid side byte: {other}"),
        };
        let price = Price::new(u64::from_be_bytes(bytes[1..9].try_into().unwrap()));
        StorableSidePrice { side, price }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 9,
        is_fixed_size: true,
    };
}

// ---------------------------------------------------------------------------
// StableOrderBook
// ---------------------------------------------------------------------------

/// A limit order book backed by stable memory.
///
/// Mirrors the API of [`super::OrderBook`] but stores bids, asks, and the
/// resting-order index in [`StableBTreeMap`]s instead of heap collections.
/// Pending orders and filled-order tracking remain on the heap since they are
/// transient (processed and drained every matching round).
pub struct StableOrderBook<M: Memory> {
    id: OrderBookId,
    next_seq: OrderSeq,
    tick_size: TickSize,
    lot_size: LotSize,
    pending_orders: VecDeque<Order>,
    filled_orders: BTreeSet<OrderSeq>,
    bids: StableBTreeMap<BidKey, StorableQuantity, M>,
    asks: StableBTreeMap<AskKey, StorableQuantity, M>,
    resting_orders: StableBTreeMap<u64, StorableSidePrice, M>,
}

impl<M: Memory> StableOrderBook<M> {
    pub fn new(
        id: OrderBookId,
        tick_size: TickSize,
        lot_size: LotSize,
        bids_memory: M,
        asks_memory: M,
        resting_orders_memory: M,
    ) -> Self {
        Self {
            id,
            next_seq: OrderSeq::default(),
            tick_size,
            lot_size,
            pending_orders: VecDeque::new(),
            filled_orders: BTreeSet::new(),
            bids: StableBTreeMap::init(bids_memory),
            asks: StableBTreeMap::init(asks_memory),
            resting_orders: StableBTreeMap::init(resting_orders_memory),
        }
    }

    pub fn id(&self) -> OrderBookId {
        self.id
    }

    pub fn next_seq(&self) -> OrderSeq {
        self.next_seq
    }

    pub fn is_empty(&self) -> bool {
        self.resting_orders.is_empty() && self.pending_orders.is_empty()
    }

    pub fn tick_size(&self) -> TickSize {
        self.tick_size
    }

    pub fn lot_size(&self) -> LotSize {
        self.lot_size
    }

    pub fn best_bid(&self) -> Option<Order> {
        let (key, qty) = self.bids.first_key_value()?;
        Some(
            PendingOrder {
                side: Side::Buy,
                price: Price::new(key.price),
                quantity: qty.0,
            }
            .into_order(OrderSeq::new(key.seq)),
        )
    }

    pub fn best_ask(&self) -> Option<Order> {
        let (key, qty) = self.asks.first_key_value()?;
        Some(
            PendingOrder {
                side: Side::Sell,
                price: Price::new(key.price),
                quantity: qty.0,
            }
            .into_order(OrderSeq::new(key.seq)),
        )
    }

    pub fn match_order(&mut self, mut order: Order) -> Result<MatchResult, MatchOrderError> {
        self.validate_order(order.price(), order.remaining_quantity())?;

        let mut fills = Vec::new();

        match order.side() {
            Side::Buy => self.fill_against_asks(&mut order, &mut fills),
            Side::Sell => self.fill_against_bids(&mut order, &mut fills),
        }

        if order.remaining_quantity().is_zero() {
            self.filled_orders.insert(order.id());
            Ok(MatchResult::Filled { fills })
        } else {
            let resting_order_seq = order.id();
            self.insert_order(order);
            if fills.is_empty() {
                Ok(MatchResult::Resting { resting_order_seq })
            } else {
                Ok(MatchResult::PartiallyFilled {
                    fills,
                    resting_order_seq,
                })
            }
        }
    }

    pub fn validate_order(&self, price: Price, quantity: &Quantity) -> Result<(), MatchOrderError> {
        if price.is_zero() || !price.is_multiple_of(self.tick_size) {
            return Err(MatchOrderError::InvalidTickSize {
                price,
                tick_size: self.tick_size,
            });
        }
        if quantity.is_zero() || !quantity.is_multiple_of(self.lot_size) {
            return Err(MatchOrderError::InvalidLotSize {
                quantity: *quantity,
                lot_size: self.lot_size,
            });
        }
        Ok(())
    }

    pub fn add_pending_order(&mut self, order: Order) {
        assert!(
            self.validate_order(order.price(), order.remaining_quantity())
                .is_ok(),
            "BUG: order is invalid"
        );
        assert_eq!(order.id(), self.next_seq, "BUG: order seq mismatch");
        self.pending_orders.push_back(order);
        self.next_seq.increment();
    }

    pub fn process_pending_orders(&mut self) -> MatchingOutput {
        let mut all_fills = Vec::new();
        let mut resting_order_seqs = BTreeSet::new();
        while let Some(order) = self.pending_orders.pop_front() {
            match self.match_order(order) {
                Ok(result) => {
                    if let Some(resting_order_seq) = result.resting_order_seq() {
                        resting_order_seqs.insert(resting_order_seq);
                    }
                    for fill in result.into_fills() {
                        debug_assert!(!all_fills.contains(&fill), "BUG: duplicate fill {fill:?}");
                        all_fills.push(fill);
                    }
                }
                Err(err) => {
                    panic!(
                        "BUG: failed to match order: {:?}. Order was validated when inserted; \
                        and, although matching happens asynchronously afterwards, \
                        the tick/lot size are not mutable",
                        err
                    );
                }
            }
        }
        let resting_orders = &resting_order_seqs - &self.filled_orders;
        debug_assert!(
            resting_orders.is_disjoint(&self.filled_orders),
            "BUG: resting and filled sets overlap"
        );
        let filled_orders = std::mem::take(&mut self.filled_orders);
        MatchingOutput {
            fills: all_fills,
            resting_orders,
            filled_orders,
        }
    }

    pub fn pending_orders_len(&self) -> usize {
        self.pending_orders.len()
    }

    pub fn bids_len(&self) -> u64 {
        self.bids.len()
    }

    pub fn asks_len(&self) -> u64 {
        self.asks.len()
    }

    pub fn resting_orders_len(&self) -> u64 {
        self.resting_orders.len()
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn insert_order(&mut self, order: Order) {
        let side = order.side();
        let price = order.price();
        let seq = order.id();
        let prev = self
            .resting_orders
            .insert(seq.get(), StorableSidePrice { side, price });
        assert!(prev.is_none());
        let qty = StorableQuantity(*order.remaining_quantity());
        match side {
            Side::Buy => {
                self.bids.insert(
                    BidKey {
                        price: price.get(),
                        seq: seq.get(),
                    },
                    qty,
                );
            }
            Side::Sell => {
                self.asks.insert(
                    AskKey {
                        price: price.get(),
                        seq: seq.get(),
                    },
                    qty,
                );
            }
        }
    }

    fn fill_against_asks(&mut self, order: &mut Order, fills: &mut Vec<Fill>) {
        while !order.remaining_quantity().is_zero() {
            let Some((key, maker_qty)) = self.asks.first_key_value() else {
                break;
            };
            let maker_price = Price::new(key.price);
            if maker_price > order.price() {
                break;
            }
            let maker_seq = OrderSeq::new(key.seq);
            let fill_qty = *std::cmp::min(order.remaining_quantity(), &maker_qty.0);

            order.reduce_quantity(&fill_qty);

            fills.push(Fill {
                taker_order_seq: order.id(),
                taker_side: order.side(),
                taker_price: order.price(),
                maker_order_seq: maker_seq,
                maker_price,
                quantity: fill_qty,
            });

            let new_remaining = maker_qty.0.checked_sub(&fill_qty).unwrap();
            if new_remaining.is_zero() {
                self.asks.remove(&key);
                self.resting_orders.remove(&key.seq);
                self.filled_orders.insert(maker_seq);
            } else {
                self.asks.insert(key, StorableQuantity(new_remaining));
            }
        }
    }

    fn fill_against_bids(&mut self, order: &mut Order, fills: &mut Vec<Fill>) {
        while !order.remaining_quantity().is_zero() {
            let Some((key, maker_qty)) = self.bids.first_key_value() else {
                break;
            };
            let maker_price = Price::new(key.price);
            if maker_price < order.price() {
                break;
            }
            let maker_seq = OrderSeq::new(key.seq);
            let fill_qty = *std::cmp::min(order.remaining_quantity(), &maker_qty.0);

            order.reduce_quantity(&fill_qty);

            fills.push(Fill {
                taker_order_seq: order.id(),
                taker_side: order.side(),
                taker_price: order.price(),
                maker_order_seq: maker_seq,
                maker_price,
                quantity: fill_qty,
            });

            let new_remaining = maker_qty.0.checked_sub(&fill_qty).unwrap();
            if new_remaining.is_zero() {
                self.bids.remove(&key);
                self.resting_orders.remove(&key.seq);
                self.filled_orders.insert(maker_seq);
            } else {
                self.bids.insert(key, StorableQuantity(new_remaining));
            }
        }
    }
}
