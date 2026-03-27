#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OrderId(u64);

impl OrderId {
    pub const ZERO: Self = Self(0);

    pub fn increment(&mut self) -> Self {
        let current = *self;
        self.0 += 1;
        current
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

#[derive(Debug)]
pub struct Order {
    pub id: OrderId,
    // TODO DEFI-2723: add fields: price, quantity, side, etc.
}

impl Order {
    pub fn from_pending(_pending: PendingOrder, id: OrderId) -> Self {
        Self { id }
    }
}
