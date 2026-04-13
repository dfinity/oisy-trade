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

#[derive(minicbor::Encode, minicbor::Decode)]
#[cbor(transparent)]
struct CborNonZeroU64(#[cbor(n(0), with = "crate::cbor::non_zero_u64")] NonZeroU64);
