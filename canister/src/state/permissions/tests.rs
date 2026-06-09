use super::Permissions;
use crate::order::OrderBookId;
use candid::Principal;

#[test]
fn should_permit_every_event_on_empty_permissions() {
    let permissions = Permissions::default();
    let caller = Principal::from_slice(&[1]);
    let book = OrderBookId::ZERO;

    assert!(permissions.permit_trading(caller, book).is_ok());
    assert!(permissions.permit_matching().is_ok());
    assert!(permissions.permit_deposit(caller).is_ok());
    assert!(permissions.permit_withdraw(caller).is_ok());
    assert!(permissions.permit_cancel().is_ok());
    assert!(permissions.permit_settling().is_ok());
    assert!(permissions.permit_add_trading_pair().is_ok());
    assert!(permissions.permit_admin().is_ok());
}
