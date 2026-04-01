mod add_limit_order {
    use crate::add_limit_order;
    use crate::test_fixtures::{init_state_with_order_book, limit_order_request};
    use std::collections::BTreeSet;

    #[test]
    fn should_add_limit_orders_with_distinct_order_ids() {
        init_state_with_order_book();
        let mut order_ids = BTreeSet::new();
        let num_orders = 100;

        for _ in 0..num_orders {
            let order_id = add_limit_order(limit_order_request()).unwrap();
            assert!(order_ids.insert(order_id));
        }
    }

    #[test]
    fn should_reject_order_for_unknown_trading_pair() {
        init_state_with_order_book();
        let mut request = limit_order_request();
        request.pair = dex_types::TradingPair {
            base: candid::Principal::management_canister(),
            quote: candid::Principal::management_canister(),
        };
        let result = add_limit_order(request);
        assert_eq!(
            result,
            Err(dex_types::AddLimitOrderError::UnknownTradingPair)
        );
    }

    #[test]
    fn should_reject_order_with_invalid_price() {
        init_state_with_order_book();
        let mut request = limit_order_request();
        request.price = 7; // not a multiple of tick size (10)
        let result = add_limit_order(request);
        assert_eq!(
            result,
            Err(dex_types::AddLimitOrderError::InvalidPrice {
                price: 7,
                tick_size: 10,
            })
        );
    }

    #[test]
    fn should_reject_order_with_invalid_quantity() {
        init_state_with_order_book();
        let mut request = limit_order_request();
        request.quantity = 500_000; // not a multiple of lot size (1_000_000)
        let result = add_limit_order(request);
        assert_eq!(
            result,
            Err(dex_types::AddLimitOrderError::InvalidQuantity {
                quantity: 500_000,
                lot_size: 1_000_000,
            })
        );
    }
}

mod get_order_status {
    use crate::test_fixtures::{init_state_with_order_book, limit_order_request};
    use crate::{add_limit_order, get_order_status};
    use dex_types::OrderStatus;

    #[test]
    fn should_return_pending_for_existing_order() {
        init_state_with_order_book();
        let order_id = add_limit_order(limit_order_request()).unwrap();
        let status = get_order_status(order_id);
        assert_eq!(status, OrderStatus::Pending);
    }

    #[test]
    fn should_return_not_found_for_nonexistent_order() {
        init_state_with_order_book();
        // Valid hex format but refers to a non-existent book/seq
        let status = get_order_status("ffffffffffffffffffffffffffffffff".to_string());
        assert_eq!(status, OrderStatus::NotFound);
    }

    #[test]
    #[should_panic(expected = "ERROR: invalid order id")]
    fn should_trap_on_syntactically_invalid_order_id() {
        init_state_with_order_book();
        get_order_status("not-a-valid-order-id".to_string());
    }
}
