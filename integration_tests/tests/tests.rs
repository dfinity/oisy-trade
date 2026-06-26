use assert_matches::assert_matches;
use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use oisy_trade_client::{OisyTradeClient, Runtime};
use oisy_trade_int_tests::icrc_ledger::{BASE_LEDGER_FEE, LedgerConfig, QUOTE_LEDGER_FEE};
use oisy_trade_int_tests::{LOT_SIZE, PRICE_SCALE, Setup, TICK_SIZE, fill_one_cross_with_fees};
use oisy_trade_types::{
    AddTradingPairError, AddTradingPairRequest, Balance, DepositError, DepositRequest,
    DepositRequestError, DepositTemporaryError, ErrorKind, LimitOrderRequest, OrderStatus, Side,
    Token, TokenId, TokenMetadata, TradingPairInfo, TradingStatus, WithdrawRequest,
    WithdrawRequestError, WithdrawTemporaryError,
};
use oisy_trade_types_internal::log::Priority;

fn expected_balance(free: u64) -> Balance {
    Balance {
        free: Nat::from(free),
        reserved: Nat::from(0u64),
    }
}

#[allow(clippy::too_many_arguments)]
async fn assert_balances<R: Runtime>(
    client1: &OisyTradeClient<R>,
    client2: &OisyTradeClient<R>,
    cksol: &TokenId,
    ckbtc: &TokenId,
    expected_user1_cksol: u64,
    expected_user1_ckbtc: u64,
    expected_user2_cksol: u64,
    expected_user2_ckbtc: u64,
) {
    assert_eq!(
        client1.get_balance(cksol.clone()).await.unwrap(),
        expected_balance(expected_user1_cksol),
        "user1 ckSOL balance mismatch"
    );
    assert_eq!(
        client1.get_balance(ckbtc.clone()).await.unwrap(),
        expected_balance(expected_user1_ckbtc),
        "user1 ckBTC balance mismatch"
    );
    assert_eq!(
        client2.get_balance(cksol.clone()).await.unwrap(),
        expected_balance(expected_user2_cksol),
        "user2 ckSOL balance mismatch"
    );
    assert_eq!(
        client2.get_balance(ckbtc.clone()).await.unwrap(),
        expected_balance(expected_user2_ckbtc),
        "user2 ckBTC balance mismatch"
    );
}

mod add_limit_order {
    use candid::{Nat, Principal};
    use oisy_trade_int_tests::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
    use oisy_trade_int_tests::{LOT_SIZE, PRICE_SCALE, Setup};
    use oisy_trade_types::{
        AddLimitOrderRequestError, AddTradingPairRequest, Balance, ErrorKind, GetMyOrdersArgs,
        GetMyOrdersRequestError, LimitOrderRequest, OrderId, OrderStatus, Side,
    };

    /// A `ByPage` filter, matching the previous flat `after`/`length` args.
    fn by_page(after: Option<OrderId>, length: u32) -> GetMyOrdersArgs {
        GetMyOrdersArgs::by_page(after, length)
    }

    /// Mints, approves, and deposits `amount` base units for `who`.
    async fn fund_base(setup: &Setup, who: Principal, amount: u64) {
        setup
            .deposit_flow(who, setup.base_token_id())
            .mint(amount + 2 * BASE_LEDGER_FEE)
            .approve(amount + BASE_LEDGER_FEE)
            .deposit(amount)
            .execute()
            .await;
    }

    /// Mints, approves, and deposits `amount` quote units for `who`.
    async fn fund_quote(setup: &Setup, who: Principal, amount: u64) {
        setup
            .deposit_flow(who, setup.quote_token_id())
            .mint(amount + 2 * QUOTE_LEDGER_FEE)
            .approve(amount + QUOTE_LEDGER_FEE)
            .deposit(amount)
            .execute()
            .await;
    }

    #[tokio::test]
    async fn should_add_limit_buy_order_and_query_status() {
        let setup = Setup::new().await.with_trading_pair().await;
        let client = setup.oisy_trade_client();
        let token_id = setup.quote_token_id();
        let fee = QUOTE_LEDGER_FEE;
        // Buy 1_000_000 ckSOL base units at 10_000 ckBTC per whole ckSOL
        // (10_000 * PRICE_SCALE). Reserve = price * quantity / 10^9 = 1_000_000_000.
        let order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Buy,
            price: Nat::from(10_000 * PRICE_SCALE),
            quantity: 1_000_000u64.into(),
            time_in_force: None,
        };

