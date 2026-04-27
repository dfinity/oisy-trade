use super::UserOpGuard;
use crate::order::TokenId;
use crate::state;
use crate::test_fixtures::state_vmem;
use candid::Principal;

const USER_A: Principal = Principal::from_slice(&[0x01]);
const USER_B: Principal = Principal::from_slice(&[0x02]);
const TOKEN_A: TokenId = TokenId::new(Principal::from_slice(&[0xAA]));
const TOKEN_B: TokenId = TokenId::new(Principal::from_slice(&[0xBB]));

fn init() {
    state::init_state(state_vmem());
}

fn assert_in_flight(expected: &[(Principal, TokenId)]) {
    state::with_state(|s| {
        let actual: Vec<_> = s.in_flight_user_ops().iter().copied().collect();
        let mut expected: Vec<_> = expected.to_vec();
        expected.sort();
        assert_eq!(actual, expected);
    });
}

#[test]
fn should_acquire_and_release_guard() {
    init();
    {
        let _guard = UserOpGuard::new(USER_A, TOKEN_A).expect("first acquire should succeed");
        assert_in_flight(&[(USER_A, TOKEN_A)]);
    }
    assert_in_flight(&[]);
}

#[test]
fn should_block_second_acquire_for_same_key() {
    init();
    let _first = UserOpGuard::new(USER_A, TOKEN_A).expect("first acquire should succeed");
    let second = UserOpGuard::new(USER_A, TOKEN_A);
    assert!(second.is_none(), "second acquire for same key must fail");
}

#[test]
fn should_re_acquire_after_drop() {
    init();
    drop(UserOpGuard::new(USER_A, TOKEN_A).expect("first acquire should succeed"));
    let _second = UserOpGuard::new(USER_A, TOKEN_A).expect("re-acquire after drop should succeed");
    assert_in_flight(&[(USER_A, TOKEN_A)]);
}

#[test]
fn should_not_block_distinct_token() {
    init();
    let _a = UserOpGuard::new(USER_A, TOKEN_A).expect("first acquire should succeed");
    let _b = UserOpGuard::new(USER_A, TOKEN_B).expect("distinct token should not be blocked");
    assert_in_flight(&[(USER_A, TOKEN_A), (USER_A, TOKEN_B)]);
}

#[test]
fn should_not_block_distinct_caller() {
    init();
    let _a = UserOpGuard::new(USER_A, TOKEN_A).expect("first acquire should succeed");
    let _b = UserOpGuard::new(USER_B, TOKEN_A).expect("distinct caller should not be blocked");
    assert_in_flight(&[(USER_A, TOKEN_A), (USER_B, TOKEN_A)]);
}
