use super::{OrderId, OrderStatus, Price, Quantity, Side};
use candid::Principal;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;

/// Record of an order from submission through terminal state.
///
/// Persisted in a [`ic_stable_structures::StableBTreeMap`] keyed by [`OrderId`],
/// so the CBOR layout is an upgrade-durable schema: removing or renumbering a
/// field breaks decoding of records written by prior canister versions. New
/// fields must be added with `#[cbor(n(N), default)]` or an `Option<T>` type.
/// The trading pair is deliberately not stored — it is derivable from the
/// `OrderBookId` embedded in the [`OrderId`] via the trading-pair registry.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct OrderRecord {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub owner: Principal,
    #[n(1)]
    pub side: Side,
    #[n(2)]
    pub price: Price,
    #[n(3)]
    pub quantity: Quantity,
    #[n(4)]
    pub status: OrderStatus,
}

impl From<OrderRecord> for dex_types::OrderRecord {
    fn from(record: OrderRecord) -> Self {
        dex_types::OrderRecord {
            owner: record.owner,
            side: record.side.into(),
            price: record.price.into(),
            quantity: record.quantity.into(),
            status: record.status.into(),
        }
    }
}

impl Storable for OrderRecord {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("order record encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("order record encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode order record bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

pub struct OrderHistory<M: Memory> {
    orders: StableBTreeMap<OrderId, OrderRecord, M>,
}

impl<M: Memory> std::fmt::Debug for OrderHistory<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrderHistory")
            .field("len", &self.orders.len())
            .finish()
    }
}

impl<M: Memory> OrderHistory<M> {
    pub fn new(memory: M) -> Self {
        Self {
            orders: StableBTreeMap::init(memory),
        }
    }

    /// Insert a new order record. Panics if the order ID already exists.
    pub fn insert_once(&mut self, id: OrderId, record: OrderRecord) {
        bench_scopes!("order_history", "order_history::insert_once");
        assert_eq!(
            self.orders.insert(id, record),
            None,
            "BUG: duplicate order ID {id}"
        );
    }

    /// Returns a copy of the record for the given order, or `None` if absent.
    pub fn get(&self, id: &OrderId) -> Option<OrderRecord> {
        bench_scopes!("order_history", "order_history::get");
        self.orders.get(id)
    }

    /// Updates the status of an existing order. Panics if the order is unknown.
    pub fn set_status(&mut self, id: &OrderId, status: OrderStatus) {
        bench_scopes!("order_history", "order_history::set_status");
        let mut record = self
            .orders
            .get(id)
            .unwrap_or_else(|| panic!("BUG: order {id} missing from order_history"));
        record.status = status;
        self.orders.insert(*id, record);
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = (OrderId, OrderRecord)> + '_ {
        self.orders
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }
}

#[cfg(test)]
impl Clone for OrderHistory<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(ic_stable_structures::VectorMemory::default());
        for (id, record) in self.iter() {
            fresh.insert_once(id, record);
        }
        fresh
    }
}

#[cfg(test)]
impl PartialEq for OrderHistory<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
impl Eq for OrderHistory<ic_stable_structures::VectorMemory> {}
