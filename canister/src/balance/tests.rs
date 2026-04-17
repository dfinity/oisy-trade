mod balance {
    use crate::balance::{Balance, InsufficientBalanceError};
    use crate::order::Quantity;

    #[test]
    fn should_reserve_from_free_balance() {
        let mut balance = Balance::zero();
        balance.deposit(Quantity::from(100));

        balance.reserve(Quantity::from(40)).unwrap();

        assert_eq!(balance, Balance::new(60u64, 40u64));
    }

    #[test]
    fn should_fail_to_reserve_more_than_free() {
        let mut balance = Balance::zero();
        balance.deposit(Quantity::from(50));
        let balance_before_reserve = balance.clone();

        assert_eq!(
            balance.reserve(Quantity::from(100)).unwrap_err(),
            InsufficientBalanceError {
                available: Quantity::from(50),
                required: Quantity::from(100),
            }
        );
        assert_eq!(
            balance_before_reserve, balance,
            "Balance should not have changed when reserve failed"
        );
    }

    #[test]
    fn should_debit_reserved() {
        let mut balance = Balance::new(10u64, 90u64);

        balance.debit_reserved(Quantity::from(30));

        assert_eq!(balance, Balance::new(10u64, 60u64));
    }

    #[test]
    #[should_panic(expected = "BUG: debit_reserved underflow")]
    fn should_panic_on_debit_reserved_underflow() {
        let mut balance = Balance::new(0u64, 10u64);
        balance.debit_reserved(Quantity::from(20));
    }

    #[test]
    fn should_withdraw_from_free_balance() {
        let mut balance = Balance::zero();
        balance.deposit(Quantity::from(100));

        balance.withdraw(Quantity::from(40)).unwrap();

        assert_eq!(balance, Balance::new(60u64, 0u64));
    }

    #[test]
    fn should_fail_to_withdraw_more_than_free() {
        let mut balance = Balance::new(50u64, 30u64);
        let balance_before = balance.clone();

        assert_eq!(
            balance.withdraw(Quantity::from(60)).unwrap_err(),
            InsufficientBalanceError {
                available: Quantity::from(50),
                required: Quantity::from(60),
            }
        );
        assert_eq!(
            balance_before, balance,
            "Balance should not have changed when withdraw failed"
        );
    }

    #[test]
    fn should_unreserve() {
        let mut balance = Balance::new(10u64, 90u64);

        balance.unreserve(Quantity::from(40));

        assert_eq!(balance, Balance::new(50u64, 50u64));
    }

    #[test]
    #[should_panic(expected = "BUG: unreserve underflow")]
    fn should_panic_on_unreserve_underflow() {
        let mut balance = Balance::new(100u64, 10u64);
        balance.unreserve(Quantity::from(20));
    }
}

mod user_balance {
    use crate::balance::user::UserBalance;
    use crate::balance::{Balance, InsufficientBalanceError};
    use crate::order::Quantity;
    use candid::Principal;

    #[test]
    fn should_deposit_to_new_user() {
        let mut ub = UserBalance::default();
        ub.deposit(alice(), Quantity::from(100u64));

        assert_eq!(ub.get(&alice()), Some(&Balance::new(100u64, 0u64)));
    }

    #[test]
    fn should_deposit_to_existing_user() {
        let mut ub = UserBalance::default();
        ub.deposit(alice(), Quantity::from(50u64));
        ub.deposit(alice(), Quantity::from(30u64));

        assert_eq!(ub.get(&alice()), Some(&Balance::new(80u64, 0u64)));
    }

    #[test]
    fn should_reserve_from_deposited() {
        let mut ub = UserBalance::default();
        ub.deposit(alice(), Quantity::from(100u64));

        ub.reserve(&alice(), Quantity::from(40u64)).unwrap();

        assert_eq!(ub.get(&alice()), Some(&Balance::new(60u64, 40u64)));
    }

    #[test]
    fn should_fail_reserve_with_insufficient_free() {
        let mut ub = UserBalance::default();
        ub.deposit(alice(), Quantity::from(10u64));

        let err = ub.reserve(&alice(), Quantity::from(50u64)).unwrap_err();

        assert_eq!(
            err,
            InsufficientBalanceError {
                available: Quantity::from(10u64),
                required: Quantity::from(50u64),
            }
        );
    }

    #[test]
    #[should_panic(expected = "BUG: user balance missing for reserve")]
    fn should_panic_reserve_missing_user() {
        let mut ub = UserBalance::default();
        let _ = ub.reserve(&alice(), Quantity::from(10u64));
    }

