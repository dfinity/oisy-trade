mod add_limit_order {
    use crate::add_limit_order;
    use crate::state::init_state;
    use dex_types::LimitOrderRequest;
    use std::collections::BTreeSet;

    #[test]
    fn should_add_limit_orders_with_distinct_order_ids() {
        init_state();
        let mut order_ids = BTreeSet::new();
        let num_orders = 100;

        for _ in 0..num_orders {
            let response = add_limit_order(LimitOrderRequest {});
            assert!(order_ids.insert(response.order_id));
        }
    }
}

mod get_order_status {
    use crate::state::init_state;
    use crate::{add_limit_order, get_order_status};
    use dex_types::{LimitOrderRequest, OrderStatus};

    #[test]
    fn should_return_pending_for_existing_order() {
        init_state();
        let response = add_limit_order(LimitOrderRequest {});

        let status = get_order_status(response.order_id);

        assert_eq!(status, OrderStatus::Pending);
    }

    #[test]
    fn should_return_not_found_for_nonexistent_order() {
        init_state();

        let status = get_order_status(999);

        assert_eq!(status, OrderStatus::NotFound);
    }
}
