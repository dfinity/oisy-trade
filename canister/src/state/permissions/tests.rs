use super::{Permissions, Reconciliation, UnauthorizedError};
use crate::order::OrderBookId;
use candid::Principal;

#[test]
fn should_permit_every_event_on_empty_permissions() {
    let permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);
    let book = OrderBookId::ZERO;

    assert!(permissions.permit_trading(caller, book).is_ok());
    assert!(permissions.permit_matching(book).is_ok());
    assert!(permissions.permit_deposit(caller).is_ok());
    assert!(permissions.permit_withdraw(caller).is_ok());
    assert!(permissions.permit_cancel().is_ok());
    assert!(permissions.permit_settling().is_ok());
    assert!(permissions.permit_add_trading_pair().is_ok());
    assert!(permissions.permit_admin().is_ok());
}

#[test]
fn should_reconcile_clean_on_empty_permissions() {
    let permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);

    let pre = permissions
        .permit_deposit(caller)
        .expect("deposit is never gated in this build");
    assert!(matches!(
        pre.reconcile(&permissions).verdict,
        Reconciliation::Clean
    ));

    let pre = permissions
        .permit_withdraw(caller)
        .expect("withdraw is never gated in this build");
    assert!(matches!(
        pre.reconcile(&permissions).verdict,
        Reconciliation::Clean
    ));
}

#[test]
fn should_reject_trading_and_matching_when_globally_halted() {
    let mut permissions = Permissions::default();
    permissions.set_trading_halted(true);
    let caller = Principal::from_slice(&[1]);
    let book = OrderBookId::ZERO;

    assert!(matches!(
        permissions.permit_trading(caller, book),
        Err(UnauthorizedError::TradingHalted)
    ));
    assert!(matches!(
        permissions.permit_matching(book),
        Err(UnauthorizedError::TradingHalted)
    ));
}

#[test]
fn should_keep_ungated_permits_ok_when_globally_halted() {
    let mut permissions = Permissions::default();
    permissions.set_trading_halted(true);
    let caller = Principal::from_slice(&[1]);

    assert!(permissions.permit_deposit(caller).is_ok());
    assert!(permissions.permit_withdraw(caller).is_ok());
    assert!(permissions.permit_cancel().is_ok());
    assert!(permissions.permit_settling().is_ok());
    assert!(permissions.permit_add_trading_pair().is_ok());
    assert!(permissions.permit_admin().is_ok());
}

#[test]
fn should_reject_trading_on_halted_pair_only() {
    let mut permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);
    let halted = OrderBookId::new(0);
    let other = OrderBookId::new(1);
    permissions.set_pair_halted(halted, true);

    assert!(permissions.is_pair_halted(&halted));
    assert!(!permissions.is_pair_halted(&other));
    assert!(matches!(
        permissions.permit_trading(caller, halted),
        Err(UnauthorizedError::PairHalted)
    ));
    assert!(permissions.permit_trading(caller, other).is_ok());
    // The per-pair halt gates matching on the halted book only; every other
    // book keeps matching.
    assert!(matches!(
        permissions.permit_matching(halted),
        Err(UnauthorizedError::PairHalted)
    ));
    assert!(permissions.permit_matching(other).is_ok());
}

#[test]
fn should_re_enable_trading_after_unhalting_pair() {
    let mut permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);
    let book = OrderBookId::new(0);

    permissions.set_pair_halted(book, true);
    permissions.set_pair_halted(book, false);

    assert!(!permissions.is_pair_halted(&book));
    assert!(permissions.permit_trading(caller, book).is_ok());
}

#[test]
fn should_keep_ungated_permits_ok_when_pair_halted() {
    let mut permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);
    permissions.set_pair_halted(OrderBookId::new(0), true);

    assert!(permissions.permit_deposit(caller).is_ok());
    assert!(permissions.permit_withdraw(caller).is_ok());
    assert!(permissions.permit_cancel().is_ok());
    assert!(permissions.permit_settling().is_ok());
    assert!(permissions.permit_add_trading_pair().is_ok());
    assert!(permissions.permit_admin().is_ok());
}

#[test]
fn should_re_enable_trading_after_resuming() {
    let mut permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);
    let book = OrderBookId::ZERO;

    permissions.set_trading_halted(true);
    permissions.set_trading_halted(false);

    assert!(permissions.permit_trading(caller, book).is_ok());
    assert!(permissions.permit_matching(book).is_ok());
}