    #[test]
    fn should_transfer_reserved_to_free() {
        let mut ub = UserBalance::default();
        ub.deposit(alice(), Quantity::from(100u64));
        ub.deposit(bob(), Quantity::from(10u64));
        ub.reserve(&alice(), Quantity::from(100u64)).unwrap();

        // Bob exists
        assert_eq!(ub.get(&bob()), Some(&Balance::new(10u64, 0u64)));

        ub.transfer(&alice(), &bob(), Quantity::from(60u64));

        assert_eq!(ub.get(&alice()), Some(&Balance::new(0u64, 40u64)));
        assert_eq!(ub.get(&bob()), Some(&Balance::new(70u64, 0u64)));
    }

    #[test]
    fn should_transfer_creating_creditor_entry() {
        let mut ub = UserBalance::default();
        ub.deposit(alice(), Quantity::from(50u64));
        ub.reserve(&alice(), Quantity::from(50u64)).unwrap();

        // Bob doesn't exist yet
        assert_eq!(ub.get(&bob()), None);
        ub.transfer(&alice(), &bob(), Quantity::from(50u64));

        assert_eq!(ub.get(&bob()), Some(&Balance::new(50u64, 0u64)));
    }

    #[test]
    #[should_panic(expected = "BUG: debtor balance missing")]
    fn should_panic_transfer_missing_debtor() {
        let mut ub = UserBalance::default();
        ub.transfer(&alice(), &bob(), Quantity::from(10u64));
    }

    #[test]
    fn should_unreserve() {
        let mut ub = UserBalance::default();
        ub.deposit(alice(), Quantity::from(100u64));
        ub.reserve(&alice(), Quantity::from(80u64)).unwrap();

        ub.unreserve(&alice(), Quantity::from(30u64));

        assert_eq!(ub.get(&alice()), Some(&Balance::new(50u64, 50u64)));
    }

    #[test]
    #[should_panic(expected = "BUG: user balance missing for unreserve")]
    fn should_panic_unreserve_missing_user() {
        let mut ub = UserBalance::default();
        ub.unreserve(&alice(), Quantity::from(10u64));
    }

    #[test]
    fn should_return_none_for_unknown_user() {
        let ub = UserBalance::default();
        assert_eq!(ub.get(&alice()), None);
    }

    fn alice() -> Principal {
        Principal::from_slice(&[0x01])
    }

    fn bob() -> Principal {
        Principal::from_slice(&[0x02])
    }
}

mod token_balance {
    use crate::balance::{Balance, TokenBalance};
    use crate::order::{Quantity, TokenId};
    use candid::Principal;

    #[test]
    fn should_deposit_and_read_balance() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(&Balance::new(100u64, 0u64))
        );
    }

    #[test]
    fn should_return_none_for_unknown_balance() {
        let tb = TokenBalance::default();
        assert_eq!(tb.get_balance(&alice(), &token_a()), None);
        assert_eq!(tb.get_free(&alice(), &token_a()), Quantity::ZERO);
    }

    #[test]
    fn should_get_free_balance() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));

        assert_eq!(tb.get_free(&alice(), &token_a()), Quantity::from(100u64));
    }

    #[test]
    fn should_reserve_from_deposited() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));

        tb.reserve(&alice(), &token_a(), Quantity::from(40u64))
            .unwrap();

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(&Balance::new(60u64, 40u64))
        );
    }

    #[test]
    fn should_keep_tokens_separate() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));
        tb.deposit(alice(), token_b(), Quantity::from(200u64));

        assert_eq!(tb.get_free(&alice(), &token_a()), Quantity::from(100u64));
        assert_eq!(tb.get_free(&alice(), &token_b()), Quantity::from(200u64));
    }

    #[test]
    fn should_transfer_via_token_mut() {
        let bob = Principal::from_slice(&[0x02]);
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));
        tb.reserve(&alice(), &token_a(), Quantity::from(100u64))
            .unwrap();

        tb.token_mut(&token_a())
            .transfer(&alice(), &bob, Quantity::from(100u64));

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(&Balance::new(0u64, 0u64))
        );
        assert_eq!(
            tb.get_balance(&bob, &token_a()),
            Some(&Balance::new(100u64, 0u64))
        );
    }

    fn alice() -> Principal {
        Principal::from_slice(&[0x01])
    }

    fn token_a() -> TokenId {
        TokenId::new(Principal::from_slice(&[0xA0]))
    }

    fn token_b() -> TokenId {
        TokenId::new(Principal::from_slice(&[0xB0]))
    }
}
