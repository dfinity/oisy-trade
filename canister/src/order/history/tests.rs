use super::*;

/// `UserOrderKey`'s derived `Ord` must agree with its `Storable` byte order,
/// since `StableBTreeMap` relies on that consistency for range scans.
#[test]
fn user_order_key_ord_matches_storable_bytes() {
    let keys = [
        UserOrderKey::from_seq(UserId::new(2), 0),
        UserOrderKey::from_seq(UserId::new(1), 0),
        UserOrderKey::from_seq(UserId::new(1), 5),
        UserOrderKey::from_seq(UserId::new(1), 9),
        UserOrderKey::newest(UserId::new(0)),
        UserOrderKey::oldest(UserId::new(0)),
    ];
    for a in &keys {
        for b in &keys {
            assert_eq!(
                a.cmp(b),
                a.to_bytes().cmp(&b.to_bytes()),
                "Ord disagrees with Storable bytes for {a:?} vs {b:?}"
            );
        }
        assert_eq!(
            UserOrderKey::from_bytes(a.to_bytes()),
            *a,
            "Storable round-trip mismatch for {a:?}"
        );
    }
}
