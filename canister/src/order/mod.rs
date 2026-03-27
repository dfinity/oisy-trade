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
pub struct Order {
    pub id: OrderId,
}
