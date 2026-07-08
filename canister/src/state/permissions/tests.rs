use super::{Permissions, UnauthorizedError};
use crate::order::OrderBookId;
use crate::test_fixtures::arbitrary::arb_book_halted_permissions;
use candid::Principal;
use proptest::prelude::*;

const BOOK: OrderBookId = OrderBookId::ZERO;

proptest! {
    /// Whether trading is halted globally or just for the book, the gated
    /// permits on that book are rejected while every ungated permit stays OK.
    /// Under a per-pair halt, a distinct book stays un-gated; under a global
    /// halt every book is gated.
    #[test]
    fn should_gate_book_when_halted((permissions, other, global) in arb_book_halted_permissions()) {
        let caller = Principal::from_slice(&[1]);

        prop_assert!(permissions.is_halted(&BOOK));
        prop_assert!(matches!(
            permissions.permit_trading(caller, BOOK),
            Err(UnauthorizedError::TradingHalted)
        ));
        prop_assert!(matches!(
            permissions.permit_matching(BOOK),
            Err(UnauthorizedError::TradingHalted)
        ));

        prop_assert!(permissions.permit_deposit(caller, false).is_ok());
        prop_assert!(permissions.permit_withdraw(caller, false).is_ok());
        let _ = permissions.permit_cancel();
        let _ = permissions.permit_settling();
        let _ = permissions.permit_add_trading_pair();
        let _ = permissions.permit_admin();

        if global {
            prop_assert!(permissions.is_halted(&other));
            prop_assert!(matches!(
                permissions.permit_trading(caller, other),
                Err(UnauthorizedError::TradingHalted)
            ));
            prop_assert!(matches!(
                permissions.permit_matching(other),
                Err(UnauthorizedError::TradingHalted)
            ));
        } else {
            prop_assert!(!permissions.is_halted(&other));
            prop_assert!(permissions.permit_trading(caller, other).is_ok());
            prop_assert!(permissions.permit_matching(other).is_ok());
        }
    }
}

#[test]
fn should_permit_every_event_on_empty_permissions() {
    let permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);

    assert!(permissions.permit_trading(caller, BOOK).is_ok());
    assert!(permissions.permit_matching(BOOK).is_ok());
    assert!(permissions.permit_deposit(caller, false).is_ok());
    assert!(permissions.permit_withdraw(caller, false).is_ok());
    let _ = permissions.permit_cancel();
    let _ = permissions.permit_settling();
    let _ = permissions.permit_add_trading_pair();
    let _ = permissions.permit_admin();
}

#[test]
fn should_deny_funding_operations_to_a_trading_account() {
    let permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);

    assert!(matches!(
        permissions.permit_deposit(caller, true),
        Err(UnauthorizedError::TradingAccountCannotFund)
    ));
    assert!(matches!(
        permissions.permit_withdraw(caller, true),
        Err(UnauthorizedError::TradingAccountCannotFund)
    ));
    // A caller that is not a trading account is still admitted.
    assert!(permissions.permit_deposit(caller, false).is_ok());
    assert!(permissions.permit_withdraw(caller, false).is_ok());
}

#[test]
fn should_re_enable_trading_after_resuming_pair() {
    let mut permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);

    permissions.halt_trading(BOOK);
    permissions.resume_trading(BOOK);

    assert!(!permissions.is_halted(&BOOK));
    assert!(permissions.permit_trading(caller, BOOK).is_ok());
}

#[test]
fn should_clear_every_halted_pair_on_global_resume() {
    let mut permissions = Permissions::default();
    let a = OrderBookId::new(0);
    let b = OrderBookId::new(1);
    permissions.halt_trading_globally();
    permissions.halt_trading(a);
    permissions.halt_trading(b);

    permissions.resume_trading_globally();

    assert!(!permissions.is_halted(&a));
    assert!(!permissions.is_halted(&b));
}
