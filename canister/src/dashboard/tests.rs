use super::saturating_to_u128;
use crate::order::Quantity;

#[test]
fn should_saturate_quantity_to_u128() {
    assert_eq!(saturating_to_u128(&Quantity::ZERO), 0);
    assert_eq!(saturating_to_u128(&Quantity::from(1u64)), 1);
    assert_eq!(
        saturating_to_u128(&Quantity::from(u64::MAX)),
        u128::from(u64::MAX)
    );
    assert_eq!(
        saturating_to_u128(&Quantity::from_u128(u128::MAX)),
        u128::MAX
    );
    assert_eq!(saturating_to_u128(&Quantity::new(1, 0)), u128::MAX);
    assert_eq!(saturating_to_u128(&Quantity::MAX), u128::MAX);
}
