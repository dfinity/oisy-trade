use dex_types::{OrderId, OrderStatus};
use std::cell::RefCell;
use std::collections::BTreeSet;

thread_local! {
    static NEXT_ORDER_ID: RefCell<OrderId> = const { RefCell::new(0) };
    static ORDERS: RefCell<BTreeSet<OrderId>> = const { RefCell::new(BTreeSet::new()) };
}

pub fn add_order() -> OrderId {
    let order_id = NEXT_ORDER_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });
    ORDERS.with(|orders| {
        orders.borrow_mut().insert(order_id);
    });
    order_id
}

pub fn get_order_status(order_id: OrderId) -> OrderStatus {
    ORDERS.with(|orders| {
        if orders.borrow().contains(&order_id) {
            OrderStatus::Pending
        } else {
            OrderStatus::NotFound
        }
    })
}
