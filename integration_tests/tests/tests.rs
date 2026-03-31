use dex_int_tests::Setup;

mod add_limit_order {
    use candid::Principal;
    use dex_int_tests::{Setup, test_trading_pair};
    use dex_types::{LimitOrderRequest, OrderStatus, Side, TradingPair};
    use std::time::Duration;

    #[tokio::test]
    async fn should_add_limit_order_and_query_status() {
        let setup = Setup::new().await;
        let client = setup.client();

        let order_id = client
            .add_limit_order(LimitOrderRequest {
                pair: TradingPair {
                    base: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
                    quote: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
                },
                side: Side::Buy,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();

        let status = client.get_order_status(order_id).await;
        assert_eq!(status, OrderStatus::Pending);

        let not_found = client.get_order_status(u64::MAX).await;
        assert_eq!(not_found, OrderStatus::NotFound);

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_reject_invalid_orders() {
        let setup = Setup::new().await;
        let client = setup.client();
        let pair = test_trading_pair();

        let cases = vec![
            (
                "unknown trading pair",
                LimitOrderRequest {
                    pair: TradingPair {
                        base: Principal::management_canister(),
                        quote: Principal::management_canister(),
                    },
                    side: Side::Buy,
                    price: 100,
                    quantity: 1_000_000,
                },
                dex_types::AddLimitOrderError::UnknownTradingPair,
            ),
            (
                "price not a multiple of tick size",
                LimitOrderRequest {
                    pair,
                    side: Side::Buy,
                    price: 7,
                    quantity: 1_000_000,
                },
                dex_types::AddLimitOrderError::InvalidPrice {
                    price: 7,
                    tick_size: 10,
                },
            ),
            (
                "zero price",
                LimitOrderRequest {
                    pair,
                    side: Side::Buy,
                    price: 0,
                    quantity: 1_000_000,
                },
                dex_types::AddLimitOrderError::InvalidPrice {
                    price: 0,
                    tick_size: 10,
                },
            ),
            (
                "quantity not a multiple of lot size",
                LimitOrderRequest {
                    pair,
                    side: Side::Sell,
                    price: 100,
                    quantity: 500_000,
                },
                dex_types::AddLimitOrderError::InvalidQuantity {
                    quantity: 500_000,
                    lot_size: 1_000_000,
                },
            ),
            (
                "zero quantity",
                LimitOrderRequest {
                    pair,
                    side: Side::Sell,
                    price: 100,
                    quantity: 0,
                },
                dex_types::AddLimitOrderError::InvalidQuantity {
                    quantity: 0,
                    lot_size: 1_000_000,
                },
            ),
        ];

        for (name, request, expected_error) in cases {
            let result = client.add_limit_order(request).await;
            assert_eq!(result, Err(expected_error), "case: {name}");
        }

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_match_orders_after_timer_fires() {
        let setup = Setup::new().await;
        let client = setup.client();
        let pair = test_trading_pair();

        // Place a sell, then a buy at the same price — both are pending
        let sell_id = client
            .add_limit_order(LimitOrderRequest {
                pair,
                side: Side::Sell,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();
        let buy_id = client
            .add_limit_order(LimitOrderRequest {
                pair,
                side: Side::Buy,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();

        assert_eq!(client.get_order_status(sell_id).await, OrderStatus::Pending);
        assert_eq!(client.get_order_status(buy_id).await, OrderStatus::Pending);

        // Advance time past the matching interval and tick to fire the timer
        setup.env().advance_time(Duration::from_secs(61)).await;
        setup.env().tick().await;
        setup.env().tick().await;

        // After matching, the sell rests in the book (processed first), then the
        // buy matches against it — both are fully filled and no longer tracked.
        // TODO DEFI-2740: verify user's balances
        assert_eq!(
            client.get_order_status(sell_id).await,
            OrderStatus::NotFound
        );
        assert_eq!(client.get_order_status(buy_id).await, OrderStatus::NotFound);

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_transition_unmatched_order_to_open() {
        let setup = Setup::new().await;
        let client = setup.client();

        let order_id = client
            .add_limit_order(LimitOrderRequest {
                pair: test_trading_pair(),
                side: Side::Buy,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();

        assert_eq!(
            client.get_order_status(order_id).await,
            OrderStatus::Pending
        );

        // Advance time past the matching interval and tick
        setup.env().advance_time(Duration::from_secs(61)).await;
        setup.env().tick().await;
        setup.env().tick().await;

        // No counterparty — order rests in the book as Open
        // TODO DEFI-2740: verify user's balances
        assert_eq!(client.get_order_status(order_id).await, OrderStatus::Open);

        setup.drop().await;
    }
}

#[tokio::test]
async fn should_have_different_ledger_configs() {
    let setup = Setup::new().await;
    let base = setup.base_token_ledger();
    let quote = setup.quote_token_ledger();

    let base_decimals = base.icrc1_decimals().await;
    let quote_decimals = quote.icrc1_decimals().await;
    assert_ne!(base_decimals, quote_decimals);

    let base_fee = base.icrc1_fee().await;
    let quote_fee = quote.icrc1_fee().await;
    assert_ne!(base_fee, quote_fee);

    setup.drop().await;
}
