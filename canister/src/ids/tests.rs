use crate::ids::{CompositeId, FixedWidthId, Seq, SeqMarker};
use minicbor::{Decode, Encode};
use proptest::arbitrary::any;
use proptest::prelude::TestCaseError;
use proptest::prop_assert_eq;
use proptest::strategy::Strategy;

mod seq {
    use crate::ids::tests::{
        SeqTest, arb_seq_test, check_fixed_size, check_hex_roundtrip, check_minicbor_roundtrip,
    };
    use proptest::proptest;

    #[test]
    fn should_have_debug_representation() {
        let seq = SeqTest::new(42);
        let dbg = format!("{seq:?}");
        assert_eq!(dbg, "SeqTest(42)");
    }

    proptest! {
        #[test]
        fn should_encode_decode_minicbor(seq in arb_seq_test()) {
            check_minicbor_roundtrip(&seq)?;
        }

        #[test]
        fn should_have_fixed_size(seq in arb_seq_test()) {
            check_fixed_size::<_,8>(seq)?;
        }

        #[test]
        fn should_roundtrip_hex(seq in arb_seq_test()) {
            check_hex_roundtrip::<_,16>(seq)?;
        }
    }
}

mod composite {
    use crate::ids::ParseFixedWithIdError;
    use crate::ids::tests::{
        CompositeTest, arb_composite_test, check_fixed_size, check_hex_roundtrip,
        check_minicbor_roundtrip,
    };
    use proptest::proptest;

    #[test]
    fn should_reject_a_malformed_hex_id() {
        assert_eq!("".parse::<CompositeTest>(), Err(ParseFixedWithIdError {}));
        assert_eq!(
            "0".repeat(31).parse::<CompositeTest>(),
            Err(ParseFixedWithIdError {}),
            "too short"
        );
        assert_eq!(
            "0".repeat(33).parse::<CompositeTest>(),
            Err(ParseFixedWithIdError {}),
            "too long"
        );
        assert_eq!(
            "z".repeat(32).parse::<CompositeTest>(),
            Err(ParseFixedWithIdError {}),
            "non-hex"
        );
    }

    proptest! {
        #[test]
        fn should_encode_decode_minicbor(composite in arb_composite_test()) {
            check_minicbor_roundtrip(&composite)?;
        }

        #[test]
        fn should_have_fixed_size(composite in arb_composite_test()) {
            check_fixed_size::<_,16>(composite)?;
        }

        #[test]
        fn should_roundtrip_hex(composite in arb_composite_test()) {
            check_hex_roundtrip::<_,32>(composite)?;
        }
    }
}

mod nested {
    use crate::ids::tests::{
        arb_nested_test, check_fixed_size, check_hex_roundtrip, check_minicbor_roundtrip,
    };
    use proptest::proptest;

    proptest! {
        #[test]
        fn should_encode_decode_minicbor(nested in arb_nested_test()) {
            check_minicbor_roundtrip(&nested)?;
        }

        #[test]
        fn should_have_fixed_size(nested in arb_nested_test()) {
            check_fixed_size::<_,24>(nested)?;
        }

        #[test]
        fn should_roundtrip_hex(nested in arb_nested_test()) {
            check_hex_roundtrip::<_,48>(nested)?;
        }
    }
}

pub fn check_minicbor_roundtrip<T>(v: &T) -> Result<(), TestCaseError>
where
    for<'a> T: PartialEq + std::fmt::Debug + Encode<()> + Decode<'a, ()>,
{
    let mut buf = vec![];
    minicbor::encode(v, &mut buf).expect("encoding should succeed");
    let decoded = minicbor::decode(&buf).expect("decoding should succeed");
    prop_assert_eq!(v, &decoded);
    Ok(())
}

pub fn check_fixed_size<T: FixedWidthId + PartialEq + std::fmt::Debug, const N: usize>(
    id: T,
) -> Result<(), TestCaseError> {
    let mut bytes = Vec::with_capacity(N);
    id.write_be_bytes(&mut bytes);
    prop_assert_eq!(bytes.len(), N);

    let deser = T::from_be_bytes(&bytes).unwrap();
    prop_assert_eq!(id, deser);

    Ok(())
}

pub fn check_hex_roundtrip<T: FixedWidthId + PartialEq + std::fmt::Debug, const N: usize>(
    id: T,
) -> Result<(), TestCaseError> {
    struct Hex<'a, T>(&'a T);

    impl<T: FixedWidthId> std::fmt::Display for Hex<'_, T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.write_hex(f)
        }
    }

    let hex = format!("{}", Hex(&id));
    prop_assert_eq!(hex.len(), N);

    let deser = T::from_hex(&hex).unwrap();
    prop_assert_eq!(id, deser);

    Ok(())
}

struct SeqTestMarker;

impl SeqMarker for SeqTestMarker {
    const NAME: &'static str = "SeqTest";
}

type SeqTest = Seq<SeqTestMarker>;

struct TestIdMarker;

impl SeqMarker for TestIdMarker {
    const NAME: &'static str = "TestId";
}

type TestId = Seq<TestIdMarker>;

type CompositeTest = CompositeId<TestId, SeqTest>;

type NestedTest = CompositeId<CompositeTest, SeqTest>;

fn arb_seq<M: SeqMarker>() -> impl Strategy<Value = Seq<M>> {
    any::<u64>().prop_map(Seq::new)
}

fn arb_seq_test() -> impl Strategy<Value = SeqTest> {
    arb_seq()
}

fn arb_composite_test() -> impl Strategy<Value = CompositeTest> {
    (arb_seq::<TestIdMarker>(), arb_seq::<SeqTestMarker>())
        .prop_map(|(id, seq)| CompositeTest::new(id, seq))
}

fn arb_nested_test() -> impl Strategy<Value = NestedTest> {
    (arb_composite_test(), arb_seq::<SeqTestMarker>())
        .prop_map(|(id, seq)| NestedTest::new(id, seq))
}
