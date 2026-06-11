use proptest::prelude::*;
use std::num::NonZeroU64;

proptest! {
    #[test]
    fn should_roundtrip_non_zero_u64(value in 1..=u64::MAX) {
        let nz = NonZeroU64::new(value).unwrap();
        let mut buf = vec![];
        minicbor::encode(CborNonZeroU64(nz), &mut buf).unwrap();
        let decoded: CborNonZeroU64 = minicbor::decode(&buf).unwrap();
        prop_assert_eq!(nz, decoded.0);
    }
}

#[test]
fn should_fail_to_decode_zero() {
    let mut buf = vec![];
    minicbor::encode(0u64, &mut buf).unwrap();
    let result = minicbor::decode::<CborNonZeroU64>(&buf);
    assert!(result.is_err());
}

#[derive(minicbor::Encode, minicbor::Decode)]
#[cbor(transparent)]
struct CborNonZeroU64(#[cbor(n(0), with = "crate::cbor::non_zero_u64")] NonZeroU64);

proptest! {
    /// The `Quantity`-based u128 codec (used by `Price`/`TickSize`) round-trips
    /// any `u128` value.
    #[test]
    fn u128_via_quantity_roundtrips(v in any::<u128>()) {
        let mut buf = vec![];
        crate::cbor::u128_via_quantity::encode(&v, &mut minicbor::Encoder::new(&mut buf), &mut ())
            .unwrap();
        let decoded =
            crate::cbor::u128_via_quantity::decode(&mut minicbor::Decoder::new(&buf), &mut ())
                .unwrap();
        prop_assert_eq!(decoded, v);
    }
}
