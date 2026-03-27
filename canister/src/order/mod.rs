#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OrderId(u64);

impl OrderId {
    pub const ZERO: Self = Self(0);

    pub fn increment(&mut self) {
        self.0 = self.0.checked_add(1).expect("OrderId overflow");
    }
}

impl From<u64> for OrderId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<OrderId> for u64 {
    fn from(id: OrderId) -> Self {
        id.0
    }
}

#[derive(Debug)]
pub struct PendingOrder {
    // TODO DEFI-2723: add fields: price, quantity, side, etc.
}

impl PendingOrder {
    pub fn into_order(self, id: OrderId) -> Order {
        Order { id }
    }
}

#[derive(Debug)]
pub struct Order {
    pub id: OrderId,
    // TODO DEFI-2723: add fields: price, quantity, side, etc.
}
