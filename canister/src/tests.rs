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
        let status = get_order_status(u64::MAX);
        assert_eq!(status, OrderStatus::NotFound);
    }
}

mod get_trading_pairs {
    use crate::get_trading_pairs;
    use crate::order::{TokenId, TradingPair};
    use crate::state;
    use crate::state::init_state;
    use crate::test_fixtures::order_book;
    use candid::Principal;
    use dex_types::TradingPairInfo;

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
        let order_book = order_book();
        let tick_size = order_book.tick_size().get();
        let lot_size = order_book.lot_size().get();
        state::with_state_mut(|s| {
            s.add_trading_pair(TradingPair { base, quote }, order_book);
        });

        let pairs = get_trading_pairs();

        assert_eq!(
            pairs,
            vec![TradingPairInfo {
                base_asset: dex_types::TokenId::from(base),
                quote_asset: dex_types::TokenId::from(quote),
                tick_size,
                lot_size,
            }]
        );
    }
}