        let required = 1_000_000_000u64;
        assert_eq!(
            client
                .add_limit_order(order.clone())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(AddLimitOrderRequestError::InsufficientBalance {
                token: token_id.clone(),
                available: 0u64.into(),
                required: required.into(),
            }))
        );

        setup
            .deposit_flow(setup.user(), token_id.clone())
            .mint(required + 2 * fee)
            .approve(required + fee)
            .deposit(required)
            .execute()
            .await;

        let order_id = client.add_limit_order(order).await.unwrap();
        assert_eq!(
            client.get_balance(token_id).await.unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: required.into(),
            }
        );
        // The matching timer fires eagerly after placement; with no counterparty
        // the order rests in the book as Open.
        assert_eq!(
            client.get_my_order(order_id).await.unwrap().order.status,
            OrderStatus::Open
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_return_my_orders_newest_first_paginated() {
        let setup = Setup::new().await.with_trading_pair().await;
        let alice = setup.oisy_trade_client();
        let bob_principal = Principal::from_slice(&[0xBB]);
        let bob = setup.oisy_trade_client_with_caller(bob_principal);
        let pair = setup.trading_pair();

        // Fund both callers for three resting buys of 1M @ 1000 (1B each).
        let per_order = 1_000_000_000u64;
        let total = 3 * per_order;
        for who in [setup.user(), bob_principal] {
            setup
                .deposit_flow(who, setup.quote_token_id())
                .mint(total + 2 * QUOTE_LEDGER_FEE)
                .approve(total + QUOTE_LEDGER_FEE)
                .deposit(total)
                .execute()
                .await;
        }

        // Interleave alice's and bob's placements so a correct per-user index
        // can't rely on a caller's orders being inserted contiguously.
        let buy = LimitOrderRequest {
            pair,
            side: Side::Buy,
            price: 1000u64.into(),
            quantity: 1_000_000u64.into(),
            time_in_force: None,
        };
        let before_placement = setup.time_nanos().await;
        let mut alice_ids = vec![];
        let mut bob_ids = vec![];
        for _ in 0..3 {
            alice_ids.push(alice.add_limit_order(buy.clone()).await.unwrap());
            bob_ids.push(bob.add_limit_order(buy.clone()).await.unwrap());
        }
        let after_placement = setup.time_nanos().await;

        // Alice sees only her own orders, newest first — bob's interleaved
        // orders don't leak in.
        let orders = alice.get_my_orders(by_page(None, 10)).await.unwrap();
        assert_eq!(
            orders.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
            vec![
                alice_ids[2].clone(),
                alice_ids[1].clone(),
                alice_ids[0].clone()
            ]
        );
        for o in &orders {
            assert_eq!(o.pair, pair);
            assert_eq!(o.order.owner, setup.user());
            assert_eq!(o.order.side, Side::Buy);
            assert_eq!(o.order.price, candid::Nat::from(1000u64));
            assert!(
                before_placement <= o.order.created_at && o.order.created_at <= after_placement,
                "submission timestamp {} outside placement window [{}, {}]",
                o.order.created_at,
                before_placement,
                after_placement
            );
        }

        // Bob likewise sees only his own.
        let bob_orders = bob.get_my_orders(by_page(None, 10)).await.unwrap();
        assert_eq!(
            bob_orders.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
            vec![bob_ids[2].clone(), bob_ids[1].clone(), bob_ids[0].clone()]
        );
        assert!(bob_orders.iter().all(|o| o.order.owner == bob_principal));

        // Cursor pagination: resume after the newest, take one → the next order.
        let page = alice
            .get_my_orders(by_page(Some(alice_ids[2].clone()), 1))
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, alice_ids[1]);

        // A valid cursor at the oldest order has no older orders: end of
        // history is Ok([]), not an error.
        assert_eq!(
            alice
                .get_my_orders(by_page(Some(alice_ids[0].clone()), 10))
                .await
                .unwrap(),
            Vec::new()
        );

        // Point lookup by id: each caller resolves only their own orders.
        let alice_by_id = alice.get_my_order(alice_ids[0].clone()).await.unwrap();
        assert_eq!(alice_by_id.id, alice_ids[0]);
        // An order owned by another principal is not found for a foreign caller.
        assert_eq!(
            alice
                .get_my_order(bob_ids[0].clone())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(GetMyOrdersRequestError::OrderNotFound))
        );
        // An unknown (but well-formed) id is likewise not found.
        assert_eq!(
            alice
                .get_my_order("ffffffffffffffffffffffffffffffff".to_string())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(GetMyOrdersRequestError::OrderNotFound))
        );

        // A caller that placed nothing sees none.
        let stranger = setup.oisy_trade_client_with_caller(Principal::from_slice(&[0xAB]));
        assert!(
            stranger
                .get_my_orders(by_page(None, 10))
                .await
                .unwrap()
                .is_empty()
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_add_limit_sell_order_and_query_status() {
        let setup = Setup::new().await.with_trading_pair().await;
        let client = setup.oisy_trade_client();
        let token_id = setup.base_token_id();
        let fee = BASE_LEDGER_FEE;
        // Sell 1_000_000 ckSOL base units at 10_000 ckBTC per whole ckSOL
        // (10_000 * PRICE_SCALE); a sell reserves the base → need 1_000_000 base units.
        let order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Sell,
            price: Nat::from(10_000 * PRICE_SCALE),
            quantity: 1_000_000u64.into(),
            time_in_force: None,
        };

        let required = 1_000_000u64;
        assert_eq!(
            client
                .add_limit_order(order.clone())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(AddLimitOrderRequestError::InsufficientBalance {
                token: token_id.clone(),
                available: 0u64.into(),
                required: required.into(),
            }))
        );

        setup
            .deposit_flow(setup.user(), token_id.clone())
            .mint(required + 2 * fee)
            .approve(required + fee)
            .deposit(required)
            .execute()
            .await;

        let order_id = client.add_limit_order(order).await.unwrap();
        assert_eq!(
            client.get_balance(token_id).await.unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: required.into(),
            }
        );
        // The matching timer fires eagerly after placement; with no counterparty
        // the order rests in the book as Open.
        assert_eq!(
            client.get_my_order(order_id).await.unwrap().order.status,
            OrderStatus::Open
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_report_unknown_order_as_not_found_and_reject_malformed_id() {
        let setup = Setup::new().await;
        let client = setup.oisy_trade_client();

        // A well-formed but unknown id is not found.
        let not_found = client
            .get_my_order("ffffffffffffffffffffffffffffffff".to_string())
            .await
            .unwrap_err();
        assert_eq!(
            not_found.kind,
            ErrorKind::RequestError(Some(GetMyOrdersRequestError::OrderNotFound))
        );
        assert!(not_found.message.is_some_and(|m| !m.is_empty()));

        // A well-formed but unknown cursor is likewise not found, rather than
        // silently yielding an empty page.
        let bogus_cursor = client
            .get_my_orders(by_page(
                Some("ffffffffffffffffffffffffffffffff".to_string()),
                10,
            ))
            .await
            .unwrap_err();
        assert_eq!(
            bogus_cursor.kind,
            ErrorKind::RequestError(Some(GetMyOrdersRequestError::OrderNotFound))
        );
        assert!(bogus_cursor.message.is_some_and(|m| !m.is_empty()));

        // A malformed id is rejected cleanly (no trap) with InvalidOrderId.
        assert_eq!(
            client
                .get_my_orders(by_page(Some("not-a-valid-id".to_string()), 10))
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(GetMyOrdersRequestError::InvalidOrderId))
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_match_crossing_orders() {
        let setup = Setup::new().await.with_trading_pair().await;
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let seller = Principal::from_slice(&[0x02]);
        let seller_client = setup.oisy_trade_client_with_caller(seller);

        // Buy 1_000_000 ckSOL base units at 10_000 ckBTC per whole ckSOL
        // (10_000 * PRICE_SCALE). Reserve = price * quantity / 10^9 = 1_000_000_000.
        let buy_order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Buy,
            price: Nat::from(10_000 * PRICE_SCALE),
            quantity: 1_000_000u64.into(),
            time_in_force: None,
        };
        let required_quote_amount = 1_000_000_000u64;
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(required_quote_amount + 2 * QUOTE_LEDGER_FEE)
            .approve(required_quote_amount + QUOTE_LEDGER_FEE)
            .deposit(required_quote_amount)
            .execute()
            .await;
        let buy_order_id = buyer_client
            .add_limit_order(buy_order.clone())
            .await
            .unwrap();

        // Sell 1_000_000 ckSOL base units at 10_000 ckBTC per whole ckSOL
        // (10_000 * PRICE_SCALE); a sell reserves the base → need 1_000_000 base units.
        let sell_order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Sell,
            price: Nat::from(10_000 * PRICE_SCALE),
            quantity: 1_000_000u64.into(),
            time_in_force: None,
        };
        let required_base_amount = 1_000_000u64;
        setup
            .deposit_flow(seller, setup.base_token_id())
            .mint(required_base_amount + 2 * BASE_LEDGER_FEE)
            .approve(required_base_amount + BASE_LEDGER_FEE)
            .deposit(required_base_amount)
            .execute()
            .await;
        let sell_order_id = seller_client.add_limit_order(sell_order).await.unwrap();

        setup.env().tick().await;

        // Both orders are fully filled. Lookup is owner-scoped, so each side is
        // queried with its own client.
        assert_eq!(
            buyer_client
                .get_my_order(buy_order_id)
                .await
                .unwrap()
                .order
                .status,
            OrderStatus::Filled
        );
        assert_eq!(
            seller_client
                .get_my_order(sell_order_id)
                .await
                .unwrap()
                .order
                .status,
            OrderStatus::Filled
        );

        // Buyer: received 1M base tokens, spent 1000M quote tokens
        assert_eq!(
            buyer_client
                .get_balance(setup.base_token_id())
                .await
                .unwrap(),
            Balance {
                free: required_base_amount.into(),
                reserved: 0u64.into()
            },
        );
        assert_eq!(
            buyer_client
                .get_balance(setup.quote_token_id())
                .await
                .unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: 0u64.into()
            },
        );

        // Seller: spent 1M base tokens, received 1000M quote tokens
        assert_eq!(
            seller_client
                .get_balance(setup.base_token_id())
                .await
                .unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: 0u64.into()
            },
        );
        assert_eq!(
            seller_client
                .get_balance(setup.quote_token_id())
                .await
                .unwrap(),
            Balance {
                free: required_quote_amount.into(),
                reserved: 0u64.into()
            },
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_expose_per_order_fills_via_get_my_trades() {
        use oisy_trade_types::{GetMyTradesRequestError, PairToken, TradesByOrder, TradesFilter};

        const MAKER_FEE_BPS: u16 = 10;
        const TAKER_FEE_BPS: u16 = 23;
        let setup = Setup::new().await;
        setup
            .oisy_trade_client_with_caller(setup.controller())
            .add_trading_pair(AddTradingPairRequest {
                maker_fee_bps: MAKER_FEE_BPS,
                taker_fee_bps: TAKER_FEE_BPS,
                ..setup.add_trading_pair_request()
            })
            .await
            .unwrap();
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let seller = Principal::from_slice(&[0x02]);
        let seller_client = setup.oisy_trade_client_with_caller(seller);

        // A resting sell at 9_000, hit by a price-improving buy at 10_000: the
        // fill executes at the maker's 9_000, not the taker's 10_000.
        let maker_price = 9_000 * PRICE_SCALE;
        let taker_price = 10_000 * PRICE_SCALE;
        let quantity = 1_000_000u64;
        let notional = maker_price as u128 * quantity as u128 / 1_000_000_000u128;

        fund_base(&setup, seller, quantity).await;
        let sell_id = seller_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Sell,
                price: Nat::from(maker_price),
                quantity: quantity.into(),
                time_in_force: None,
            })
            .await
            .unwrap();
        setup.env().tick().await;

        let buyer_deposit = taker_price as u128 * quantity as u128 / 1_000_000_000u128;
        fund_quote(&setup, buyer, buyer_deposit as u64).await;
        let buy_id = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Buy,
                price: Nat::from(taker_price),
                quantity: quantity.into(),
                time_in_force: None,
            })
            .await
            .unwrap();
        setup.env().tick().await;

        let by_order = |order_id: OrderId| {
            TradesFilter::ByOrder(TradesByOrder {
                order_id,
                after: None,
                length: 10,
            })
        };

        // Buyer's taker fill: maker price, base-denominated taker fee, counterparty absent.
        let buy_trades = buyer_client
            .get_my_trades(by_order(buy_id.clone()))
            .await
            .unwrap();
        assert_eq!(buy_trades.len(), 1);
        let buy_fill = &buy_trades[0];
        assert_eq!(buy_fill.order_id, buy_id);
        assert_eq!(buy_fill.side, Side::Buy);
        assert!(!buy_fill.is_maker);
        assert_eq!(buy_fill.price, Nat::from(maker_price));
        assert_eq!(buy_fill.notional, Nat::from(notional));
        assert_eq!(buy_fill.fee_token, PairToken::Base);
        assert_eq!(
            buy_fill.fee,
            Nat::from(quantity as u128 * TAKER_FEE_BPS as u128 / 10_000)
        );

        // Seller's maker fill: same maker price, quote-denominated maker fee.
        let sell_trades = seller_client
            .get_my_trades(by_order(sell_id.clone()))
            .await
            .unwrap();
        assert_eq!(sell_trades.len(), 1);
        let sell_fill = &sell_trades[0];
        assert_eq!(sell_fill.order_id, sell_id);
        assert_eq!(sell_fill.side, Side::Sell);
        assert!(sell_fill.is_maker);
        assert_eq!(sell_fill.notional, Nat::from(notional));
        assert_eq!(sell_fill.fee_token, PairToken::Quote);
        assert_eq!(
            sell_fill.fee,
            Nat::from(notional * MAKER_FEE_BPS as u128 / 10_000)
        );

        // get_my_orders rollups are consistent with the fills (VWAP derivable).
        let buy_record = buyer_client.get_my_order(buy_id.clone()).await.unwrap();
        assert_eq!(buy_record.order.filled_quote, Nat::from(notional));
        assert_eq!(buy_record.order.filled_quantity, Nat::from(quantity));

        // Owner-scoping and error/empty-page paths.
        assert!(
            buyer_client
                .get_my_trades(by_order(sell_id.clone()))
                .await
                .unwrap()
                .is_empty(),
            "buyer does not own the seller's order",
        );
        let unknown = "ffffffffffffffffffffffffffffffff".to_string();
        assert!(
            buyer_client
                .get_my_trades(by_order(unknown))
                .await
                .unwrap()
                .is_empty(),
        );
        assert_eq!(
            buyer_client
                .get_my_trades(by_order("not-an-order-id".to_string()))
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(GetMyTradesRequestError::InvalidOrderId)),
        );
        assert_eq!(
            buyer_client
                .get_my_trades(TradesFilter::ByOrder(TradesByOrder {
                    order_id: buy_id,
                    after: Some("bad-cursor".to_string()),
                    length: 10,
                }))
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(GetMyTradesRequestError::InvalidCursor)),
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_paginate_per_order_fills_via_trade_cursor() {
        use oisy_trade_types::{TradesByOrder, TradesFilter};

        let setup = Setup::new().await;
        setup
            .oisy_trade_client_with_caller(setup.controller())
            .add_trading_pair(setup.add_trading_pair_request())
            .await
            .unwrap();
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let seller = Principal::from_slice(&[0x02]);
        let seller_client = setup.oisy_trade_client_with_caller(seller);

        // Two resting sells at distinct prices; a single buy sweeps both, so the
        // buy order accumulates two fills (two distinct Trade.id values).
        let low_price = 9_000 * PRICE_SCALE;
        let high_price = 10_000 * PRICE_SCALE;
        let quantity = 1_000_000u64;

        fund_base(&setup, seller, 2 * quantity).await;
        for price in [low_price, high_price] {
            seller_client
                .add_limit_order(LimitOrderRequest {
                    pair: setup.trading_pair(),
                    side: Side::Sell,
                    price: Nat::from(price),
                    quantity: quantity.into(),
                    time_in_force: None,
                })
                .await
                .unwrap();
            setup.env().tick().await;
        }

        let taker_price = 11_000 * PRICE_SCALE;
        let buyer_deposit = taker_price as u128 * 2 * quantity as u128 / 1_000_000_000u128;
        fund_quote(&setup, buyer, buyer_deposit as u64).await;
        let buy_id = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Buy,
                price: Nat::from(taker_price),
                quantity: (2 * quantity).into(),
                time_in_force: None,
            })
            .await
            .unwrap();
        setup.env().tick().await;

        let page = |after: Option<String>| {
            TradesFilter::ByOrder(TradesByOrder {
                order_id: buy_id.clone(),
                after,
                length: 1,
            })
        };

        // A full unpaginated read establishes the expected newest-first order.
        let all = buyer_client
            .get_my_trades(TradesFilter::ByOrder(TradesByOrder {
                order_id: buy_id.clone(),
                after: None,
                length: 10,
            }))
            .await
            .unwrap();
        assert_eq!(all.len(), 2, "the buy order swept two maker levels");

        // Page 1: the newest fill, carrying its own id.
        let page1 = buyer_client.get_my_trades(page(None)).await.unwrap();
        assert_eq!(page1.len(), 1);
        assert_eq!(page1[0], all[0]);

        // Page 2: feeding page 1's last id back as `after` continues strictly
        // after it — the next-older fill, with no overlap or gap.
        let cursor = page1.last().unwrap().id.clone();
        let page2 = buyer_client
            .get_my_trades(page(Some(cursor)))
            .await
            .unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0], all[1]);
        assert_ne!(page1[0].id, page2[0].id);

        // Page 3: nothing older remains.
        let cursor = page2.last().unwrap().id.clone();
        let page3 = buyer_client
            .get_my_trades(page(Some(cursor)))
            .await
            .unwrap();
        assert!(page3.is_empty());

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_expose_account_wide_fills_via_get_my_trades_by_account() {
        use oisy_trade_types::{
            GetMyTradesRequestError, TradesByAccount, TradesByOrder, TradesFilter,
        };

        let setup = Setup::new().await;
        setup
            .oisy_trade_client_with_caller(setup.controller())
            .add_trading_pair(setup.add_trading_pair_request())
            .await
            .unwrap();
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let seller = Principal::from_slice(&[0x02]);
        let seller_client = setup.oisy_trade_client_with_caller(seller);

        let price = 10_000 * PRICE_SCALE;
        let quantity = 1_000_000u64;
        const ORDERS: u64 = 3;

        // The seller funds all its resting sells up front; the buyer funds all
        // its crossing buys up front.
        let seller_deposit = ORDERS * quantity;
        setup
            .deposit_flow(seller, setup.base_token_id())
            .mint(seller_deposit + 2 * BASE_LEDGER_FEE)
            .approve(seller_deposit + BASE_LEDGER_FEE)
            .deposit(seller_deposit)
            .execute()
            .await;
        let buyer_deposit = price as u128 * ORDERS as u128 * quantity as u128 / 1_000_000_000u128;
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(buyer_deposit as u64 + 2 * QUOTE_LEDGER_FEE)
            .approve(buyer_deposit as u64 + QUOTE_LEDGER_FEE)
            .deposit(buyer_deposit as u64)
            .execute()
            .await;

        // Each round rests one sell, then a buy that fully crosses it, so the
        // buyer ends up with three fills spread over three distinct buy orders.
        let mut buy_ids = Vec::new();
        for _ in 0..ORDERS {
            seller_client
                .add_limit_order(LimitOrderRequest {
                    pair: setup.trading_pair(),
                    side: Side::Sell,
                    price: Nat::from(price),
                    quantity: quantity.into(),
                    time_in_force: None,
                })
                .await
                .unwrap();
            setup.env().tick().await;
            let buy_id = buyer_client
                .add_limit_order(LimitOrderRequest {
                    pair: setup.trading_pair(),
                    side: Side::Buy,
                    price: Nat::from(price),
                    quantity: quantity.into(),
                    time_in_force: None,
                })
                .await
                .unwrap();
            setup.env().tick().await;
            buy_ids.push(buy_id);
        }

        let by_account = |after: Option<String>, length: u32| {
            TradesFilter::ByAccount(TradesByAccount { after, length })
        };

        // The full account-wide page spans all three orders, newest-first.
        let all = buyer_client
            .get_my_trades(by_account(None, 10))
            .await
            .unwrap();
        let newest_first: Vec<OrderId> = buy_ids.iter().rev().cloned().collect();
        assert_eq!(
            all.iter().map(|t| t.order_id.clone()).collect::<Vec<_>>(),
            newest_first,
            "account-wide feed returns the buyer's fills across all orders, newest-first",
        );

        // Paginate: page 1 (length 2) then resume from its last cursor — page 2
        // is strictly older, with no overlap or gap.
        let page1 = buyer_client
            .get_my_trades(by_account(None, 2))
            .await
            .unwrap();
        assert_eq!(page1, all[..2].to_vec());
        let cursor = page1.last().unwrap().id.clone();
        let page2 = buyer_client
            .get_my_trades(by_account(Some(cursor.clone()), 2))
            .await
            .unwrap();
        assert_eq!(page2, all[2..].to_vec());
        assert!(
            page2.iter().all(|t| t.id != cursor),
            "the resumed page never repeats the cursor it resumed from",
        );

        // Owner-scoped: the seller's account-wide feed is its three maker fills,
        // disjoint from the buyer's taker fills.
        let seller_trades = seller_client
            .get_my_trades(by_account(None, 10))
            .await
            .unwrap();
        assert_eq!(seller_trades.len(), ORDERS as usize);
        assert!(seller_trades.iter().all(|t| t.is_maker));
        assert!(
            seller_trades.iter().all(|t| !buy_ids.contains(&t.order_id)),
            "the seller never sees the buyer's orders",
        );

        // An unknown cursor is an empty page; a malformed one is an error.
        let unknown = "f".repeat(48);
        assert!(
            buyer_client
                .get_my_trades(by_account(Some(unknown), 10))
                .await
                .unwrap()
                .is_empty(),
        );
        assert_eq!(
            buyer_client
                .get_my_trades(by_account(Some("bad-cursor".to_string()), 10))
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(GetMyTradesRequestError::InvalidCursor)),
        );

        // Account and per-order feeds agree on the newest fill.
        let newest_order = newest_first[0].clone();
        let by_order = buyer_client
            .get_my_trades(TradesFilter::ByOrder(TradesByOrder {
                order_id: newest_order,
                after: None,
                length: 10,
            }))
            .await
            .unwrap();
        assert_eq!(by_order.len(), 1);
        assert_eq!(by_order[0], all[0]);

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_enforce_notional_bounds_at_placement() {
        let setup = Setup::new().await;
        let controller_client = setup.oisy_trade_client_with_caller(setup.controller());
        // notional = price * quantity / 10^9. With price = 10_000 * PRICE_SCALE
        // and 1 lot (LOT_SIZE base units), one lot is worth 1_000_000_000 quote
        // units, so notional = lots * 1_000_000_000.
        let min_notional = 2_000_000_000u64;
        let max_notional = 5_000_000_000u64;
        let result = controller_client
            .add_trading_pair(AddTradingPairRequest {
                min_notional: Nat::from(min_notional),
                max_notional: Some(Nat::from(max_notional)),
                ..setup.add_trading_pair_request()
            })
            .await;
        assert_eq!(result, Ok(()));

        let client = setup.oisy_trade_client();
        let pair = setup.trading_pair();
        let price = Nat::from(10_000 * PRICE_SCALE);
        let order = |lots: u64| LimitOrderRequest {
            pair,
            side: Side::Buy,
            price: price.clone(),
            quantity: Nat::from(lots * LOT_SIZE),
            time_in_force: None,
        };

        // 1 lot -> notional 1_000_000_000 < min: rejected.
        assert_eq!(
            client.add_limit_order(order(1)).await.unwrap_err().kind,
            ErrorKind::RequestError(Some(AddLimitOrderRequestError::InvalidNotional {
                notional: Nat::from(1_000_000_000u64),
                min: Nat::from(min_notional),
                max: Some(Nat::from(max_notional)),
            }))
        );

        // 6 lots -> notional 6_000_000_000 > max: rejected.
        assert_eq!(
            client.add_limit_order(order(6)).await.unwrap_err().kind,
            ErrorKind::RequestError(Some(AddLimitOrderRequestError::InvalidNotional {
                notional: Nat::from(6_000_000_000u64),
                min: Nat::from(min_notional),
                max: Some(Nat::from(max_notional)),
            }))
        );

        // 3 lots -> notional 3_000_000_000 within [min, max]: accepted once funded.
        let required = 3_000_000_000u64;
        let fee = QUOTE_LEDGER_FEE;
        setup
            .deposit_flow(setup.user(), setup.quote_token_id())
            .mint(required + 2 * fee)
            .approve(required + fee)
            .deposit(required)
            .execute()
            .await;
        client.add_limit_order(order(3)).await.unwrap();

        setup.drop().await;
    }
}

