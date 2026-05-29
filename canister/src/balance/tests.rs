mod balance {
    use crate::balance::{Balance, InsufficientBalanceError};
    use crate::order::Quantity;
    use crate::test_fixtures::arbitrary::arb_balance;
    use ic_stable_structures::Storable;
    use proptest::prelude::*;

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
        let balance_before_reserve = balance;

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

        balance.debit_reserved(&Quantity::from(30));

        assert_eq!(balance, Balance::new(10u64, 60u64));
    }

    #[test]
    #[should_panic(expected = "BUG: debit_reserved underflow")]
    fn should_panic_on_debit_reserved_underflow() {
        let mut balance = Balance::new(0u64, 10u64);
        balance.debit_reserved(&Quantity::from(20));
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
        let balance_before = balance;

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

    proptest! {
        #[test]
        fn should_roundtrip_through_stable_bytes(balance in arb_balance()) {
            let bytes = balance.to_bytes();
            let decoded = Balance::from_bytes(bytes);
            prop_assert_eq!(decoded, balance);
        }
    }
}

mod token_balance {
    use crate::balance::{Balance, InsufficientBalanceError, TokenBalance};
    use crate::order::{Quantity, TokenId};
    use candid::Principal;

    #[test]
    fn should_deposit_and_read_balance() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(100u64, 0u64))
        );
    }

    #[test]
    fn should_return_none_for_unknown_balance() {
        let tb = TokenBalance::default();
        assert_eq!(tb.get_balance(&alice(), &token_a()), None);
    }

    #[test]
    fn should_reserve_from_deposited() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));

        tb.reserve(&alice(), &token_a(), Quantity::from(40u64))
            .unwrap();

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(60u64, 40u64))
        );
    }

    #[test]
    fn should_keep_tokens_separate() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));
        tb.deposit(alice(), token_b(), Quantity::from(200u64));

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(100u64, 0u64))
        );
        assert_eq!(
            tb.get_balance(&alice(), &token_b()),
            Some(Balance::new(200u64, 0u64))
        );
    }

    #[test]
    fn should_withdraw_from_deposited() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));

        tb.withdraw(&alice(), &token_a(), Quantity::from(40u64))
            .unwrap();

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(60u64, 0u64))
        );
    }

    #[test]
    fn should_fail_to_withdraw_from_missing_entry() {
        let mut tb = TokenBalance::default();

        let err = tb
            .withdraw(&alice(), &token_a(), Quantity::from(10u64))
            .unwrap_err();

        assert_eq!(
            err,
            InsufficientBalanceError {
                available: Quantity::ZERO,
                required: Quantity::from(10u64),
            }
        );
    }

    #[test]
    fn should_fail_to_reserve_missing_entry() {
        let mut tb = TokenBalance::default();

        let err = tb
            .reserve(&alice(), &token_a(), Quantity::from(10u64))
            .unwrap_err();

        assert_eq!(
            err,
            InsufficientBalanceError {
                available: Quantity::ZERO,
                required: Quantity::from(10u64),
            }
        );
    }

    #[test]
    fn should_transfer_between_users() {
        let bob = Principal::from_slice(&[0x02]);
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));
        tb.reserve(&alice(), &token_a(), Quantity::from(100u64))
            .unwrap();

        tb.transfer(
            &alice(),
            &bob,
            &token_a(),
            Quantity::from(100u64),
            Quantity::ZERO,
        );

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(0u64, 0u64))
        );
        assert_eq!(
            tb.get_balance(&bob, &token_a()),
            Some(Balance::new(100u64, 0u64))
        );
    }

    #[test]
    fn should_transfer_self_trade_preserves_free_and_clears_reserved() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));
        tb.reserve(&alice(), &token_a(), Quantity::from(60u64))
            .unwrap();

        tb.transfer(
            &alice(),
            &alice(),
            &token_a(),
            Quantity::from(60u64),
            Quantity::ZERO,
        );

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(100u64, 0u64))
        );
    }

    #[test]
    #[should_panic(expected = "BUG: debtor balance missing")]
    fn should_panic_transfer_missing_debtor() {
        let bob = Principal::from_slice(&[0x02]);
        let mut tb = TokenBalance::default();
        tb.transfer(
            &alice(),
            &bob,
            &token_a(),
            Quantity::from(10u64),
            Quantity::ZERO,
        );
    }

    #[test]
    fn should_unreserve_back_to_free() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));
        tb.reserve(&alice(), &token_a(), Quantity::from(80u64))
            .unwrap();

        tb.unreserve(&alice(), &token_a(), Quantity::from(30u64));

        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(50u64, 50u64))
        );
    }

    #[test]
    #[should_panic(expected = "BUG: user balance missing for unreserve")]
    fn should_panic_unreserve_missing_entry() {
        let mut tb = TokenBalance::default();
        tb.unreserve(&alice(), &token_a(), Quantity::from(10u64));
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

mod fee_pool {
    use crate::balance::{Balance, FeeEntry, TokenBalance};
    use crate::order::{Quantity, TokenId};
    use candid::Principal;

    #[test]
    fn fee_balance_is_none_for_unknown_token() {
        let tb = TokenBalance::default();
        assert_eq!(tb.fee_balance(&token_a()), None);
    }

    #[test]
    fn transfer_credits_net_to_creditor_and_accrues_to_pool() {
        let mut tb = setup_alice_reserve(100);

        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(40u64),
            Quantity::from(3u64),
        );

        // Debtor's reserved is debited by the full gross.
        assert_eq!(
            tb.get_balance(&alice(), &token_a()),
            Some(Balance::new(0u64, 60u64))
        );
        // Creditor receives gross − fee.
        assert_eq!(
            tb.get_balance(&bob(), &token_a()),
            Some(Balance::new(37u64, 0u64))
        );
        assert_eq!(tb.fee_balance(&token_a()), Some(Quantity::from(3u64)));
    }

    #[test]
    fn multiple_accruals_sum_per_token() {
        let mut tb = setup_alice_reserve(100);
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(40u64),
            Quantity::from(3u64),
        );
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(20u64),
            Quantity::from(1u64),
        );

        assert_eq!(tb.fee_balance(&token_a()), Some(Quantity::from(4u64)));
    }

    /// `Σ users(free + reserved) + fee_pool` is conserved on every
    /// transfer call. The pool absorbs exactly the fee withheld
    /// from the creditor.
    #[test]
    fn invariant_holds_across_a_mixed_workload() {
        let mut tb = setup_alice_reserve(100);
        tb.deposit(bob(), token_b(), Quantity::from(50u64));

        // Several operations against token_a:
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(40u64),
            Quantity::from(2u64),
        );
        tb.reserve(&bob(), &token_a(), Quantity::from(10u64))
            .unwrap();
        tb.unreserve(&bob(), &token_a(), Quantity::from(5u64));
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(10u64),
            Quantity::from(1u64),
        );
        tb.withdraw(&bob(), &token_a(), Quantity::from(7u64))
            .unwrap();

        let total_a = sum_users(&tb, &token_a())
            .checked_add(tb.fee_balance(&token_a()).unwrap_or_default())
            .unwrap();
        // 100 deposited into token_a, 7 withdrawn; invariant holds.
        assert_eq!(total_a, Quantity::from(93u64));

        // token_b had no fee activity; user sum equals the deposit.
        assert_eq!(sum_users(&tb, &token_b()), Quantity::from(50u64));
        assert_eq!(tb.fee_balance(&token_b()), None);
    }

    #[test]
    #[should_panic(expected = "fee")]
    fn should_panic_when_fee_exceeds_gross() {
        let mut tb = setup_alice_reserve(100);
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(10u64),
            Quantity::from(11u64),
        );
    }

    #[test]
    fn snapshot_roundtrips_through_save_and_restore() {
        let mut tb = setup_alice_reserve(100);
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(40u64),
            Quantity::from(3u64),
        );
        tb.deposit(alice(), token_b(), Quantity::from(50u64));
        tb.reserve(&alice(), &token_b(), Quantity::from(20u64))
            .unwrap();
        tb.transfer(
            &alice(),
            &bob(),
            &token_b(),
            Quantity::from(20u64),
            Quantity::from(2u64),
        );

        let snapshot: Vec<FeeEntry> = tb.fee_pool_snapshot();
        assert_eq!(snapshot.len(), 2);

        let mut restored = TokenBalance::default();
        restored.restore_fee_pool(snapshot.clone());
        assert_eq!(restored.fee_balance(&token_a()), Some(Quantity::from(3u64)));
        assert_eq!(restored.fee_balance(&token_b()), Some(Quantity::from(2u64)));

        // Snapshot order is by TokenId; round-tripping the Vec yields the
        // same Vec.
        let resnap: Vec<FeeEntry> = restored.fee_pool_snapshot();
        assert_eq!(resnap, snapshot);
    }

    #[test]
    fn restore_replaces_any_existing_pool() {
        let mut tb = setup_alice_reserve(100);
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(20u64),
            Quantity::from(5u64),
        );
        assert_eq!(tb.fee_balance(&token_a()), Some(Quantity::from(5u64)));

        tb.restore_fee_pool(vec![FeeEntry {
            token: token_b(),
            amount: Quantity::from(7u64),
        }]);

        assert_eq!(tb.fee_balance(&token_a()), None);
        assert_eq!(tb.fee_balance(&token_b()), Some(Quantity::from(7u64)));
    }

    #[test]
    fn drain_fee_credits_recipient_and_decreases_pool() {
        let mut tb = setup_alice_reserve(100);
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(40u64),
            Quantity::from(10u64),
        );
        assert_eq!(tb.fee_balance(&token_a()), Some(Quantity::from(10u64)));

        let admin = Principal::from_slice(&[0xAA]);
        tb.drain_fee_to(&token_a(), Quantity::from(7u64), admin)
            .unwrap();

        assert_eq!(tb.fee_balance(&token_a()), Some(Quantity::from(3u64)));
        assert_eq!(
            tb.get_balance(&admin, &token_a()),
            Some(Balance::new(7u64, 0u64)),
        );
    }

    #[test]
    fn drain_fee_clears_entry_when_pool_reaches_zero() {
        let mut tb = setup_alice_reserve(100);
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(40u64),
            Quantity::from(5u64),
        );
        let admin = Principal::from_slice(&[0xAA]);

        tb.drain_fee_to(&token_a(), Quantity::from(5u64), admin)
            .unwrap();

        assert_eq!(tb.fee_balance(&token_a()), None);
        assert_eq!(tb.iter_fee_balances().count(), 0);
    }

    #[test]
    fn drain_fee_returns_err_on_insufficient_pool() {
        let mut tb = setup_alice_reserve(100);
        tb.transfer(
            &alice(),
            &bob(),
            &token_a(),
            Quantity::from(40u64),
            Quantity::from(3u64),
        );
        let admin = Principal::from_slice(&[0xAA]);

        let err = tb
            .drain_fee_to(&token_a(), Quantity::from(4u64), admin)
            .unwrap_err();

        assert_eq!(
            err,
            crate::balance::InsufficientBalanceError {
                available: Quantity::from(3u64),
                required: Quantity::from(4u64),
            },
        );
        // Untouched on error.
        assert_eq!(tb.fee_balance(&token_a()), Some(Quantity::from(3u64)));
        assert_eq!(tb.get_balance(&admin, &token_a()), None);
    }

    #[test]
    #[should_panic(expected = "duplicate fee-pool entry")]
    fn restore_traps_on_duplicate_entries() {
        let mut tb = TokenBalance::default();
        tb.restore_fee_pool(vec![
            FeeEntry {
                token: token_a(),
                amount: Quantity::from(1u64),
            },
            FeeEntry {
                token: token_a(),
                amount: Quantity::from(2u64),
            },
        ]);
    }

    fn setup_alice_reserve(amount: u64) -> TokenBalance<ic_stable_structures::VectorMemory> {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(amount));
        tb.reserve(&alice(), &token_a(), Quantity::from(amount))
            .unwrap();
        tb
    }

    fn sum_users(
        tb: &TokenBalance<ic_stable_structures::VectorMemory>,
        token: &TokenId,
    ) -> Quantity {
        let mut acc = Quantity::ZERO;
        for (key, balance) in tb.iter() {
            if key.token() == token {
                let free_plus_reserved = balance
                    .free()
                    .checked_add(*balance.reserved())
                    .expect("test overflow");
                acc = acc.checked_add(free_plus_reserved).expect("test overflow");
            }
        }
        acc
    }

    fn alice() -> Principal {
        Principal::from_slice(&[0x01])
    }

    fn bob() -> Principal {
        Principal::from_slice(&[0x02])
    }

    fn token_a() -> TokenId {
        TokenId::new(Principal::from_slice(&[0xA0]))
    }

    fn token_b() -> TokenId {
        TokenId::new(Principal::from_slice(&[0xB0]))
    }
}

mod key {
    use super::super::BalanceKey;
    use crate::test_fixtures::arbitrary::arb_balance_key;
    use ic_stable_structures::Storable;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn should_roundtrip_through_stable_bytes(key in arb_balance_key()) {
            let bytes = key.to_bytes();
            let decoded = BalanceKey::from_bytes(bytes);
            prop_assert_eq!(decoded, key);
        }
    }
}
