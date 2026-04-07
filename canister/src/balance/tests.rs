use super::{Balance, InsufficientBalanceError};
use candid::Nat;

#[test]
fn should_reserve_from_free_balance() {
    let mut balance = Balance::zero();
    balance.deposit(Nat::from(100u64));

    balance.reserve(Nat::from(40u64)).unwrap();

    assert_eq!(balance, Balance::new(60u64, 40u64));
}

#[test]
fn should_fail_to_reserve_more_than_free() {
    let mut balance = Balance::zero();
    balance.deposit(Nat::from(50u64));
    let balance_before_reserve = balance.clone();

    assert_eq!(
        balance.reserve(Nat::from(100u64)).unwrap_err(),
        InsufficientBalanceError {
            available: Nat::from(50u64),
            required: Nat::from(100u64),
        }
    );
    assert_eq!(
        balance_before_reserve, balance,
        "Balance should not have changed when reserve failed"
    );
}
