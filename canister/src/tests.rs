/// Helper to add an order via State directly (bypasses IC timer in lib::add_limit_order).
fn add_order(
    request: dex_types::LimitOrderRequest,
) -> Result<dex_types::OrderId, dex_types::AddLimitOrderError> {
    use crate::{order, state};
    let pair = order::TradingPair::from(request.pair);
    let pending = order::PendingOrder {
        side: order::Side::from(request.side),
        price: order::Price::from(request.price),
        quantity: order::Quantity::from(request.quantity),
    };
    state::with_state_mut(|s| s.add_limit_order(pair, pending))
        .map(u64::from)
        .map_err(dex_types::AddLimitOrderError::from)
}

mod add_limit_order {
    use super::add_order;
    use crate::test_fixtures::{init_state_with_order_book, limit_order_request};
    use std::collections::BTreeSet;

    #[test]
    fn should_add_limit_orders_with_distinct_order_ids() {
        init_state_with_order_book();
        let mut order_ids = BTreeSet::new();
        let num_orders = 100;

        for _ in 0..num_orders {
            let order_id = add_order(limit_order_request()).unwrap();
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
        let result = add_order(request);
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
        let result = add_order(request);
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
        let result = add_order(request);
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
    use super::add_order;
    use crate::get_order_status;
    use crate::test_fixtures::{init_state_with_order_book, limit_order_request};
    use dex_types::OrderStatus;

    #[test]
    fn should_return_pending_for_existing_order() {
        init_state_with_order_book();
        let order_id = add_order(limit_order_request()).unwrap();
        let status = get_order_status(order_id);
        assert_eq!(status, OrderStatus::Pending);
    }

    #[test]
    fn should_return_not_found_for_nonexistent_order() {
        init_state_with_order_book();
        let status = get_order_status(u64::MAX);
        assert_eq!(status, OrderStatus::NotFound);
    }
}