mod fill_or_kill {
    use candid::{Nat, Principal};
    use oisy_trade_int_tests::icrc_ledger::QUOTE_LEDGER_FEE;
    use oisy_trade_int_tests::{PRICE_SCALE, Setup};
    use oisy_trade_types::{Balance, LimitOrderRequest, OrderStatus, Side, TimeInForce};

    const PRICE: u64 = 10_000 * PRICE_SCALE;

    #[tokio::test]
    async fn should_fill_fok_against_sufficient_liquidity() {
        let setup = Setup::new().await.with_trading_pair().await;
        let seller = Principal::from_slice(&[0x02]);
        setup.rest_sell_maker(seller, 1_000_000, PRICE).await;

        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let required = 1_000_000_000u64;
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(required + 2 * QUOTE_LEDGER_FEE)
            .approve(required + QUOTE_LEDGER_FEE)
            .deposit(required)
            .execute()
            .await;
        let buy_id = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Buy,
                price: Nat::from(PRICE),
                quantity: 1_000_000u64.into(),
                time_in_force: Some(TimeInForce::FillOrKill),
            })
            .await
            .unwrap();
        setup.env().tick().await;

        let order = buyer_client.get_my_order(buy_id).await.unwrap().order;
        assert_eq!(order.status, OrderStatus::Filled);
        assert_eq!(order.filled_quantity, Nat::from(1_000_000u64));
        assert_eq!(
            buyer_client
                .get_balance(setup.base_token_id())
                .await
                .unwrap(),
            Balance {
                free: 1_000_000u64.into(),
                reserved: 0u64.into(),
            },
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_expire_fok_against_insufficient_liquidity_without_partial_fill() {
        let setup = Setup::new().await.with_trading_pair().await;
        let seller = Principal::from_slice(&[0x02]);
        // Only one lot of the two lots the FOK wants rests in the book.
        let sell_id = setup.rest_sell_maker(seller, 1_000_000, PRICE).await;

        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let required = 2_000_000_000u64;
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(required + 2 * QUOTE_LEDGER_FEE)
            .approve(required + QUOTE_LEDGER_FEE)
            .deposit(required)
            .execute()
            .await;
        let buy_id = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Buy,
                price: Nat::from(PRICE),
                quantity: 2_000_000u64.into(),
                time_in_force: Some(TimeInForce::FillOrKill),
            })
            .await
            .unwrap();
        setup.env().tick().await;

        let order = buyer_client.get_my_order(buy_id).await.unwrap().order;
        assert_eq!(order.status, OrderStatus::Expired);
        assert_eq!(
            order.filled_quantity,
            Nat::from(0u64),
            "FOK must not settle a partial fill"
        );
        assert_eq!(
            buyer_client
                .get_balance(setup.quote_token_id())
                .await
                .unwrap(),
            Balance {
                free: required.into(),
                reserved: 0_u64.into(),
            },
        );

        // The resting maker was not touched: it stays Open with its base still
        // reserved and nothing filled.
        let seller_client = setup.oisy_trade_client_with_caller(seller);
        let maker = seller_client.get_my_order(sell_id).await.unwrap().order;
        assert_eq!(maker.status, OrderStatus::Open);
        assert_eq!(maker.filled_quantity, Nat::from(0u64));
        assert_eq!(
            seller_client
                .get_balance(setup.base_token_id())
                .await
                .unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: 1_000_000u64.into(),
            },
        );

        setup.drop().await;
    }

    /// A killed FOK survives a canister upgrade: the `Expired` terminal state
    /// and the released reservation are reconstructed by pre_upgrade snapshot →
    /// post_upgrade load + event-log replay, and the refund is not re-applied.
    #[tokio::test]
    async fn should_preserve_expired_fok_and_refund_across_upgrade() {
        let setup = Setup::new().await.with_trading_pair().await;
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let required = 1_000_000_000u64;
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(required + 2 * QUOTE_LEDGER_FEE)
            .approve(required + QUOTE_LEDGER_FEE)
            .deposit(required)
            .execute()
            .await;
        // FOK Buy against an empty book → killed to Expired with the whole
        // reservation refunded.
        let buy_id = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Buy,
                price: Nat::from(PRICE),
                quantity: 1_000_000u64.into(),
                time_in_force: Some(TimeInForce::FillOrKill),
            })
            .await
            .unwrap();
        setup.env().tick().await;

        let before_order = buyer_client
            .get_my_order(buy_id.clone())
            .await
            .unwrap()
            .order;
        assert_eq!(before_order.status, OrderStatus::Expired);
        let before_balance = buyer_client
            .get_balance(setup.quote_token_id())
            .await
            .unwrap();
        assert_eq!(
            before_balance,
            Balance {
                free: required.into(),
                reserved: 0u64.into(),
            },
        );

        // pre_upgrade snapshot save → post_upgrade load + replay.
        setup.upgrade(None).await;

        let after_order = buyer_client.get_my_order(buy_id).await.unwrap().order;
        assert_eq!(
            after_order, before_order,
            "Expired state must survive upgrade"
        );
        let after_balance = buyer_client
            .get_balance(setup.quote_token_id())
            .await
            .unwrap();
        assert_eq!(
            after_balance, before_balance,
            "reservation release must not be double-applied on replay",
        );

        setup.drop().await;
    }
}

mod cancel_limit_order {
    use candid::{Nat, Principal};
    use oisy_trade_int_tests::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
    use oisy_trade_int_tests::{PRICE_SCALE, Setup};
    use oisy_trade_types::{
        AddTradingPairRequest, Balance, CancelLimitOrderRequestError, ErrorKind, LimitOrderRequest,
        OrderRecord, OrderStatus, Side, TimeInForce,
    };

    #[tokio::test]
    async fn should_cancel_partially_filled_buy_and_refund_residual() {
        // Non-zero maker/taker fees so the partially filled buy accrues a
        // non-trivial `filled_fee` and a `filled_quote` consistent with the
        // realized notional.
        const MAKER_FEE_BPS: u16 = 10;
        const TAKER_FEE_BPS: u16 = 23;
        let setup = Setup::new().await;
        setup
            .oisy_trade_client_with_caller(setup.controller())
            .add_trading_pair(AddTradingPairRequest {
                maker_fee_bps: MAKER_FEE_BPS,
                taker_fee_bps: TAKER_FEE_BPS,
                ..setup.add_trading_pair_request()
            })
            .await
            .unwrap();
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let seller = Principal::from_slice(&[0x02]);
        let seller_client = setup.oisy_trade_client_with_caller(seller);

        // Buyer wants 3M base @ 1000 → reserves 3000M quote.
        // Seller supplies only 1M base @ 1000 → fills 1M, 2M residual on buy.
        let buyer_deposit = 3_000_000_000u64;
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(buyer_deposit + 2 * QUOTE_LEDGER_FEE)
            .approve(buyer_deposit + QUOTE_LEDGER_FEE)
            .deposit(buyer_deposit)
            .execute()
            .await;
        let seller_deposit = 1_000_000u64;
        setup
            .deposit_flow(seller, setup.base_token_id())
            .mint(seller_deposit + 2 * BASE_LEDGER_FEE)
            .approve(seller_deposit + BASE_LEDGER_FEE)
            .deposit(seller_deposit)
            .execute()
            .await;

        let before_placement = setup.time_nanos().await;
        let buy_id = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Buy,
                price: Nat::from(10_000 * PRICE_SCALE),
                quantity: 3_000_000u64.into(),
                time_in_force: None,
            })
            .await
            .unwrap();
        let after_placement = setup.time_nanos().await;
        seller_client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Sell,
                price: Nat::from(10_000 * PRICE_SCALE),
                quantity: 1_000_000u64.into(),
                time_in_force: None,
            })
            .await
            .unwrap();
        setup.env().tick().await;

        // Buyer: 1M base filled, 2000M quote still reserved for the 2M residual.
        // Partially filled, resting Open with 1M of 3M filled.
        let resting = buyer_client
            .get_my_order(buy_id.clone())
            .await
            .expect("buyer owns the order");
        assert_eq!(resting.order.status, OrderStatus::Open);
        assert_eq!(resting.order.filled_quantity, Nat::from(1_000_000u64));
        assert_eq!(
            buyer_client
                .get_balance(setup.quote_token_id())
                .await
                .unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: 2_000_000_000u64.into(),
            }
        );

        assert_eq!(
            seller_client
                .cancel_limit_order(buy_id.clone())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(CancelLimitOrderRequestError::NotOrderOwner)),
            "only buyer can cancel buy order"
        );

        let canceled = buyer_client
            .cancel_limit_order(buy_id.clone())
            .await
            .unwrap();
        // `created_at` must carry the order's *submission* time, not the cancel
        // time (a tick ran in between), so pin it to the placement window.
        assert!(
            before_placement <= canceled.created_at && canceled.created_at <= after_placement,
            "submission timestamp {} should fall within the placement window [{before_placement}, {after_placement}]",
            canceled.created_at,
        );
        // Cancel stamps `last_updated_at`; it post-dates placement.
        assert!(
            canceled
                .last_updated_at
                .is_some_and(|t| t >= canceled.created_at),
            "last_updated_at {:?} should be set at/after created_at {}",
            canceled.last_updated_at,
            canceled.created_at,
        );
        // The canceled record keeps its 1M filled; remaining (2M) is derived as
        // quantity − filled_quantity. `Canceled` is a unit variant. The
        // timestamps are checked in the windows above, so reuse them here.
        assert_eq!(
            canceled,
            OrderRecord {
                owner: buyer,
                side: Side::Buy,
                price: Nat::from(10_000 * PRICE_SCALE),
                quantity: Nat::from(3_000_000u64),
                filled_quantity: Nat::from(1_000_000u64),
                status: OrderStatus::Canceled,
                created_at: canceled.created_at,
                last_updated_at: canceled.last_updated_at,
                time_in_force: TimeInForce::GoodTilCanceled,
                filled_quote: Nat::from(1_000_000_000u64),
                // Buyer rested first, so it is the maker: a base-token fee at the
                // maker rate on the 1M filled = ceil(1_000_000 × 10 / 10_000).
                filled_fee: Nat::from(1_000u64),
            }
        );

        assert_eq!(
            buyer_client
                .get_my_order(buy_id)
                .await
                .unwrap()
                .order
                .status,
            OrderStatus::Canceled
        );
        assert_eq!(
            buyer_client
                .get_balance(setup.quote_token_id())
                .await
                .unwrap(),
            Balance {
                free: 2_000_000_000u64.into(),
                reserved: 0u64.into(),
            }
        );
        assert_eq!(
            buyer_client
                .get_balance(setup.base_token_id())
                .await
                .unwrap(),
            Balance {
                // 1M base received minus the 1_000 maker fee withheld on the
                // base side.
                free: 999_000u64.into(),
                reserved: 0u64.into(),
            }
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_reject_cancel_of_unknown_order() {
        let setup = Setup::new().await.with_trading_pair().await;
        let client = setup.oisy_trade_client();

        // Valid hex format but refers to a non-existent book/seq.
        assert_eq!(
            client
                .cancel_limit_order("ffffffffffffffffffffffffffffffff".to_string())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(CancelLimitOrderRequestError::OrderNotFound))
        );
        assert_eq!(
            client
                .cancel_limit_order("not-a-valid-id".to_string())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::RequestError(Some(CancelLimitOrderRequestError::InvalidOrderId))
        );

        setup.drop().await;
    }
}

