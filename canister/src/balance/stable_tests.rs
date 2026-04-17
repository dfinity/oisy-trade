use super::stable::*;
use super::{Balance, InsufficientBalanceError};
use crate::order::{Quantity, TokenId};
use candid::Principal;
use ic_stable_structures::VectorMemory;

fn token_a() -> TokenId {
    TokenId::new(Principal::from_slice(&[1]))
}

fn token_b() -> TokenId {
    TokenId::new(Principal::from_slice(&[2]))
}

fn user_1() -> Principal {
    Principal::from_slice(&[10])
}

fn user_2() -> Principal {
    Principal::from_slice(&[20])
}

fn stable_balances() -> StableTokenBalance<VectorMemory> {
    StableTokenBalance::new(VectorMemory::default())
}

mod storable_roundtrips {
    use super::*;
    use ic_stable_structures::Storable;

    #[test]
    fn balance_key_roundtrip() {
        let key = BalanceKey::new(&token_a(), &user_1());
        let bytes = key.to_bytes();
        let restored = BalanceKey::from_bytes(bytes);
        assert_eq!(restored, key);
        assert_eq!(restored.token(), token_a());
        assert_eq!(restored.user(), user_1());
    }

    #[test]
    fn balance_key_ordering_token_first() {
        let key_a1 = BalanceKey::new(&token_a(), &user_1());
        let key_b1 = BalanceKey::new(&token_b(), &user_1());
        // token_a < token_b (by principal bytes)
        assert!(key_a1 < key_b1);
    }

    #[test]
    fn balance_key_ordering_user_within_token() {
        let key_a1 = BalanceKey::new(&token_a(), &user_1());
        let key_a2 = BalanceKey::new(&token_a(), &user_2());
        // Same token, different users — user_1 < user_2
        assert!(key_a1 < key_a2);
    }

    #[test]
    fn storable_balance_roundtrip() {
        let sb = StorableBalance {
            free: Quantity::from(42_000_000u64),
            reserved: Quantity::from(1_000u64),
        };
        let bytes = sb.to_bytes();
        let restored = StorableBalance::from_bytes(bytes);
        assert_eq!(restored, sb);
    }

    #[test]
    fn storable_balance_roundtrip_zero() {
        let sb = StorableBalance {
            free: Quantity::ZERO,
            reserved: Quantity::ZERO,
        };
        let bytes = sb.to_bytes();
        let restored = StorableBalance::from_bytes(bytes);
        assert_eq!(restored, sb);
    }
}

mod deposit {
    use super::*;

    #[test]
    fn should_deposit_to_new_user() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));

        let bal = balances.get_balance(&user_1(), &token_a()).unwrap();
        assert_eq!(bal, Balance::new(100u64, 0u64));
    }

    #[test]
    fn should_accumulate_deposits() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));
        balances.deposit(user_1(), token_a(), Quantity::from(50u64));

        assert_eq!(balances.get_free(&user_1(), &token_a()), Quantity::from(150u64));
    }

    #[test]
    fn should_separate_tokens() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));
        balances.deposit(user_1(), token_b(), Quantity::from(200u64));

        assert_eq!(balances.get_free(&user_1(), &token_a()), Quantity::from(100u64));
        assert_eq!(balances.get_free(&user_1(), &token_b()), Quantity::from(200u64));
    }

    #[test]
    fn should_separate_users() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));
        balances.deposit(user_2(), token_a(), Quantity::from(200u64));

        assert_eq!(balances.get_free(&user_1(), &token_a()), Quantity::from(100u64));
        assert_eq!(balances.get_free(&user_2(), &token_a()), Quantity::from(200u64));
    }
}

mod get_balance {
    use super::*;

    #[test]
    fn should_return_none_for_unknown_user() {
        let balances = stable_balances();
        assert!(balances.get_balance(&user_1(), &token_a()).is_none());
    }

    #[test]
    fn should_return_zero_free_for_unknown_user() {
        let balances = stable_balances();
        assert_eq!(balances.get_free(&user_1(), &token_a()), Quantity::ZERO);
    }
}

mod reserve {
    use super::*;

    #[test]
    fn should_reserve_from_free() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));
        balances.reserve(&user_1(), &token_a(), Quantity::from(40u64)).unwrap();

        let bal = balances.get_balance(&user_1(), &token_a()).unwrap();
        assert_eq!(bal, Balance::new(60u64, 40u64));
    }

    #[test]
    fn should_fail_reserve_with_insufficient_balance() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(10u64));

        let err = balances
            .reserve(&user_1(), &token_a(), Quantity::from(50u64))
            .unwrap_err();
        assert_eq!(
            err,
            InsufficientBalanceError {
                available: Quantity::from(10u64),
                required: Quantity::from(50u64),
            }
        );
    }

    #[test]
    fn should_fail_reserve_for_unknown_user() {
        let mut balances = stable_balances();
        let err = balances
            .reserve(&user_1(), &token_a(), Quantity::from(50u64))
            .unwrap_err();
        assert_eq!(
            err,
            InsufficientBalanceError {
                available: Quantity::ZERO,
                required: Quantity::from(50u64),
            }
        );
    }
}

mod withdraw {
    use super::*;

    #[test]
    fn should_withdraw_from_free() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));
        balances.withdraw(&user_1(), &token_a(), Quantity::from(40u64)).unwrap();

        assert_eq!(balances.get_free(&user_1(), &token_a()), Quantity::from(60u64));
    }

    #[test]
    fn should_fail_withdraw_with_insufficient_balance() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(10u64));

        let err = balances
            .withdraw(&user_1(), &token_a(), Quantity::from(50u64))
            .unwrap_err();
        assert_eq!(
            err,
            InsufficientBalanceError {
                available: Quantity::from(10u64),
                required: Quantity::from(50u64),
            }
        );
    }
}

mod transfer {
    use super::*;

    #[test]
    fn should_transfer_reserved_to_free() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));
        balances.reserve(&user_1(), &token_a(), Quantity::from(60u64)).unwrap();

        balances.transfer(&token_a(), &user_1(), &user_2(), Quantity::from(60u64));

        let bal1 = balances.get_balance(&user_1(), &token_a()).unwrap();
        assert_eq!(bal1, Balance::new(40u64, 0u64));

        let bal2 = balances.get_balance(&user_2(), &token_a()).unwrap();
        assert_eq!(bal2, Balance::new(60u64, 0u64));
    }
}

mod unreserve {
    use super::*;

    #[test]
    fn should_move_reserved_back_to_free() {
        let mut balances = stable_balances();
        balances.deposit(user_1(), token_a(), Quantity::from(100u64));
        balances.reserve(&user_1(), &token_a(), Quantity::from(60u64)).unwrap();

        balances.unreserve(&token_a(), &user_1(), Quantity::from(30u64));

        let bal = balances.get_balance(&user_1(), &token_a()).unwrap();
        assert_eq!(bal, Balance::new(70u64, 30u64));
    }
}
