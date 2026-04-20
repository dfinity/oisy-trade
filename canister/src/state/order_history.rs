use crate::order::{OrderId, OrderRecord, OrderStatus};
use ic_stable_structures::{Memory, StableBTreeMap};

pub struct OrderHistory<M: Memory> {
    map: StableBTreeMap<OrderId, OrderRecord, M>,
}

impl<M: Memory> OrderHistory<M> {
    pub fn new(memory: M) -> Self {
        Self {
            map: StableBTreeMap::init(memory),
        }
    }

    /// Insert a new order record. Panics if the order ID already exists.
    pub fn insert_once(&mut self, id: OrderId, record: OrderRecord) {
        assert!(!self.map.contains_key(&id), "BUG: duplicate order ID {id}");
        self.map.insert(id, record);
    }

    /// Returns a copy of the record for the given order, or `None` if absent.
    pub fn get(&self, id: &OrderId) -> Option<OrderRecord> {
        self.map.get(id)
    }

    /// Returns the status of the given order, or `None` if absent.
    pub fn get_status(&self, id: &OrderId) -> Option<OrderStatus> {
        self.map.get(id).map(|r| r.status)
    }

    /// Updates the status of an existing order. Panics if the order is unknown.
    pub fn set_status(&mut self, id: &OrderId, status: OrderStatus) {
        let mut record = self
            .map
            .get(id)
            .unwrap_or_else(|| panic!("BUG: order {id} missing from order_history"));
        record.status = status;
        self.map.insert(*id, record);
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = (OrderId, OrderRecord)> + '_ {
        self.map.iter().map(|entry| (*entry.key(), entry.value()))
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn clear_for_test(&mut self) {
        let ids: Vec<OrderId> = self.map.iter().map(|entry| *entry.key()).collect();
        for id in ids {
            self.map.remove(&id);
        }
    }
}