#[tokio::test]
async fn should_return_empty_trading_pairs() {
    let setup = Setup::new().await;
    let client = setup.oisy_trade_client();
    assert_eq!(client.get_trading_pairs().await, vec![]);

    let setup = setup.with_trading_pair().await;
    let client = setup.oisy_trade_client();

    assert_eq!(
        client.get_trading_pairs().await,
        vec![TradingPairInfo {
            base: Token {
                id: setup.base_token_id(),
                metadata: TokenMetadata {
                    symbol: "ckSOL".to_string(),
                    decimals: 9,
                },
            },
            quote: Token {
                id: setup.quote_token_id(),
                metadata: TokenMetadata {
                    symbol: "ckBTC".to_string(),
                    decimals: 8,
                },
            },
            status: TradingStatus::Trading,
            tick_size: Nat::from(TICK_SIZE),
            lot_size: Nat::from(LOT_SIZE),
            maker_fee_bps: 0,
            taker_fee_bps: 0,
            min_notional: Nat::from(1u64),
            max_notional: None,
        }]
    );

    setup.drop().await;
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

#[tokio::test]
async fn should_deposit_and_track_balances() {
    let setup = Setup::new().await.with_trading_pair().await;

    let user1 = Principal::from_slice(&[0x01]);
    let user2 = Principal::from_slice(&[0x02]);

    let cksol = TokenId {
        ledger_id: setup.base_ledger_id(),
    };
    let ckbtc = TokenId {
        ledger_id: setup.quote_ledger_id(),
    };

    let oisy_trade_account = Account {
        owner: setup.oisy_trade_id(),
        subaccount: None,
    };

    // Mint tokens to users
    setup
        .mint_base_tokens(user1, Nat::from(10_000_000u64))
        .await;
    setup
        .mint_base_tokens(user2, Nat::from(20_000_000u64))
        .await;
    setup
        .mint_quote_tokens(user1, Nat::from(10_000_000u64))
        .await;
    setup
        .mint_quote_tokens(user2, Nat::from(20_000_000u64))
        .await;

    // Approve OISY TRADE canister to spend on behalf of users
    let base_ledger = setup.base_token_ledger();
    let quote_ledger = setup.quote_token_ledger();
    base_ledger
        .icrc2_approve(user1, oisy_trade_account, Nat::from(5_000_000u64))
        .await;
    base_ledger
        .icrc2_approve(user2, oisy_trade_account, Nat::from(5_000_000u64))
        .await;
    quote_ledger
        .icrc2_approve(user1, oisy_trade_account, Nat::from(5_000_000u64))
        .await;
    quote_ledger
        .icrc2_approve(user2, oisy_trade_account, Nat::from(5_000_000u64))
        .await;

    let client1 = setup.oisy_trade_client_with_caller(user1);
    let client2 = setup.oisy_trade_client_with_caller(user2);

    // Verify initial balances are zero
    assert_balances(&client1, &client2, &cksol, &ckbtc, 0, 0, 0, 0).await;

    // Deposit ckSOL for user1
    client1
        .deposit(DepositRequest {
            token_id: cksol.clone(),
            amount: Nat::from(1_000_000u64),
        })
        .await
        .expect("user1 ckSOL deposit failed");
    assert_balances(&client1, &client2, &cksol, &ckbtc, 1_000_000, 0, 0, 0).await;

    // Deposit ckBTC for user1
    client1
        .deposit(DepositRequest {
            token_id: ckbtc.clone(),
            amount: Nat::from(500_000u64),
        })
        .await
        .expect("user1 ckBTC deposit failed");
    assert_balances(&client1, &client2, &cksol, &ckbtc, 1_000_000, 500_000, 0, 0).await;

    // Deposit ckSOL for user2
    client2
        .deposit(DepositRequest {
            token_id: cksol.clone(),
            amount: Nat::from(2_000_000u64),
        })
        .await
        .expect("user2 ckSOL deposit failed");
    assert_balances(
        &client1, &client2, &cksol, &ckbtc, 1_000_000, 500_000, 2_000_000, 0,
    )
    .await;

    // Deposit ckBTC for user2
    client2
        .deposit(DepositRequest {
            token_id: ckbtc.clone(),
            amount: Nat::from(3_000_000u64),
        })
        .await
        .expect("user2 ckBTC deposit failed");
    assert_balances(
        &client1, &client2, &cksol, &ckbtc, 1_000_000, 500_000, 2_000_000, 3_000_000,
    )
    .await;

    setup.drop().await;
}

/// Test parameters for deposit failure scenarios.
struct DepositFailureCase {
    mint_amount: u64,
    approve_amount: u64,
    deposit_amount: u64,
    expected_error: fn(fee: Nat) -> DepositError,
}

async fn test_deposit_failure(case: DepositFailureCase) {
    let setup = Setup::new().await.with_trading_pair().await;

    let user = Principal::from_slice(&[0x03]);
    let cksol = TokenId {
        ledger_id: setup.base_ledger_id(),
    };
    let oisy_trade_account = Account {
        owner: setup.oisy_trade_id(),
        subaccount: None,
    };
    let fee = setup.base_token_ledger().icrc1_fee().await;

    setup
        .mint_base_tokens(user, Nat::from(case.mint_amount))
        .await;
    setup
        .base_token_ledger()
        .icrc2_approve(user, oisy_trade_account, Nat::from(case.approve_amount))
        .await;

    let result = setup
        .oisy_trade_client_with_caller(user)
        .deposit(DepositRequest {
            token_id: cksol,
            amount: Nat::from(case.deposit_amount),
        })
        .await;

    assert_eq!(result, Err((case.expected_error)(fee)));

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_deposit_with_insufficient_funds() {
    test_deposit_failure(DepositFailureCase {
        mint_amount: 1_000_000,
        approve_amount: 5_000_000,
        deposit_amount: 2_000_000,
        expected_error: |fee| {
            DepositError::request(DepositRequestError::InsufficientFunds {
                // The user's balance is the minted amount minus the fee charged for icrc2_approve
                balance: Nat::from(1_000_000u64) - fee,
            })
        },
    })
    .await;
}

#[tokio::test]
async fn should_fail_deposit_with_insufficient_allowance() {
    test_deposit_failure(DepositFailureCase {
        mint_amount: 10_000_000,
        approve_amount: 500_000,
        deposit_amount: 1_000_000,
        expected_error: |_fee| {
            DepositError::request(DepositRequestError::InsufficientAllowance {
                allowance: Nat::from(500_000u64),
            })
        },
    })
    .await;
}

#[tokio::test]
async fn should_fail_deposit_with_unsupported_token() {
    let setup = Setup::new().await;

    let user = Principal::from_slice(&[0x05]);
    let fake_token = TokenId {
        ledger_id: setup.oisy_trade_id(),
    };

    let result = setup
        .oisy_trade_client_with_caller(user)
        .deposit(DepositRequest {
            token_id: fake_token.clone(),
            amount: Nat::from(1_000_000u64),
        })
        .await;

    assert_eq!(
        result.unwrap_err().kind,
        ErrorKind::RequestError(Some(DepositRequestError::UnsupportedToken {
            token_id: fake_token,
        }))
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_deposit_when_ledger_is_stopped() {
    let setup = Setup::new().await.with_trading_pair().await;

    let user = Principal::from_slice(&[0x06]);
    let controller = setup.controller();
    let ledger_id = setup.base_ledger_id();

    // Stop the ledger so it rejects all incoming messages
    setup
        .env()
        .stop_canister(ledger_id, Some(controller))
        .await
        .expect("Failed to stop canister");

    let result = setup
        .oisy_trade_client_with_caller(user)
        .deposit(DepositRequest {
            token_id: TokenId { ledger_id },
            amount: Nat::from(1_000_000u64),
        })
        .await;

    assert_matches!(
        result.unwrap_err().kind,
        ErrorKind::TemporaryError(Some(DepositTemporaryError::CallFailed { reason, .. }))
            if reason.contains("is stopped")
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_add_trading_pair() {
    let setup = Setup::new().await;
    let controller_client = setup.oisy_trade_client_with_caller(setup.controller());
    let user = Principal::from_slice(&[0x01]);
    let user_client = setup.oisy_trade_client_with_caller(user);

    // not controller
    let result = user_client
        .add_trading_pair(setup.add_trading_pair_request())
        .await;
    assert_eq!(result, Err(AddTradingPairError::NotController));

    // base equals quote
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            base: Token {
                id: TokenId {
                    ledger_id: setup.base_ledger_id(),
                },
                metadata: TokenMetadata {
                    symbol: "ckSOL".to_string(),
                    decimals: 9,
                },
            },
            quote: Token {
                id: TokenId {
                    ledger_id: setup.base_ledger_id(),
                },
                metadata: TokenMetadata {
                    symbol: "ckSOL".to_string(),
                    decimals: 9,
                },
            },
            ..setup.add_trading_pair_request()
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::BaseEqualsQuote));

    // zero tick size
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            tick_size: Nat::from(0u64),
            ..setup.add_trading_pair_request()
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::InvalidTickSize));

    // zero lot size
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            lot_size: Nat::from(0u64),
            ..setup.add_trading_pair_request()
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::InvalidLotSize));

    // already exists
    let result = controller_client
        .add_trading_pair(setup.add_trading_pair_request())
        .await;
    assert_eq!(result, Ok(()));

    let result = controller_client
        .add_trading_pair(setup.add_trading_pair_request())
        .await;
    assert_eq!(result, Err(AddTradingPairError::TradingPairAlreadyExists));

    setup.drop().await;
}

#[tokio::test]
async fn should_replay_events_on_upgrade() {
    use oisy_trade_types_internal::event::EventType;

    /// Asserts that the values produced by each `$observe` expression are unchanged after
    /// a single canister upgrade. Accepts one or more expressions separated by commas.
    macro_rules! assert_preserved_after_upgrade {
    ($setup:expr, $($observe:expr),+ $(,)?) => {{
        let before = ($($observe.await,)+);
        $setup.upgrade(None).await;
        let after = ($($observe.await,)+);
        assert_eq!(before, after);
    }};
}

    // 1) Init -> Upgrade -> no trading pairs
    let setup = Setup::new().await;
    assert_preserved_after_upgrade!(setup, setup.oisy_trade_client().get_trading_pairs());
    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 1);
        assert_matches!(&events[0], EventType::Init(_));
    });

    // 2) Add trading pair -> Upgrade -> trading pair preserved
    let request = AddTradingPairRequest {
        maker_fee_bps: 10,
        taker_fee_bps: 23,
        ..setup.add_trading_pair_request()
    };
    let maker_fee_bps = request.maker_fee_bps;
    let taker_fee_bps = request.taker_fee_bps;
    let result = setup
        .oisy_trade_client_with_caller(setup.controller())
        .add_trading_pair(request)
        .await;
    assert_eq!(result, Ok(()));
    assert_preserved_after_upgrade!(setup, setup.oisy_trade_client().get_trading_pairs());
    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 2);
        assert_matches!(&events[1], EventType::AddTradingPair(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::AddTradingPairEvent {
                book_id: 0,
                base: setup.base_token_id(),
                quote: setup.quote_token_id(),
                tick_size: Nat::from(TICK_SIZE),
                lot_size: Nat::from(LOT_SIZE),
                base_metadata: TokenMetadata { symbol: "ckSOL".to_string(), decimals: 9 },
                quote_metadata: TokenMetadata { symbol: "ckBTC".to_string(), decimals: 8 },
                maker_fee_bps,
                taker_fee_bps,
                min_notional: Nat::from(1u64),
                max_notional: None,
            });
        });
    });

    // 3) Deposit -> Upgrade -> balance preserved
    let deposit_amount: u64 = 1_000_000;
    setup
        .deposit_flow(setup.user(), setup.base_token_id())
        .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
        .approve(deposit_amount + BASE_LEDGER_FEE)
        .deposit(deposit_amount)
        .execute()
        .await;
    assert_preserved_after_upgrade!(
        setup,
        setup.oisy_trade_client().get_balance(setup.base_token_id())
    );
    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 3);
        assert_matches!(&events[2], EventType::Deposit(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::DepositEvent {
                user: setup.user(),
                token: setup.base_token_id(),
                amount: Nat::from(deposit_amount),
            });
        });
    });

    // 4) AddLimitOrder -> Upgrade -> order status and reserved balance preserved
    // Reuse the base token deposit from step 3 to place a sell order.
    let order_id = setup
        .oisy_trade_client()
        .add_limit_order(oisy_trade_types::LimitOrderRequest {
            pair: setup.trading_pair(),
            side: oisy_trade_types::Side::Sell,
            price: Nat::from(10_000 * PRICE_SCALE),
            quantity: Nat::from(deposit_amount),
            time_in_force: None,
        })
        .await
        .unwrap();
    // Let the matching timer fire so the resting order transitions from
    // `Pending` to `Open` before taking the pre-upgrade snapshot. This pins
    // the "before" side of the comparison so the assertion is insensitive to
    // how many pocket-ic ticks the upgrade itself advances.
    setup.env().tick().await;
    assert_preserved_after_upgrade!(
        setup,
        setup.oisy_trade_client().get_my_order(order_id.clone()),
        setup.oisy_trade_client().get_balance(setup.base_token_id()),
    );
    setup.assert_that_events().await.satisfy(|events| {
        // Init + AddTradingPair + Deposit + AddLimitOrder + Matching.
        // The resting sell has no cross: `Matching` enumerates the pending
        // seq and applies the Pending->Open status transition synchronously
        // (no balance ops, so no `Settling` event is emitted).
        assert_eq!(events.len(), 5);
        assert_matches!(&events[3], EventType::AddLimitOrder(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::AddLimitOrderEvent {
                user: setup.user(),
                order_id: oisy_trade_types_internal::event::OrderId { book_id: 0, seq: 0 },
                side: oisy_trade_types::Side::Sell,
                price: Nat::from(10_000 * PRICE_SCALE),
                quantity: Nat::from(deposit_amount),
                time_in_force: oisy_trade_types::TimeInForce::GoodTilCanceled,
            });
        });
        assert_matches!(&events[4], EventType::Matching(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::MatchingEvent {
                book_id: 0,
                orders: vec![0],
            });
        });
    });

    // 5) Crossing buy fully fills the resting sell from step 4. Settling now
    // carries two Transfer ops and two Filled transitions; equal prices mean
    // no Unreserve (the price-improvement path is covered in the unit test
    // `should_replay_matching_with_price_improvement`).
    let buyer = Principal::from_slice(&[0x42]);
    let price: u64 = 10_000 * PRICE_SCALE;
    // Settlement is `price × quantity / 10^base_decimals` (ckSOL base = 9 dec).
    let quote_reserved = price * deposit_amount / 1_000_000_000;
    setup
        .deposit_flow(buyer, setup.quote_token_id())
        .mint(quote_reserved + 2 * QUOTE_LEDGER_FEE)
        .approve(quote_reserved + QUOTE_LEDGER_FEE)
        .deposit(quote_reserved)
        .execute()
        .await;
    let buy_order_id = setup
        .oisy_trade_client_with_caller(buyer)
        .add_limit_order(oisy_trade_types::LimitOrderRequest {
            pair: setup.trading_pair(),
            side: oisy_trade_types::Side::Buy,
            price: Nat::from(price),
            quantity: Nat::from(deposit_amount),
            time_in_force: None,
        })
        .await
        .unwrap();
    // Let the matching timer fire so both orders transition to Filled before
    // snapshotting.
    setup.env().tick().await;
    assert_preserved_after_upgrade!(
        setup,
        setup.oisy_trade_client().get_my_order(order_id.clone()),
        setup
            .oisy_trade_client_with_caller(buyer)
            .get_my_order(buy_order_id.clone()),
        setup.oisy_trade_client().get_balance(setup.base_token_id()),
        setup
            .oisy_trade_client()
            .get_balance(setup.quote_token_id()),
        setup
            .oisy_trade_client_with_caller(buyer)
            .get_balance(setup.base_token_id()),
        setup
            .oisy_trade_client_with_caller(buyer)
            .get_balance(setup.quote_token_id()),
    );
    setup.assert_that_events().await.satisfy(|events| {
        // Step 4 produced 5 events; step 5 adds Deposit (buyer) + AddLimitOrder
        // + Matching + Settling.
        assert_eq!(events.len(), 9);
        assert_matches!(&events[5], EventType::Deposit(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::DepositEvent {
                user: buyer,
                token: setup.quote_token_id(),
                amount: Nat::from(quote_reserved),
            });
        });
        assert_matches!(&events[6], EventType::AddLimitOrder(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::AddLimitOrderEvent {
                user: buyer,
                order_id: oisy_trade_types_internal::event::OrderId { book_id: 0, seq: 1 },
                side: oisy_trade_types::Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(deposit_amount),
                time_in_force: oisy_trade_types::TimeInForce::GoodTilCanceled,
            });
        });
        assert_matches!(&events[7], EventType::Matching(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::MatchingEvent {
                book_id: 0,
                orders: vec![1],
            });
        });
        assert_matches!(&events[8], EventType::Settling(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::SettlingEvent {
                book_id: 0,
                balance_operations: vec![
                    oisy_trade_types_internal::event::BalanceOperation::Transfer {
                        from_order: 1, // buyer seq
                        to_order: 0,   // seller seq
                        token: oisy_trade_types_internal::event::PairToken::Quote,
                        amount: Nat::from(quote_reserved),
                        fee: Some(Nat::from((quote_reserved * maker_fee_bps as u64).div_ceil(10_000))),
                    },
                    oisy_trade_types_internal::event::BalanceOperation::Transfer {
                        from_order: 0,
                        to_order: 1,
                        token: oisy_trade_types_internal::event::PairToken::Base,
                        amount: Nat::from(deposit_amount),
                        fee: Some(Nat::from((deposit_amount * taker_fee_bps as u64).div_ceil(10_000))),
                    },
                ],
            });
        });
    });

    setup.drop().await;
}

