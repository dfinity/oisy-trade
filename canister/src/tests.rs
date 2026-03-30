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
        let status = get_order_status(u64::MAX);
        assert_eq!(status, OrderStatus::NotFound);
    }
}

mod supported_tokens {
    use crate::state::init_state;
    use crate::{add_supported_token, get_supported_tokens};
    use dex_types::Token;

    fn test_token(symbol: &str, ledger_id: &str) -> Token {
        Token {
            name: symbol.to_string(),
            symbol: symbol.to_string(),
            decimals: 8,
            ledger_id: candid::Principal::from_text(ledger_id).unwrap(),
            fee: candid::Nat::from(10_000_u64),
        }
    }

    #[test]
    fn should_return_empty_when_no_tokens_added() {
        init_state();
        assert!(get_supported_tokens().is_empty());
    }

    #[test]
    fn should_add_and_get_supported_token() {
        init_state();
        let token = test_token("ICP", "ryjl3-tyaaa-aaaaa-aaaba-cai");
        add_supported_token(token.clone());

        let tokens = get_supported_tokens();
        assert_eq!(tokens, vec![token]);
    }

    #[test]
    fn should_add_multiple_tokens() {
        init_state();
        let icp = test_token("ICP", "ryjl3-tyaaa-aaaaa-aaaba-cai");
        let ckbtc = test_token("ckBTC", "mxzaz-hqaaa-aaaar-qaada-cai");

        add_supported_token(icp.clone());
        add_supported_token(ckbtc.clone());

        let tokens = get_supported_tokens();
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains(&icp));
        assert!(tokens.contains(&ckbtc));
    }

    #[test]
    fn should_not_duplicate_same_token() {
        init_state();
        let token = test_token("ICP", "ryjl3-tyaaa-aaaaa-aaaba-cai");
        add_supported_token(token.clone());
        add_supported_token(token.clone());

        let tokens = get_supported_tokens();
        assert_eq!(tokens, vec![token]);
    }
}
