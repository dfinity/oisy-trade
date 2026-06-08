use super::UserId;
use crate::test_fixtures::{principal, user_registry};

#[test]
fn get_or_register_is_stable_and_dense() {
    let mut registry = user_registry();
    // First-seen principals get dense ids in insertion order.
    assert_eq!(registry.get_or_register(principal(1)), UserId::new(0));
    assert_eq!(registry.get_or_register(principal(2)), UserId::new(1));
    assert_eq!(registry.get_or_register(principal(3)), UserId::new(2));
    // Re-registering returns the same id.
    assert_eq!(registry.get_or_register(principal(1)), UserId::new(0));
    assert_eq!(registry.get_or_register(principal(2)), UserId::new(1));
}

#[test]
fn lookup_does_not_assign() {
    let mut registry = user_registry();
    assert_eq!(registry.lookup(principal(1)), None);
    let id = registry.get_or_register(principal(1));
    assert_eq!(registry.lookup(principal(1)), Some(id));
    // A never-registered principal still has no id, and looking it up didn't
    // assign one (the next registration is still 1, not 2).
    assert_eq!(registry.lookup(principal(9)), None);
    assert_eq!(registry.get_or_register(principal(2)), UserId::new(1));
}

mod storable {
    use crate::test_fixtures::arbitrary::arb_user_id;
    use crate::user::UserId;
    use ic_stable_structures::Storable;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn should_roundtrip_through_stable_bytes(id in arb_user_id()) {
            prop_assert_eq!(UserId::from_bytes(id.to_bytes()), id);
        }
    }
}
