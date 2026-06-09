use super::{AsyncKind, Permissions, Reconciliation, UnauthorizedError};
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

#[test]
fn should_tag_async_permits_with_their_kind() {
    let permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);

    assert_eq!(
        permissions.permit_deposit(caller).unwrap().kind(),
        &AsyncKind::Deposit
    );
    assert_eq!(
        permissions.permit_withdraw(caller).unwrap().kind(),
        &AsyncKind::Withdraw
    );
}

#[test]
fn should_reject_frozen_account_on_trading_deposit_and_withdraw() {
    let mut permissions = Permissions::default();
    let frozen = Principal::from_slice(&[1]);
    let other = Principal::from_slice(&[2]);
    let book = OrderBookId::ZERO;
    permissions.set_account_frozen(frozen, true);

    assert!(permissions.is_frozen(&frozen));
    assert!(!permissions.is_frozen(&other));

    assert!(matches!(
        permissions.permit_trading(frozen, book),
        Err(UnauthorizedError::AccountFrozen)
    ));
    assert!(matches!(
        permissions.permit_deposit(frozen),
        Err(UnauthorizedError::AccountFrozen)
    ));
    assert!(matches!(
        permissions.permit_withdraw(frozen),
        Err(UnauthorizedError::AccountFrozen)
    ));

    // A different account is unaffected.
    assert!(permissions.permit_trading(other, book).is_ok());
    assert!(permissions.permit_deposit(other).is_ok());
    assert!(permissions.permit_withdraw(other).is_ok());
}

#[test]
fn should_check_freeze_before_global_and_pair_halt_on_trading() {
    let mut permissions = Permissions::default();
    let frozen = Principal::from_slice(&[1]);
    let book = OrderBookId::ZERO;
    permissions.set_account_frozen(frozen, true);
    permissions.set_trading_halted(true);
    permissions.set_pair_halted(book, true);

    // Freeze wins over both halts (it is the first arm).
    assert!(matches!(
        permissions.permit_trading(frozen, book),
        Err(UnauthorizedError::AccountFrozen)
    ));
}

#[test]
fn should_keep_ungated_permits_ok_when_account_frozen() {
    let mut permissions = Permissions::default();
    let frozen = Principal::from_slice(&[1]);
    permissions.set_account_frozen(frozen, true);

    assert!(permissions.permit_cancel().is_ok());
    assert!(permissions.permit_settling().is_ok());
    assert!(permissions.permit_add_trading_pair().is_ok());
    assert!(permissions.permit_admin().is_ok());
    // Matching is never gated by a freeze: a frozen account's resting orders
    // keep filling for counterparties.
    assert!(permissions.permit_matching(OrderBookId::ZERO).is_ok());
}

#[test]
fn should_re_enable_access_after_unfreezing() {
    let mut permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);
    let book = OrderBookId::ZERO;

    permissions.set_account_frozen(caller, true);
    permissions.set_account_frozen(caller, false);

    assert!(!permissions.is_frozen(&caller));
    assert!(permissions.permit_trading(caller, book).is_ok());
    assert!(permissions.permit_deposit(caller).is_ok());
    assert!(permissions.permit_withdraw(caller).is_ok());
}

#[test]
fn should_reconcile_clean_when_caller_not_frozen_post_check() {
    let permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);

    let pre = permissions.permit_deposit(caller).unwrap();
    let post = pre.reconcile(&permissions);
    assert_eq!(post.verdict(), &Reconciliation::Clean);
}

#[test]
fn should_reconcile_raced_when_caller_frozen_post_check() {
    let mut permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);

    // Admitted pre-await (not yet frozen), then frozen mid-await.
    let pre = permissions.permit_withdraw(caller).unwrap();
    permissions.set_account_frozen(caller, true);
    let post = pre.reconcile(&permissions);
    assert_eq!(post.verdict(), &Reconciliation::Raced);
}