#[tokio::test]
async fn should_withdraw_and_receive_tokens_on_ledger() {
    use oisy_trade_types_internal::event::EventType;

    let setup = Setup::new().await.with_trading_pair().await;
    let user = Principal::from_slice(&[0x01]);
    let client = setup.oisy_trade_client_with_caller(user);
    let cksol = TokenId {
        ledger_id: setup.base_ledger_id(),
    };
    let fee = Nat::from(BASE_LEDGER_FEE);

    // Deposit tokens first
    let deposit_amount = 10_000_000u64;
    setup
        .deposit_flow(user, cksol.clone())
        .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
        .approve(deposit_amount + BASE_LEDGER_FEE)
        .deposit(deposit_amount)
        .execute()
        .await;
    assert_eq!(
        client.get_balance(cksol.clone()).await.unwrap(),
        expected_balance(deposit_amount)
    );

    // Withdraw half
    let withdraw_amount = 5_000_000u64;
    let response = client
        .withdraw(WithdrawRequest {
            token_id: cksol.clone(),
            amount: Nat::from(withdraw_amount),
        })
        .await
        .expect("withdrawal should succeed");
    let expected_block_index =
        u64::try_from(&response.block_index.0).expect("ledger block_index fits in u64");

    // OISY TRADE balance decreased by the full withdraw amount
    assert_eq!(
        client.get_balance(cksol.clone()).await.unwrap(),
        expected_balance(deposit_amount - withdraw_amount)
    );

    // User received (withdraw_amount - fee) on the ledger
    let ledger_balance = setup.base_token_ledger().icrc1_balance_of(user).await;
    assert_eq!(ledger_balance, Nat::from(withdraw_amount) - fee);

    // The successful withdrawal is recorded in the audit log, including the
    // ledger block index returned from the transfer.
    setup.assert_that_events().await.satisfy(|events| {
        let withdraw = events
            .iter()
            .find(|e| matches!(e, EventType::Withdraw(_)))
            .expect("expected a Withdraw event in the log");
        assert_matches!(withdraw, EventType::Withdraw(e) => {
            assert_eq!(*e, oisy_trade_types_internal::event::WithdrawEvent {
                block_index: expected_block_index,
                user,
                token: cksol.clone(),
                amount: Nat::from(withdraw_amount),
            });
        });
    });

    setup.drop().await;
}

/// End-to-end walkthrough from `docs/src/usage/for-users.md` on the documented
/// SOLETH pair (ckDevnetSOL / ckSepoliaETH, with their decimals and ledger
/// fees): list pairs, both sides approve+deposit, a resting sell and a crossing
/// buy fill, the taker fee is charged on the base the buyer receives, then each
/// side withdraws what it received and lands on the ledger net of the ledger fee.
#[tokio::test]
async fn should_complete_for_users_walkthrough() {
    // Binance SOLETH parameters from the docs: ckDevnetSOL has 9 decimals,
    // ckSepoliaETH has 18; tick = 0.00001 ETH/SOL × 10^18, lot = 0.001 SOL × 10^9.
    const TICK_SIZE_SOLETH: u64 = 10_000_000_000_000;
    const LOT_SIZE_SOLETH: u64 = 1_000_000;
    const MAKER_FEE_BPS: u16 = 0;
    const TAKER_FEE_BPS: u16 = 20;

    let base_config = LedgerConfig::ckdevnetsol();
    let quote_config = LedgerConfig::cksepoliaeth();
    let base_ledger_fee = base_config.transfer_fee;
    let quote_ledger_fee = quote_config.transfer_fee;

    let setup = Setup::builder()
        .with_ledgers(base_config, quote_config)
        .build()
        .await;
    setup
        .oisy_trade_client_with_caller(setup.controller())
        .add_trading_pair(AddTradingPairRequest {
            tick_size: Nat::from(TICK_SIZE_SOLETH),
            lot_size: Nat::from(LOT_SIZE_SOLETH),
            maker_fee_bps: MAKER_FEE_BPS,
            taker_fee_bps: TAKER_FEE_BPS,
            ..setup.add_trading_pair_request()
        })
        .await
        .unwrap();

    let seller = Principal::from_slice(&[0x01]);
    let buyer = Principal::from_slice(&[0x02]);
    let seller_client = setup.oisy_trade_client_with_caller(seller);
    let buyer_client = setup.oisy_trade_client_with_caller(buyer);

    // 1) List trading pairs — the pair is listed with its tick/lot sizes and
    // maker/taker fee rates.
    let pairs = setup.oisy_trade_client().get_trading_pairs().await;
    let pair_info = pairs
        .iter()
        .find(|info| {
            info.base.id.ledger_id == setup.base_ledger_id()
                && info.quote.id.ledger_id == setup.quote_ledger_id()
        })
        .expect("the registered pair must be listed");
    assert_eq!(pair_info.tick_size, Nat::from(TICK_SIZE_SOLETH));
    assert_eq!(pair_info.lot_size, Nat::from(LOT_SIZE_SOLETH));
    assert_eq!(pair_info.maker_fee_bps, MAKER_FEE_BPS);
    assert_eq!(pair_info.taker_fee_bps, TAKER_FEE_BPS);

    // Trade 0.1 SOL @ 0.05 ETH/SOL. Price is quote base units per whole base
    // token (0.05 × 10^18); the base has 9 decimals, so the fill settles
    // price × quantity / 10^9 quote base units.
    let quantity = 100_000_000u64; // 0.1 SOL = 100 lots
    let price = 50_000_000_000_000_000u64; // 0.05 ETH/SOL × 10^18
    let quote_notional = 5_000_000_000_000_000u64; // = price × quantity / 10^9 (0.005 ETH)
    // Taker (buyer) pays the taker fee on the base it receives; the maker
    // (seller) pays nothing here, so it keeps the full notional.
    let taker_fee = (quantity * u64::from(TAKER_FEE_BPS)).div_ceil(10_000);
    let buyer_base_received = quantity - taker_fee;

    // 2) Approve + deposit — seller funds base to sell, buyer funds quote to buy.
    setup
        .deposit_flow(seller, setup.base_token_id())
        .mint(quantity + 2 * base_ledger_fee)
        .approve(quantity + base_ledger_fee)
        .deposit(quantity)
        .execute()
        .await;
    setup
        .deposit_flow(buyer, setup.quote_token_id())
        .mint(quote_notional + 2 * quote_ledger_fee)
        .approve(quote_notional + quote_ledger_fee)
        .deposit(quote_notional)
        .execute()
        .await;

    // 3) Place a limit order — the sell rests (maker), the matching buy crosses
    // it (taker).
    let sell_order_id = seller_client
        .add_limit_order(LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Sell,
            price: Nat::from(price),
            quantity: Nat::from(quantity),
            time_in_force: None,
        })
        .await
        .unwrap();
    let buy_order_id = buyer_client
        .add_limit_order(LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Buy,
            price: Nat::from(price),
            quantity: Nat::from(quantity),
            time_in_force: None,
        })
        .await
        .unwrap();
    setup.env().tick().await;

    // 4) Check order status — both fully filled, balances settled. The seller
    // receives the full notional (maker fee 0); the buyer receives the base
    // net of the taker fee.
    assert_eq!(
        seller_client
            .get_my_order(sell_order_id)
            .await
            .unwrap()
            .order
            .status,
        OrderStatus::Filled
    );
    assert_eq!(
        buyer_client
            .get_my_order(buy_order_id)
            .await
            .unwrap()
            .order
            .status,
        OrderStatus::Filled
    );
    assert_eq!(
        seller_client
            .get_balance(setup.base_token_id())
            .await
            .unwrap(),
        expected_balance(0)
    );
    assert_eq!(
        seller_client
            .get_balance(setup.quote_token_id())
            .await
            .unwrap(),
        expected_balance(quote_notional)
    );
    assert_eq!(
        buyer_client
            .get_balance(setup.base_token_id())
            .await
            .unwrap(),
        expected_balance(buyer_base_received)
    );
    assert_eq!(
        buyer_client
            .get_balance(setup.quote_token_id())
            .await
            .unwrap(),
        expected_balance(0)
    );

    // 5) Withdraw — each side withdraws what it received and lands on the
    // ledger net of the ledger fee.
    seller_client
        .withdraw(WithdrawRequest {
            token_id: setup.quote_token_id(),
            amount: Nat::from(quote_notional),
        })
        .await
        .expect("seller quote withdrawal should succeed");
    buyer_client
        .withdraw(WithdrawRequest {
            token_id: setup.base_token_id(),
            amount: Nat::from(buyer_base_received),
        })
        .await
        .expect("buyer base withdrawal should succeed");

    assert_eq!(
        seller_client
            .get_balance(setup.quote_token_id())
            .await
            .unwrap(),
        expected_balance(0)
    );
    assert_eq!(
        buyer_client
            .get_balance(setup.base_token_id())
            .await
            .unwrap(),
        expected_balance(0)
    );
    assert_eq!(
        setup.quote_token_ledger().icrc1_balance_of(seller).await,
        Nat::from(quote_notional - quote_ledger_fee)
    );
    assert_eq!(
        setup.base_token_ledger().icrc1_balance_of(buyer).await,
        Nat::from(buyer_base_received - base_ledger_fee)
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_withdraw_on_negative_cases() {
    let setup = Setup::new().await.with_trading_pair().await;
    let cksol = setup.base_token_id();
    let fee = Nat::from(BASE_LEDGER_FEE);

    // --- Unsupported token: token not part of any trading pair ---
    {
        let unknown_token = TokenId {
            ledger_id: Principal::from_slice(&[0xFF]),
        };
        let result = setup
            .oisy_trade_client_with_caller(Principal::from_slice(&[0x0F]))
            .withdraw(WithdrawRequest {
                token_id: unknown_token.clone(),
                amount: Nat::from(1_000_000u64),
            })
            .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::UnsupportedToken {
                token_id: unknown_token,
            }))
        );
    }

    // --- Zero balance: withdraw should fail with InsufficientBalance ---
    {
        let user = Principal::from_slice(&[0x10]);
        let client = setup.oisy_trade_client_with_caller(user);

        // Withdraw an amount larger than the fee so the AmountTooSmall check passes,
        // and the balance check is reached.
        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(1_000_000u64),
            })
            .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::InsufficientBalance {
                available: Nat::from(0u64),
            }))
        );
    }

    // --- Insufficient balance: withdraw more than deposited ---
    {
        let user = Principal::from_slice(&[0x11]);
        let client = setup.oisy_trade_client_with_caller(user);

        let deposit_amount = 1_000_000u64;
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount + BASE_LEDGER_FEE)
            .deposit(deposit_amount)
            .execute()
            .await;

        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(2_000_000u64),
            })
            .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::InsufficientBalance {
                available: Nat::from(deposit_amount),
            }))
        );

        assert_eq!(
            client.get_balance(cksol.clone()).await.unwrap(),
            expected_balance(deposit_amount)
        );
    }

    // --- Amount too small: withdraw exactly the fee ---
    {
        let user = Principal::from_slice(&[0x12]);
        let client = setup.oisy_trade_client_with_caller(user);

        let deposit_amount = fee.clone();
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount.clone() + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount.clone() + BASE_LEDGER_FEE)
            .deposit(deposit_amount.clone())
            .execute()
            .await;

        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: fee.clone(),
            })
            .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::AmountTooSmall {
                min_amount: fee.clone() + 1u64,
            }))
        );
    }

    // --- Reserved balance: all funds locked in an open order ---
    {
        let user = Principal::from_slice(&[0x13]);
        let client = setup.oisy_trade_client_with_caller(user);

        let deposit_amount = 1_000_000u64;
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount + BASE_LEDGER_FEE)
            .deposit(deposit_amount)
            .execute()
            .await;

        client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Sell,
                price: Nat::from(10_000 * PRICE_SCALE),
                quantity: Nat::from(deposit_amount),
                time_in_force: None,
            })
            .await
            .unwrap();

        assert_eq!(
            client.get_balance(cksol.clone()).await.unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: deposit_amount.into(),
            }
        );

        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(deposit_amount),
            })
            .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::InsufficientBalance {
                available: Nat::from(0u64),
            }))
        );

        assert_eq!(
            client.get_balance(cksol.clone()).await.unwrap(),
            Balance {
                free: 0u64.into(),
                reserved: deposit_amount.into(),
            }
        );
    }

    // --- Ledger stopped (must be last since it alters global state) ---
    {
        let user = Principal::from_slice(&[0x14]);

        let deposit_amount = 1_000_000u64;
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount + BASE_LEDGER_FEE)
            .deposit(deposit_amount)
            .execute()
            .await;

        setup
            .env()
            .stop_canister(setup.base_ledger_id(), Some(setup.controller()))
            .await
            .expect("Failed to stop canister");

        let result = setup
            .oisy_trade_client_with_caller(user)
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(500_000u64),
            })
            .await;

        assert_matches!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(WithdrawTemporaryError::CallFailed { reason, .. }))
                if reason.contains("is stopped")
        );

        assert_eq!(
            setup
                .oisy_trade_client_with_caller(user)
                .get_balance(cksol)
                .await
                .unwrap(),
            expected_balance(deposit_amount)
        );
    }

    setup.drop().await;
}

#[tokio::test]
async fn should_get_events() {
    let setup = Setup::new().await;

    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 1);
        assert_matches!(
            events[0],
            oisy_trade_types_internal::event::EventType::Init(_)
        );
    });

    setup.drop().await;
}

#[tokio::test]
async fn should_get_logs() {
    let setup = Setup::new().await;

    let logs = setup.retrieve_logs(&Priority::Info).await;

    assert_eq!(logs.len(), 1);
    assert!(logs[0].message.contains("[init]"));

    setup.drop().await;
}

#[tokio::test]
async fn should_get_dashboard() {
    let setup = Setup::new().await.with_trading_pair().await;

    let body = setup.fetch_dashboard().await;

    assert!(
        body.contains("OISY TRADE Dashboard"),
        "missing title in: {body}"
    );
    assert!(
        body.contains(&setup.oisy_trade_id().to_string()),
        "missing canister id in: {body}",
    );
    assert!(
        body.contains("ckSOL"),
        "missing base token symbol in: {body}"
    );
    assert!(
        body.contains("ckBTC"),
        "missing quote token symbol in: {body}",
    );
    assert!(
        body.contains("Trading pairs"),
        "missing Trading pairs section in: {body}",
    );
    assert!(
        body.contains("ckSOL/ckBTC"),
        "missing pair label in: {body}",
    );
    assert!(
        body.contains("Order book is empty."),
        "missing empty-book message for empty pair in: {body}",
    );

    setup.drop().await;
}

mod order_book {
    use candid::{Nat, Principal};
    use oisy_trade_int_tests::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
    use oisy_trade_int_tests::{PRICE_SCALE, Setup};
    use oisy_trade_types::{
        GetOrderBookDepthError, GetOrderBookDepthRequest, GetOrderBookDepthRequestError,
        GetOrderBookTickerError, GetOrderBookTickerRequestError, LimitOrderRequest,
        MAX_DEPTH_LIMIT, OrderBookDepth, OrderBookTicker, PriceLevel, Side, TradingPair,
    };

