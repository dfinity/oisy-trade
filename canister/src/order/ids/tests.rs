mod seq {
    use crate::order::ids::Seq;
    use minicbor::{Decode, Encode};
    use proptest::arbitrary::any;
    use proptest::prelude::TestCaseError;
    use proptest::{prop_assert_eq, proptest};

    proptest! {
        #[test]
        fn u64_id_encoding_roundtrip(n in any::<u64>()) {
            check_roundtrip(&SeqTest::new(n))?;
        }
    }

    pub fn check_roundtrip<T>(v: &T) -> Result<(), TestCaseError>
    where
        for<'a> T: PartialEq + std::fmt::Debug + Encode<()> + Decode<'a, ()>,
    {
        let mut buf = vec![];
        minicbor::encode(v, &mut buf).expect("encoding should succeed");
        let decoded = minicbor::decode(&buf).expect("decoding should succeed");
        prop_assert_eq!(v, &decoded);
        Ok(())
    }

    struct SeqTestMarker;
    type SeqTest = Seq<SeqTestMarker>;
}
