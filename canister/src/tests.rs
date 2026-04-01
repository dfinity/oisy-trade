mod add_limit_order {
    use crate::add_limit_order;
    use crate::state::init_state;
    use crate::test_fixtures::limit_order_request;
    use std::collections::BTreeSet;

    #[test]
    fn should_add_limit_orders_with_distinct_order_ids() {
        init_state();
        let mut order_ids = BTreeSet::new();
        let num_orders = 100;

        for _ in 0..num_orders {
            let response = add_limit_order(limit_order_request());
            assert!(order_ids.insert(response.order_id));
        }
    }
}

mod get_order_status {
    use crate::state::init_state;
    use crate::test_fixtures::limit_order_request;
    use crate::{add_limit_order, get_order_status};
    use dex_types::OrderStatus;

    #[test]
    fn should_return_pending_for_existing_order() {
        init_state();
        let response = add_limit_order(limit_order_request());
        let status = get_order_status(response.order_id);
        assert_eq!(status, OrderStatus::Pending);
    }

    #[test]
    fn should_return_not_found_for_nonexistent_order() {
        init_state();
        let status = get_order_status(u64::MAX);
        assert_eq!(status, OrderStatus::NotFound);
    }
}

mod get_trading_pairs {
    use crate::get_trading_pairs;
    use crate::order::{OrderBook, Price, Quantity, TokenId, TradingPair};
    use crate::state;
    use crate::state::init_state;
    use candid::Principal;

    #[test]
    fn should_return_empty_when_no_trading_pairs() {
        init_state();
        let pairs = get_trading_pairs();
        assert!(pairs.is_empty());
    }

    #[test]
    fn should_return_listed_trading_pairs() {
        init_state();
        let base = TokenId::new(Principal::from_slice(&[0x01]));
        let quote = TokenId::new(Principal::from_slice(&[0x02]));
        let tick_size = Price::new(10);
        let lot_size = Quantity::new(1_000_000);

        state::with_state_mut(|s| {
            s.add_trading_pair(
                TradingPair { base, quote },
                OrderBook::new(tick_size, lot_size),
            );
        });

        let pairs = get_trading_pairs();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].base_asset, *base.as_principal());
        assert_eq!(pairs[0].quote_asset, *quote.as_principal());
        assert_eq!(pairs[0].tick_size, 10);
        assert_eq!(pairs[0].lot_size, 1_000_000);
    }
}