    #[tokio::test]
    async fn should_expose_top_of_book_and_aggregated_depth() {
        let setup = Setup::new().await.with_trading_pair().await;

        // Two buyers at the best bid, a third buyer one level lower; two sellers
        // at the best ask, a third one level higher. The best-bid level
        // aggregates the quantity across the two top buyers.
        let u1 = Principal::from_slice(&[0x01]);
        let u2 = Principal::from_slice(&[0x02]);
        let u3 = Principal::from_slice(&[0x03]);
        let u4 = Principal::from_slice(&[0x04]);
        let u5 = Principal::from_slice(&[0x05]);
        let u6 = Principal::from_slice(&[0x06]);

        fund_and_place_buy(&setup, u1, 100_000 * PRICE_SCALE, 1_000_000).await;
        fund_and_place_buy(&setup, u2, 100_000 * PRICE_SCALE, 3_000_000).await;
        fund_and_place_buy(&setup, u3, 90_000 * PRICE_SCALE, 2_000_000).await;
        fund_and_place_sell(&setup, u4, 110_000 * PRICE_SCALE, 2_000_000).await;
        fund_and_place_sell(&setup, u5, 110_000 * PRICE_SCALE, 5_000_000).await;
        fund_and_place_sell(&setup, u6, 120_000 * PRICE_SCALE, 4_000_000).await;

        // Let all matching timers drain.
        setup.env().tick().await;

        let pair = setup.trading_pair();
        let client = setup.oisy_trade_client();

        assert_eq!(
            client.get_order_book_ticker(pair).await,
            Ok(OrderBookTicker {
                bid: Some(level(100_000 * PRICE_SCALE, 4_000_000)),
                ask: Some(level(110_000 * PRICE_SCALE, 7_000_000)),
            })
        );
        assert_eq!(
            client
                .get_order_book_depth(GetOrderBookDepthRequest {
                    trading_pair: pair,
                    limit: None,
                })
                .await,
            Ok(OrderBookDepth {
                bids: vec![
                    level(100_000 * PRICE_SCALE, 4_000_000),
                    level(90_000 * PRICE_SCALE, 2_000_000),
                ],
                asks: vec![
                    level(110_000 * PRICE_SCALE, 7_000_000),
                    level(120_000 * PRICE_SCALE, 4_000_000),
                ],
            })
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_report_unknown_trading_pair_over_the_boundary() {
        let setup = Setup::new().await.with_trading_pair().await;
        let unknown = TradingPair {
            base: Principal::from_slice(&[0xaa]),
            quote: Principal::from_slice(&[0xbb]),
        };
        let client = setup.oisy_trade_client();

        let ticker_error = client.get_order_book_ticker(unknown).await.unwrap_err();
        assert_eq!(
            ticker_error,
            GetOrderBookTickerError::request(GetOrderBookTickerRequestError::UnknownTradingPair),
        );

        let depth_error = client
            .get_order_book_depth(GetOrderBookDepthRequest {
                trading_pair: unknown,
                limit: None,
            })
            .await
            .unwrap_err();
        assert_eq!(
            depth_error,
            GetOrderBookDepthError::request(GetOrderBookDepthRequestError::UnknownTradingPair),
        );

        let limit_too_large_error = client
            .get_order_book_depth(GetOrderBookDepthRequest {
                trading_pair: setup.trading_pair(),
                limit: Some(MAX_DEPTH_LIMIT + 1),
            })
            .await
            .unwrap_err();
        assert_eq!(
            limit_too_large_error,
            GetOrderBookDepthError::request(GetOrderBookDepthRequestError::LimitTooLarge {
                requested: MAX_DEPTH_LIMIT + 1,
                max: MAX_DEPTH_LIMIT,
            }),
        );

        setup.drop().await;
    }

    async fn fund_and_place_buy(setup: &Setup, user: Principal, price: u64, quantity: u64) {
        // Settlement is `price × quantity / 10^base_decimals` (ckSOL base = 9
        // decimals). Compute in u128 to avoid overflow at the scaled prices.
        let required = u64::try_from(price as u128 * quantity as u128 / 1_000_000_000)
            .expect("required quote amount exceeds u64");
        setup
            .deposit_flow(user, setup.quote_token_id())
            .mint(required + 2 * QUOTE_LEDGER_FEE)
            .approve(required + QUOTE_LEDGER_FEE)
            .deposit(required)
            .execute()
            .await;
        setup
            .oisy_trade_client_with_caller(user)
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .unwrap();
    }

    async fn fund_and_place_sell(setup: &Setup, user: Principal, price: u64, quantity: u64) {
        let required = quantity;
        setup
            .deposit_flow(user, setup.base_token_id())
            .mint(required + 2 * BASE_LEDGER_FEE)
            .approve(required + BASE_LEDGER_FEE)
            .deposit(required)
            .execute()
            .await;
        setup
            .oisy_trade_client_with_caller(user)
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Sell,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .unwrap();
    }

    fn level(price: u64, quantity: u64) -> PriceLevel {
        PriceLevel {
            price: Nat::from(price),
            quantity: Nat::from(quantity),
        }
    }
}

mod chunked_matching {
    use candid::{Nat, Principal};
    use oisy_trade_int_tests::Setup;
    use oisy_trade_int_tests::icrc_ledger::QUOTE_LEDGER_FEE;
    use oisy_trade_types::{GetOrderBookDepthRequest, LimitOrderRequest, Side};
    use oisy_trade_types_internal::{InitArg, Mode};

    const MAX_ORDERS_PER_CHUNK: u32 = 5;
    const N_ORDERS: u32 = MAX_ORDERS_PER_CHUNK + 1; // forces ≥ 2 chunks
    const PRICE: u64 = 1000;
    const QUANTITY: u64 = 1_000_000;

    /// Installs the canister with a tiny `ExecutionPolicy` (5 orders per
    /// chunk), submits 6 non-crossing orders in a single PocketIC round,
    /// and verifies that the chunked-matching pipeline drains the backlog
    /// and produces at least two `MatchingEvent`s (one per chunk) —
    /// proving the work actually splits rather than being absorbed by a
    /// single oversized chunk.
    #[tokio::test]
    async fn should_drain_pending_orders_across_chunks() {
        let (setup, _user) = install_with_chunked_buy_workload().await;

        tick_until_depth_reaches(&setup, expected_resting_quantity(), MAX_TICKS).await;
        assert_matching_events_at_least(&setup, 2).await;

        setup.drop().await;
    }

    /// Same workload as `should_drain_pending_orders_across_chunks`, but
    /// the canister is installed with an instruction budget so small that
    /// no chunk can make progress. Pending orders accumulate but never
    /// match. An upgrade then raises the budget; the post-upgrade timer
    /// drains the backlog with chunked matching.
    ///
    /// Stopping cleanly mid-chunk isn't reliable: every `add_limit_order`
    /// schedules its own zero-delay timer and PocketIC's `update_call`
    /// advances enough rounds to fire them, so a tight workload can drain
    /// inside the placement phase (see DEFI-2823 for the timer-coalescing
    /// follow-up). Starving the budget instead lets the kickoff timers
    /// fire harmlessly and gives us a deterministic pending-only state to
    /// upgrade across.
    #[tokio::test]
    async fn should_drain_pending_orders_across_upgrade() {
        let setup = Setup::builder()
            .with_init_arg(InitArg {
                mode: Mode::GeneralAvailability,
                max_orders_per_chunk: MAX_ORDERS_PER_CHUNK,
                instruction_budget: 1, // intentionally too small to make progress
            })
            .build()
            .await
            .with_trading_pair()
            .await;

        let user = Principal::from_slice(&[0x43]);
        place_n_buy_orders(&setup, user).await;

        // No matching could progress under the starved budget, so all
        // orders are still pending and depth is 0.
        assert_eq!(depth_quantity(&setup).await, Nat::from(0u64));

        setup
            .upgrade(Some(oisy_trade_types_internal::UpgradeArg {
                mode: None,
                max_orders_per_chunk: None,
                instruction_budget: Some(1_000_000_000),
            }))
            .await;

        // The original kickoff timers were lost across the upgrade; advance
        // past the periodic matching interval (1 min) so the heartbeat
        // matching timer fires and drains the backlog.
        setup
            .env()
            .advance_time(std::time::Duration::from_secs(120))
            .await;

        tick_until_depth_reaches(&setup, expected_resting_quantity(), MAX_TICKS).await;
        assert_matching_events_at_least(&setup, 2).await;

        setup.drop().await;
    }

    const MAX_TICKS: u32 = 20;

    fn expected_resting_quantity() -> Nat {
        Nat::from(u64::from(N_ORDERS) * QUANTITY)
    }

    async fn install_with_chunked_buy_workload() -> (Setup, Principal) {
        let setup = Setup::builder()
            .with_init_arg(InitArg {
                mode: Mode::GeneralAvailability,
                max_orders_per_chunk: MAX_ORDERS_PER_CHUNK,
                instruction_budget: 1_000_000_000,
            })
            .build()
            .await
            .with_trading_pair()
            .await;

        let user = Principal::from_slice(&[0x42]);
        place_n_buy_orders(&setup, user).await;
        (setup, user)
    }

    async fn place_n_buy_orders(setup: &Setup, user: Principal) {
        let pair = setup.trading_pair();
        let per_order_cost = PRICE * QUANTITY;
        let total_cost = u64::from(N_ORDERS) * per_order_cost;
        setup
            .deposit_flow(user, setup.quote_token_id())
            .mint(total_cost + 2 * QUOTE_LEDGER_FEE)
            .approve(total_cost + QUOTE_LEDGER_FEE)
            .deposit(total_cost)
            .execute()
            .await;

        let client = setup.oisy_trade_client_with_caller(user);
        for _ in 0..N_ORDERS {
            client
                .add_limit_order(LimitOrderRequest {
                    pair,
                    side: Side::Buy,
                    price: Nat::from(PRICE),
                    quantity: Nat::from(QUANTITY),
                    time_in_force: None,
                })
                .await
                .unwrap();
        }
    }

    async fn depth_quantity(setup: &Setup) -> Nat {
        setup
            .oisy_trade_client()
            .get_order_book_depth(GetOrderBookDepthRequest {
                trading_pair: setup.trading_pair(),
                limit: None,
            })
            .await
            .unwrap()
            .bids
            .first()
            .map(|level| level.quantity.clone())
            .unwrap_or_else(|| Nat::from(0u64))
    }

    async fn tick_until_depth_reaches(setup: &Setup, expected: Nat, max_ticks: u32) {
        let mut ticks = 0;
        loop {
            setup.env().tick().await;
            ticks += 1;
            let resting = depth_quantity(setup).await;
            if resting == expected {
                break;
            }
            assert!(
                ticks <= max_ticks,
                "chunked matching failed to reach expected resting depth {expected} after {ticks} ticks (last seen {resting})",
            );
        }
    }

    async fn assert_matching_events_at_least(setup: &Setup, min: usize) {
        let matching_events = setup
            .get_all_events()
            .await
            .into_iter()
            .filter(|event| {
                matches!(
                    event.payload,
                    oisy_trade_types_internal::event::EventType::Matching(_),
                )
            })
            .count();
        assert!(
            matching_events >= min,
            "expected ≥ {min} MatchingEvents (chunk size {MAX_ORDERS_PER_CHUNK}, workload {N_ORDERS} orders), got {matching_events}",
        );
    }
}

#[tokio::test]
async fn should_expose_metrics() {
    let setup = Setup::new().await.with_trading_pair().await;

    setup
        .assert_metrics()
        .await
        .assert_contains_metric_matching("cycle_balance [\\d.eE+-]+")
        .assert_contains_metric_matching("stable_memory_bytes [\\d.eE+-]+")
        .assert_contains_metric_matching("event_total [\\d.eE+-]+")
        .assert_contains_metric_matching("trading_pair_count 1")
        .assert_contains_metric_matching(r#"pending_orders\{base="CKSOL",quote="CKBTC"\} 0"#)
        .assert_contains_metric_matching(r#"resting_orders\{base="CKSOL",quote="CKBTC"\} 0"#);

    let user = setup.user();
    let required = 1_000_000_000u64;
    setup
        .deposit_flow(user, setup.quote_token_id())
        .mint(required + 2 * QUOTE_LEDGER_FEE)
        .approve(required + QUOTE_LEDGER_FEE)
        .deposit(required)
        .execute()
        .await;
    setup
        .oisy_trade_client()
        .add_limit_order(LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Buy,
            price: Nat::from(10_000 * PRICE_SCALE),
            quantity: 1_000_000u64.into(),
            time_in_force: None,
        })
        .await
        .unwrap();
    setup
        .deposit_flow(user, setup.base_token_id())
        .mint(required + 2 * BASE_LEDGER_FEE)
        .approve(required + BASE_LEDGER_FEE)
        .deposit(1_000_000u64)
        .execute()
        .await;
    setup
        .oisy_trade_client()
        .add_limit_order(LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Sell,
            price: Nat::from(20_000 * PRICE_SCALE),
            quantity: 1_000_000u64.into(),
            time_in_force: None,
        })
        .await
        .unwrap();

    // Tick to let the matching timer fire and move the order from pending to open.
    setup.env().tick().await;

    setup
        .assert_metrics()
        .await
        .assert_contains_metric_matching(format!(
            r#"ask\{{base="CKSOL",quote="CKBTC"\}} {}"#,
            20_000 * PRICE_SCALE
        ))
        .assert_contains_metric_matching(format!(
            r#"bid\{{base="CKSOL",quote="CKBTC"\}} {}"#,
            10_000 * PRICE_SCALE
        ))
        .assert_contains_metric_matching(r#"pending_orders\{base="CKSOL",quote="CKBTC"\} 0"#)
        .assert_contains_metric_matching(r#"resting_orders\{base="CKSOL",quote="CKBTC"\} 2"#);

    setup.drop().await;
}

/// `/metrics` exposes a `fee_balance` gauge per token in whole token
/// units (raw amount ÷ 10^decimals) after a fee-charging fill.
#[tokio::test]
async fn should_expose_fee_balance_metric() {
    let (fills, setup) = fill_one_cross_with_fees().await;
    // `assert_contains_metric_matching` runs `regex::Regex::new(...).is_match(line)`,
    // so the dot in the formatted float must be escaped.
    setup
        .assert_metrics()
        .await
        .assert_contains_metric_matching(format!(
            r#"fee_balance\{{token="CKSOL"\}} {}"#,
            fills.base_fee_whole().replace('.', r"\.")
        ))
        .assert_contains_metric_matching(format!(
            r#"fee_balance\{{token="CKBTC"\}} {}"#,
            fills.quote_fee_whole().replace('.', r"\.")
        ));

    setup.drop().await;
}

mod get_fee_balances {
    use candid::Nat;
    use oisy_trade_int_tests::fill_one_cross_with_fees;
    use oisy_trade_types::{Balance, FilterToken, UserTokenBalance};

    /// Stand up a trading pair with non-zero maker/taker fees and run one
    /// cross so both sides accrue into the canister-owned fee pool. Asserts
    /// that `get_fee_balances` reports the accrued amounts.
    #[tokio::test]
    async fn should_report_accrued_fees_after_a_fill() {
        let (fills, setup) = fill_one_cross_with_fees().await;

        let no_filter = setup
            .oisy_trade_client()
            .get_fee_balances(None)
            .await
            .unwrap();
        assert_eq!(no_filter.len(), 2);

        let with_filter = setup
            .oisy_trade_client()
            .get_fee_balances(Some(vec![
                FilterToken::ById(fills.base.id.clone()),
                FilterToken::ById(fills.quote.id.clone()),
            ]))
            .await
            .unwrap();
        assert_eq!(
            with_filter,
            vec![
                UserTokenBalance {
                    token: fills.base.clone(),
                    balance: Balance {
                        free: Nat::from(fills.base_fee_raw),
                        reserved: Nat::from(0u64),
                    },
                },
                UserTokenBalance {
                    token: fills.quote.clone(),
                    balance: Balance {
                        free: Nat::from(fills.quote_fee_raw),
                        reserved: Nat::from(0u64),
                    },
                },
            ],
        );

        setup.drop().await;
    }
}

mod list_supported_tokens {
    use oisy_trade_int_tests::Setup;

    #[tokio::test]
    async fn should_return_empty_then_pair_tokens() {
        let setup = Setup::new().await;
        assert!(
            setup
                .oisy_trade_client()
                .list_supported_tokens()
                .await
                .is_empty()
        );

        let setup = setup.with_trading_pair().await;
        let request = setup.add_trading_pair_request();
        let tokens = setup.oisy_trade_client().list_supported_tokens().await;
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains(&request.base));
        assert!(tokens.contains(&request.quote));

        setup.drop().await;
    }
}

mod get_balances {
    use candid::{Nat, Principal};
    use oisy_trade_int_tests::Setup;
    use oisy_trade_int_tests::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
    use oisy_trade_types::{
        FilterToken, GetBalancesError, GetBalancesRequestError, MAX_FILTER_LEN, TokenId,
    };

    #[tokio::test]
    async fn should_return_empty_without_filter_for_fresh_user() {
        let setup = Setup::new().await.with_trading_pair().await;
        let result = setup.oisy_trade_client().get_balances(None).await.unwrap();
        assert!(result.is_empty());
        setup.drop().await;
    }

    #[tokio::test]
    async fn should_return_ok_for_registered_filter_entry() {
        let setup = Setup::new().await.with_trading_pair().await;
        let token = setup.base_token_id();
        let deposit = 1_000_000u64;
        setup
            .deposit_flow(setup.user(), token.clone())
            .mint(deposit + 2 * BASE_LEDGER_FEE)
            .approve(deposit + BASE_LEDGER_FEE)
            .deposit(deposit)
            .execute()
            .await;

        let result = setup
            .oisy_trade_client()
            .get_balances(Some(vec![FilterToken::ById(token.clone())]))
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        let entry = &result[0];
        assert_eq!(entry.token.id, token);
        assert_eq!(entry.balance.free, Nat::from(deposit));

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_fail_whole_call_for_unknown_filter_entry() {
        let setup = Setup::new().await.with_trading_pair().await;
        let client = setup.oisy_trade_client();

        let unknown = TokenId {
            ledger_id: Principal::from_slice(&[0xFF]),
        };
        let unknown_entry_error = client
            .get_balances(Some(vec![FilterToken::ById(unknown.clone())]))
            .await
            .unwrap_err();
        assert_eq!(
            unknown_entry_error,
            GetBalancesError::request(GetBalancesRequestError::TokenNotSupported(
                FilterToken::ById(unknown)
            )),
        );

        let len = MAX_FILTER_LEN + 1;
        let filter = vec![FilterToken::ById(setup.base_token_id()); len as usize];
        let filter_too_large_error = client.get_balances(Some(filter)).await.unwrap_err();
        assert_eq!(
            filter_too_large_error,
            GetBalancesError::request(GetBalancesRequestError::FilterTooLarge {
                len,
                max: MAX_FILTER_LEN,
            }),
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_match_full_filter_to_no_filter_when_user_holds_every_supported_token() {
        let setup = Setup::new().await.with_trading_pair().await;
        let base = setup.base_token_id();
        let quote = setup.quote_token_id();
        setup
            .deposit_flow(setup.user(), base.clone())
            .mint(1_000_000u64 + 2 * BASE_LEDGER_FEE)
            .approve(1_000_000u64 + BASE_LEDGER_FEE)
            .deposit(1_000_000u64)
            .execute()
            .await;
        setup
            .deposit_flow(setup.user(), quote.clone())
            .mint(500_000u64 + 2 * QUOTE_LEDGER_FEE)
            .approve(500_000u64 + QUOTE_LEDGER_FEE)
            .deposit(500_000u64)
            .execute()
            .await;

        let supported = setup.oisy_trade_client().list_supported_tokens().await;
        let full_filter: Vec<FilterToken> = supported
            .iter()
            .map(|t| FilterToken::ById(t.id.clone()))
            .collect();

        let no_filter = setup.oisy_trade_client().get_balances(None).await.unwrap();
        let with_full_filter = setup
            .oisy_trade_client()
            .get_balances(Some(full_filter))
            .await
            .unwrap();

        assert_eq!(no_filter, with_full_filter);

        setup.drop().await;
    }
}

mod halt {
    use assert_matches::assert_matches;
    use candid::{Encode, Nat, Principal};
    use oisy_trade_int_tests::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
    use oisy_trade_int_tests::{PRICE_SCALE, Setup};
    use oisy_trade_types::{
        AddLimitOrderTemporaryError, Balance, ErrorKind, LimitOrderRequest, OrderStatus, Side,
        TradingPair, TradingStatus, UnauthorizedError, WithdrawRequest,
    };
    use pocket_ic::{RejectCode, RejectResponse};

    /// Whether a pair is halted by the global flag or by a per-pair halt. Every
    /// shared flow below runs against both so the common behavior is asserted
    /// once per mode.
    #[derive(Copy, Clone, Debug)]
    enum HaltMode {
        Global,
        Pair,
    }

    impl HaltMode {
        /// The `halt_trading` / `resume_trading` argument that halts (or
        /// resumes) `pair` under this mode.
        fn arg(self, pair: TradingPair) -> Option<Vec<TradingPair>> {
            match self {
                HaltMode::Global => None,
                HaltMode::Pair => Some(vec![pair]),
            }
        }
    }

    const MODES: [HaltMode; 2] = [HaltMode::Global, HaltMode::Pair];

    /// End-to-end halt lifecycle on a crossable buy/sell pair placed before the
    /// halt, run once per [`HaltMode`]:
    ///
    /// 1. buyer and seller each fund and place one resting order that crosses;
    /// 2. trading halts;
    /// 3. the orders keep the exact status they had before the halt (no
    ///    transition is driven while halted);
    /// 4. balances stay fully reserved — no partial fill slips through;
    /// 5. `resume_trading` re-arms matching from the endpoint, so the cross
    ///    fills without advancing time past the periodic matching interval and
    ///    without placing a new order.
    #[tokio::test]
    async fn should_freeze_orders_under_halt_then_fill_them_on_resume() {
        for mode in MODES {
            let setup = Setup::new().await.with_trading_pair().await;
            let pair = setup.trading_pair();
            let buyer = Principal::from_slice(&[0x01]);
            let buyer_client = setup.oisy_trade_client_with_caller(buyer);
            let seller = Principal::from_slice(&[0x02]);
            let seller_client = setup.oisy_trade_client_with_caller(seller);
            let controller_client = setup.oisy_trade_client_with_caller(setup.controller());

            // 10_000 ckBTC per whole ckSOL (10_000 * PRICE_SCALE), 1M base
            // units. Reserve = price * quantity / 10^9 = 1_000_000_000 quote
            // units.
            let price = 10_000 * PRICE_SCALE;
            let quantity = 1_000_000u64;
            let required_quote = 1_000_000_000u64;
            let required_base = quantity;

            // Buyer places a buy, seller a crossing sell, while trading is
            // active. No tick runs in between, so neither placement kickoff has
            // matched yet.
            setup
                .deposit_flow(buyer, setup.quote_token_id())
                .mint(required_quote + 2 * QUOTE_LEDGER_FEE)
                .approve(required_quote + QUOTE_LEDGER_FEE)
                .deposit(required_quote)
                .execute()
                .await;
            let buy_id = buyer_client
                .add_limit_order(LimitOrderRequest {
                    pair,
                    side: Side::Buy,
                    price: Nat::from(price),
                    quantity: Nat::from(quantity),
                    time_in_force: None,
                })
                .await
                .unwrap();
            setup
                .deposit_flow(seller, setup.base_token_id())
                .mint(required_base + 2 * BASE_LEDGER_FEE)
                .approve(required_base + BASE_LEDGER_FEE)
                .deposit(required_base)
                .execute()
                .await;
            let sell_id = seller_client
                .add_limit_order(LimitOrderRequest {
                    pair,
                    side: Side::Sell,
                    price: Nat::from(price),
                    quantity: Nat::from(quantity),
                    time_in_force: None,
                })
                .await
                .unwrap();

            // Before the halt the pair reports as trading.
            assert_eq!(
                setup.pair_status(pair).await,
                TradingStatus::Trading,
                "{mode:?}: pair must report Trading before the halt"
            );

            // Halt right after placement — before any round runs the placement
            // kickoffs — so the cross stays unmatched under the halt.
            assert_eq!(controller_client.halt_trading(mode.arg(pair)).await, Ok(()));

            // The halt is reflected on the pair's trading status.
            assert_eq!(
                setup.pair_status(pair).await,
                TradingStatus::Halted,
                "{mode:?}: pair must report Halted while halted"
            );

            // The orders are open or pending under the halt; capture that status
            // as the baseline and require it to be preserved across the matching
            // ticks below.
            let buy_status_under_halt = buyer_client
                .get_my_order(buy_id.clone())
                .await
                .unwrap()
                .order
                .status;
            let sell_status_under_halt = seller_client
                .get_my_order(sell_id.clone())
                .await
                .unwrap()
                .order
                .status;
            assert_matches!(
                buy_status_under_halt,
                OrderStatus::Open | OrderStatus::Pending
            );
            assert_matches!(
                sell_status_under_halt,
                OrderStatus::Open | OrderStatus::Pending
            );

            // Advance past the matching interval and tick: matching must make no
            // progress.
            setup
                .env()
                .advance_time(std::time::Duration::from_secs(120))
                .await;
            for _ in 0..3 {
                setup.env().tick().await;
            }

            // The orders keep the exact status they had when the halt took
            // effect.
            assert_eq!(
                buyer_client
                    .get_my_order(buy_id.clone())
                    .await
                    .unwrap()
                    .order
                    .status,
                buy_status_under_halt,
                "{mode:?}: buy status must be unchanged while halted"
            );
            assert_eq!(
                seller_client
                    .get_my_order(sell_id.clone())
                    .await
                    .unwrap()
                    .order
                    .status,
                sell_status_under_halt,
                "{mode:?}: sell status must be unchanged while halted"
            );
            // `OrderStatus` cannot express a partial fill, so pin
            // `filled_quantity` too: neither side has matched any quantity while
            // halted.
            assert_eq!(
                buyer_client
                    .get_my_order(buy_id.clone())
                    .await
                    .unwrap()
                    .order
                    .filled_quantity,
                Nat::from(0u64),
                "{mode:?}: buy must have no partial fill while halted"
            );
            assert_eq!(
                seller_client
                    .get_my_order(sell_id.clone())
                    .await
                    .unwrap()
                    .order
                    .filled_quantity,
                Nat::from(0u64),
                "{mode:?}: sell must have no partial fill while halted"
            );

            // Resume and tick WITHOUT advancing time and WITHOUT placing a new
            // order: the resume kickoff alone re-arms matching and drives the
            // fill.
            assert_eq!(
                controller_client.resume_trading(mode.arg(pair)).await,
                Ok(())
            );

            // The resume is reflected on the pair's trading status.
            assert_eq!(
                setup.pair_status(pair).await,
                TradingStatus::Trading,
                "{mode:?}: pair must report Trading again after resume"
            );
            for _ in 0..3 {
                setup.env().tick().await;
            }
            assert_eq!(
                buyer_client
                    .get_my_order(buy_id)
                    .await
                    .unwrap()
                    .order
                    .status,
                OrderStatus::Filled,
                "{mode:?}: buy fills from the resume kickoff"
            );
            assert_eq!(
                seller_client
                    .get_my_order(sell_id)
                    .await
                    .unwrap()
                    .order
                    .status,
                OrderStatus::Filled,
                "{mode:?}: sell fills from the resume kickoff"
            );
            assert_eq!(
                buyer_client
                    .get_balance(setup.base_token_id())
                    .await
                    .unwrap(),
                Balance {
                    free: required_base.into(),
                    reserved: 0u64.into()
                },
            );

            setup.drop().await;
        }
    }

    /// While trading is halted (globally or per-pair), the halt itself blocks
    /// only new orders: `add_limit_order` is rejected with `TradingHalted`, but
    /// a resting order placed pre-halt still cancels and a withdrawal of
    /// available balance still succeeds. Run once per [`HaltMode`].
    #[tokio::test]
    async fn should_block_new_orders_but_allow_cancel_and_withdraw_under_halt() {
        for mode in MODES {
            let setup = Setup::new().await.with_trading_pair().await;
            let pair = setup.trading_pair();
            let user = setup.user();
            let client = setup.oisy_trade_client();
            let controller_client = setup.oisy_trade_client_with_caller(setup.controller());
            let quote = setup.quote_token_id();

            // Fund the user with enough quote to place a resting buy order and
            // keep a free balance to withdraw.
            let price = 1000u64;
            let quantity = 1_000_000u64;
            let order_cost = price * quantity;
            let free_to_withdraw = 10_000_000u64;
            let deposit_amount = order_cost + free_to_withdraw;
            setup
                .deposit_flow(user, quote.clone())
                .mint(deposit_amount + 2 * QUOTE_LEDGER_FEE)
                .approve(deposit_amount + QUOTE_LEDGER_FEE)
                .deposit(deposit_amount)
                .execute()
                .await;

            let order = LimitOrderRequest {
                pair,
                side: Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            };

            // Place a resting buy order before the halt.
            let resting_id = client.add_limit_order(order.clone()).await.unwrap();
            setup.env().tick().await;
            assert_eq!(
                client
                    .get_my_order(resting_id.clone())
                    .await
                    .unwrap()
                    .order
                    .status,
                OrderStatus::Open
            );

            // Halt trading.
            assert_eq!(controller_client.halt_trading(mode.arg(pair)).await, Ok(()));

            // New orders are rejected.
            assert_eq!(
                client
                    .add_limit_order(order.clone())
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::TemporaryError(Some(AddLimitOrderTemporaryError::TradingHalted)),
                "{mode:?}: new orders must be rejected while halted"
            );

            // The resting order can still be canceled.
            let canceled = client.cancel_limit_order(resting_id.clone()).await.unwrap();
            assert_matches!(canceled.status, OrderStatus::Canceled);

            // A withdrawal of available balance still succeeds.
            client
                .withdraw(WithdrawRequest {
                    token_id: quote.clone(),
                    amount: Nat::from(free_to_withdraw),
                })
                .await
                .expect("withdrawal should succeed while halted");

            setup.drop().await;
        }
    }

    /// `halt_trading` and `resume_trading` reject non-controller callers with
    /// `NotController`, in both global and per-pair form.
    #[tokio::test]
    async fn should_reject_non_controller_callers() {
        for mode in MODES {
            let setup = Setup::new().await.with_trading_pair().await;
            let pair = setup.trading_pair();
            let user_client = setup.oisy_trade_client_with_caller(Principal::from_slice(&[0x01]));

            assert_eq!(
                user_client.halt_trading(mode.arg(pair)).await,
                Err(UnauthorizedError::NotController),
                "{mode:?}: non-controller halt must be rejected"
            );
            assert_eq!(
                user_client.resume_trading(mode.arg(pair)).await,
                Err(UnauthorizedError::NotController),
                "{mode:?}: non-controller resume must be rejected"
            );

            setup.drop().await;
        }
    }

    /// The halt state is part of the upgrade snapshot: after halting and
    /// upgrading, new orders remain rejected until trading is resumed. Run once
    /// per [`HaltMode`].
    #[tokio::test]
    async fn should_preserve_halt_state_across_upgrade() {
        for mode in MODES {
            let setup = Setup::new().await.with_trading_pair().await;
            let pair = setup.trading_pair();
            let user = setup.user();
            let client = setup.oisy_trade_client();
            let controller_client = setup.oisy_trade_client_with_caller(setup.controller());
            let quote = setup.quote_token_id();

            let price = 1000u64;
            let quantity = 1_000_000u64;
            let order_cost = price * quantity;
            setup
                .deposit_flow(user, quote.clone())
                .mint(order_cost + 2 * QUOTE_LEDGER_FEE)
                .approve(order_cost + QUOTE_LEDGER_FEE)
                .deposit(order_cost)
                .execute()
                .await;

            assert_eq!(controller_client.halt_trading(mode.arg(pair)).await, Ok(()));
            setup.upgrade(None).await;

            let order = LimitOrderRequest {
                pair,
                side: Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            };
            assert_eq!(
                client
                    .add_limit_order(order.clone())
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::TemporaryError(Some(AddLimitOrderTemporaryError::TradingHalted)),
                "{mode:?}: halt must survive the upgrade"
            );

            assert_eq!(
                controller_client.resume_trading(mode.arg(pair)).await,
                Ok(())
            );
            client
                .add_limit_order(order)
                .await
                .expect("orders accepted after resume");

            setup.drop().await;
        }
    }

    /// A per-pair halt blocks only the halted pair: orders on the other pair
    /// still succeed and match. Unique to the per-pair mode.
    #[tokio::test]
    async fn should_block_orders_on_halted_pair_only() {
        let setup = Setup::new()
            .await
            .with_trading_pair()
            .await
            .with_second_trading_pair()
            .await;
        let pair_a = setup.trading_pair();
        let pair_b = setup.second_trading_pair();
        let user = setup.user();
        let client = setup.oisy_trade_client();
        let controller_client = setup.oisy_trade_client_with_caller(setup.controller());

        let price = 1000u64;
        let quantity = 1_000_000u64;
        let order_cost = price * quantity;

        // The Buy on pair A reserves quote; pair B reuses the two ledgers with
        // base/quote swapped, so the Buy on pair B reserves the base ledger's
        // token. Fund both so each order is gated by the halt, not by balance.
        setup
            .deposit_flow(user, setup.quote_token_id())
            .mint(order_cost + 2 * QUOTE_LEDGER_FEE)
            .approve(order_cost + QUOTE_LEDGER_FEE)
            .deposit(order_cost)
            .execute()
            .await;
        setup
            .deposit_flow(user, setup.base_token_id())
            .mint(order_cost + 2 * BASE_LEDGER_FEE)
            .approve(order_cost + BASE_LEDGER_FEE)
            .deposit(order_cost)
            .execute()
            .await;

        // Halt pair A.
        assert_eq!(
            controller_client.halt_trading(Some(vec![pair_a])).await,
            Ok(())
        );

        // Only pair A reports as halted; pair B keeps trading.
        assert_eq!(
            setup.pair_status(pair_a).await,
            TradingStatus::Halted,
            "halted pair must report Halted"
        );
        assert_eq!(
            setup.pair_status(pair_b).await,
            TradingStatus::Trading,
            "unaffected pair must keep reporting Trading"
        );

        // New orders on pair A are rejected.
        assert_eq!(
            client
                .add_limit_order(LimitOrderRequest {
                    pair: pair_a,
                    side: Side::Buy,
                    price: Nat::from(price),
                    quantity: Nat::from(quantity),
                    time_in_force: None,
                })
                .await
                .unwrap_err()
                .kind,
            ErrorKind::TemporaryError(Some(AddLimitOrderTemporaryError::TradingHalted))
        );

        // Orders on pair B still succeed.
        let order_b = client
            .add_limit_order(LimitOrderRequest {
                pair: pair_b,
                side: Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .unwrap();
        setup.env().tick().await;
        assert_eq!(
            client.get_my_order(order_b).await.unwrap().order.status,
            OrderStatus::Open,
            "orders on the unaffected pair are accepted and rest in the book"
        );

        setup.drop().await;
    }

    /// Resting crossable orders on a halted pair do not fill after the timer
    /// ticks; unhalting lets them fill; meanwhile a cross on the other pair
    /// fills throughout. Unique to the per-pair mode.
    #[tokio::test]
    async fn should_stop_matching_on_halted_pair_only() {
        let setup = Setup::new()
            .await
            .with_trading_pair()
            .await
            .with_second_trading_pair()
            .await;
        let pair_a = setup.trading_pair();
        let pair_b = setup.second_trading_pair();
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.oisy_trade_client_with_caller(buyer);
        let seller = Principal::from_slice(&[0x02]);
        let seller_client = setup.oisy_trade_client_with_caller(seller);
        let controller_client = setup.oisy_trade_client_with_caller(setup.controller());

        let price = 1000u64;
        let quantity = 1_000_000u64;
        let required_base = quantity;

        // Pair A (base ckSOL/9 dec, quote ckBTC): buy reserves ckBTC, sell
        // reserves ckSOL. Pair B is the swapped pair (base ckBTC/8 dec, quote
        // ckSOL): buy reserves ckSOL, sell reserves ckBTC. A buy reserves
        // `price * quantity / 10^base_decimals` of the quote token, so the
        // buyer needs a quote-worth of *each* token, and the seller needs a
        // base-worth of *each* token. `quote_token_id()` is ckBTC;
        // `base_token_id()` is ckSOL.
        let required_quote_a = price * quantity / 10u64.pow(9);
        let required_quote_b = price * quantity / 10u64.pow(8);
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(required_quote_a + 2 * QUOTE_LEDGER_FEE)
            .approve(required_quote_a + QUOTE_LEDGER_FEE)
            .deposit(required_quote_a)
            .execute()
            .await;
        setup
            .deposit_flow(buyer, setup.base_token_id())
            .mint(required_quote_b + 2 * BASE_LEDGER_FEE)
            .approve(required_quote_b + BASE_LEDGER_FEE)
            .deposit(required_quote_b)
            .execute()
            .await;
        setup
            .deposit_flow(seller, setup.base_token_id())
            .mint(required_base + 2 * BASE_LEDGER_FEE)
            .approve(required_base + BASE_LEDGER_FEE)
            .deposit(required_base)
            .execute()
            .await;
        setup
            .deposit_flow(seller, setup.quote_token_id())
            .mint(required_base + 2 * QUOTE_LEDGER_FEE)
            .approve(required_base + QUOTE_LEDGER_FEE)
            .deposit(required_base)
            .execute()
            .await;

        // Halt pair A first.
        assert_eq!(
            controller_client.halt_trading(Some(vec![pair_a])).await,
            Ok(())
        );

        // Pair B cross (matches freely).
        let buy_b = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: pair_b,
                side: Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .unwrap();
        let sell_b = seller_client
            .add_limit_order(LimitOrderRequest {
                pair: pair_b,
                side: Side::Sell,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .unwrap();

        // Pair A cross: orders are rejected while halted, so resume A just long
        // enough to place both, then halt again before ticking.
        assert_eq!(
            controller_client.resume_trading(Some(vec![pair_a])).await,
            Ok(())
        );
        let buy_a = buyer_client
            .add_limit_order(LimitOrderRequest {
                pair: pair_a,
                side: Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .unwrap();
        let sell_a = seller_client
            .add_limit_order(LimitOrderRequest {
                pair: pair_a,
                side: Side::Sell,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .unwrap();
        assert_eq!(
            controller_client.halt_trading(Some(vec![pair_a])).await,
            Ok(())
        );

        setup
            .env()
            .advance_time(std::time::Duration::from_secs(120))
            .await;
        for _ in 0..3 {
            setup.env().tick().await;
        }

        // Pair A's cross has not filled; pair B's has.
        assert_matches!(
            buyer_client
                .get_my_order(buy_a.clone())
                .await
                .unwrap()
                .order
                .status,
            OrderStatus::Pending | OrderStatus::Open,
            "halted pair's buy must not fill"
        );
        assert_matches!(
            seller_client
                .get_my_order(sell_a.clone())
                .await
                .unwrap()
                .order
                .status,
            OrderStatus::Pending | OrderStatus::Open,
            "halted pair's sell must not fill"
        );
        assert_eq!(
            buyer_client.get_my_order(buy_b).await.unwrap().order.status,
            OrderStatus::Filled,
            "unaffected pair's buy must fill"
        );
        assert_eq!(
            seller_client
                .get_my_order(sell_b)
                .await
                .unwrap()
                .order
                .status,
            OrderStatus::Filled,
            "unaffected pair's sell must fill"
        );

        // Unhalt pair A and let the timer fire: its cross now fills.
        assert_eq!(
            controller_client.resume_trading(Some(vec![pair_a])).await,
            Ok(())
        );
        setup
            .env()
            .advance_time(std::time::Duration::from_secs(120))
            .await;
        for _ in 0..3 {
            setup.env().tick().await;
        }
        assert_eq!(
            buyer_client.get_my_order(buy_a).await.unwrap().order.status,
            OrderStatus::Filled
        );
        assert_eq!(
            seller_client
                .get_my_order(sell_a)
                .await
                .unwrap()
                .order
                .status,
            OrderStatus::Filled
        );

        setup.drop().await;
    }

    /// A per-pair `halt_trading` traps for an unregistered pair, recording
    /// nothing. Unique to the per-pair mode.
    #[tokio::test]
    async fn should_trap_on_unknown_pair() {
        let setup = Setup::new().await.with_trading_pair().await;

        // A controller targeting an unregistered pair traps before recording
        // anything.
        let unknown = TradingPair {
            base: Principal::from_slice(&[0xAA]),
            quote: Principal::from_slice(&[0xBB]),
        };
        let result = setup
            .env()
            .update_call(
                setup.oisy_trade_id(),
                setup.controller(),
                "halt_trading",
                Encode!(&Some(vec![unknown])).unwrap(),
            )
            .await;
        assert_matches!(
            result,
            Err(RejectResponse { reject_code: RejectCode::CanisterError, reject_message, .. })
            if reject_message.contains("unknown trading pair")
        );

        setup.drop().await;
    }

    /// A per-pair `halt_trading` traps when given more than `MAX_HALT_BOOKS`
    /// (100) pairs, recording nothing. Bounds the `SetHalt` event size.
    #[tokio::test]
    async fn should_trap_on_too_many_pairs() {
        let setup = Setup::new().await.with_trading_pair().await;
        let pairs = vec![setup.trading_pair(); 101];

        for endpoint in ["halt_trading", "resume_trading"] {
            let result = setup
                .env()
                .update_call(
                    setup.oisy_trade_id(),
                    setup.controller(),
                    endpoint,
                    Encode!(&Some(pairs.clone())).unwrap(),
                )
                .await;
            assert_matches!(
                result,
                Err(RejectResponse { reject_code: RejectCode::CanisterError, reject_message, .. })
                if reject_message.contains("too many trading pairs"),
                "endpoint {endpoint} must trap on more than 100 pairs"
            );
        }

        setup.drop().await;
    }

    /// A global `resume_trading(None)` clears every per-pair halt in one call:
    /// a pair halted individually accepts orders again after a global resume.
    /// Unique to the per-pair mode.
    #[tokio::test]
    async fn should_clear_pair_halts_on_global_resume() {
        let setup = Setup::new().await.with_trading_pair().await;
        let pair = setup.trading_pair();
        let user = setup.user();
        let client = setup.oisy_trade_client();
        let controller_client = setup.oisy_trade_client_with_caller(setup.controller());

        let price = 1000u64;
        let quantity = 1_000_000u64;
        let order_cost = price * quantity;
        setup
            .deposit_flow(user, setup.quote_token_id())
            .mint(order_cost + 2 * QUOTE_LEDGER_FEE)
            .approve(order_cost + QUOTE_LEDGER_FEE)
            .deposit(order_cost)
            .execute()
            .await;

        // Halt the pair individually, then clear all halts globally.
        assert_eq!(
            controller_client.halt_trading(Some(vec![pair])).await,
            Ok(())
        );
        assert_eq!(
            setup.pair_status(pair).await,
            TradingStatus::Halted,
            "pair must report Halted after the per-pair halt"
        );
        assert_eq!(controller_client.resume_trading(None).await, Ok(()));

        // The per-pair halt is gone: the pair trades again.
        assert_eq!(
            setup.pair_status(pair).await,
            TradingStatus::Trading,
            "global resume must clear the per-pair halt"
        );
        client
            .add_limit_order(LimitOrderRequest {
                pair,
                side: Side::Buy,
                price: Nat::from(price),
                quantity: Nat::from(quantity),
                time_in_force: None,
            })
            .await
            .expect("orders accepted after the global resume clears the pair halt");

        setup.drop().await;
    }
}
