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
            Some(Balance::new(60u64, 40u64))
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
    fn should_withdraw_from_deposited() {
        let mut tb = TokenBalance::default();
        tb.deposit(alice(), token_a(), Quantity::from(100u64));

        tb.withdraw(&alice(), &token_a(), Quantity::from(40u64))
            .unwrap();

        assert_eq!(tb.get_free(&alice(), &token_a()), Quantity::from(60u64));
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

        tb.transfer(&alice(), &bob, &token_a(), Quantity::from(100u64));

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

        tb.transfer(&alice(), &alice(), &token_a(), Quantity::from(60u64));

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
        tb.transfer(&alice(), &bob, &token_a(), Quantity::from(10u64));
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

mod key {
    use super::super::BalanceKey;
    use crate::order::TokenId;
    use candid::Principal;
    use ic_stable_structures::Storable;
    use proptest::prelude::*;

    /// Arbitrary principal up to the 29-byte spec max. Covers length 0, 1,
    /// 29, and same-length-different-bytes by construction.
    fn arb_principal() -> impl Strategy<Value = Principal> {
        prop::collection::vec(any::<u8>(), 0..=29).prop_map(|bytes| Principal::from_slice(&bytes))
    }

    fn arb_key() -> impl Strategy<Value = BalanceKey> {
        (arb_principal(), arb_principal())
            .prop_map(|(token, owner)| BalanceKey::new(TokenId::new(token), owner))
    }

    proptest! {
        #[test]
        fn should_roundtrip_through_stable_bytes(key in arb_key()) {
            let bytes = key.to_bytes();
            prop_assert_eq!(bytes.len(), BalanceKey::ENCODED_SIZE as usize);
            let decoded = BalanceKey::from_bytes(bytes);
            prop_assert_eq!(decoded, key);
        }

        /// Byte-lexicographic order of the encoded key must agree with
        /// `(TokenId, Principal)` ordering, which is what `StableBTreeMap`
        /// uses to iterate entries. A mismatch would silently break range
        /// scans and any future per-token iteration.
        #[test]
        fn should_preserve_order_under_encoding(a in arb_key(), b in arb_key()) {
            let bytes_a = a.to_bytes();
            let bytes_b = b.to_bytes();
            let natural = (a.token(), a.owner()).cmp(&(b.token(), b.owner()));
            prop_assert_eq!(natural, bytes_a.cmp(&bytes_b));
        }
    }
}
