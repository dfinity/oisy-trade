use crate::ids::{CompositeId, FixedWidthId, Seq, SeqMarker};
use proptest::arbitrary::any;
use proptest::prelude::TestCaseError;
use proptest::prop_assert_eq;
use proptest::strategy::Strategy;

mod seq {
    use crate::ids::tests::{SeqTest, arb_seq_test, check_fixed_size, check_hex_roundtrip};
    use crate::ids::{FixedWidthId, ParseFixedWithIdError};
    use crate::test_fixtures::arbitrary::check_minicbor_roundtrip;
    use proptest::proptest;

    #[test]
    fn should_have_debug_representation() {
        let seq = SeqTest::new(42);
        let dbg = format!("{seq:?}");
        assert_eq!(dbg, "SeqTest(42)");
    }

    #[test]
    fn should_reject_a_malformed_hex_id() {
        let valid = format!("{:016x}", 42_u64);
        assert_eq!(SeqTest::from_hex(&valid), Ok(SeqTest::new(42)));

        let too_short = "000000000000000";
        let too_long = "00000000000000000";
        let non_hex = "z000000000000000";
        let uppercase = "000000000000000A";
        let leading_plus = "+000000000000000";
        let non_ascii = "\u{00e9}".repeat(8);
        assert_eq!(non_ascii.len(), 16);

        for malformed in [
            too_short,
            too_long,
            non_hex,
            uppercase,
            leading_plus,
            non_ascii.as_str(),
        ] {
            assert_eq!(
                SeqTest::from_hex(malformed),
                Err(ParseFixedWithIdError {}),
                "expected {malformed:?} to be rejected",
            );
        }
    }

    #[test]
    fn should_reject_be_bytes_of_the_wrong_length() {
        assert_eq!(
            SeqTest::from_be_bytes(&[0u8; 7]),
            Err(ParseFixedWithIdError {})
        );
        assert_eq!(
            SeqTest::from_be_bytes(&[0u8; 9]),
            Err(ParseFixedWithIdError {})
        );
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
    use crate::ids::tests::{
        CompositeTest, SeqTest, TestId, arb_composite_test, check_fixed_size, check_hex_roundtrip,
    };
    use crate::ids::{FixedWidthId, ParseFixedWithIdError};
    use crate::test_fixtures::arbitrary::check_minicbor_roundtrip;
    use proptest::proptest;

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

    #[test]
    fn should_reject_a_malformed_hex_id() {
        let valid = format!("{:016x}{:016x}", 7_u64, 42_u64);
        assert_eq!(
            CompositeTest::from_hex(&valid),
            Ok(CompositeTest::new(TestId::new(7), SeqTest::new(42)))
        );

        let too_short = &valid[..31];
        let too_long = format!("{valid}0");
        let non_hex = format!("z{}", &valid[1..]);
        let uppercase = format!("{}A", &valid[..31]);
        let leading_plus = format!("+{}", &valid[1..]);
        let non_ascii = "\u{00e9}".repeat(16);
        assert_eq!(non_ascii.len(), 32);

        for malformed in [
            too_short,
            too_long.as_str(),
            non_hex.as_str(),
            uppercase.as_str(),
            leading_plus.as_str(),
            non_ascii.as_str(),
        ] {
            assert_eq!(
                CompositeTest::from_hex(malformed),
                Err(ParseFixedWithIdError {}),
                "expected {malformed:?} to be rejected",
            );
        }
    }

    #[test]
    fn should_reject_be_bytes_of_the_wrong_length() {
        assert_eq!(
            CompositeTest::from_be_bytes(&[0u8; 15]),
            Err(ParseFixedWithIdError {})
        );
        assert_eq!(
            CompositeTest::from_be_bytes(&[0u8; 17]),
            Err(ParseFixedWithIdError {})
        );
    }
}

mod nested {
    use crate::ids::tests::{arb_nested_test, check_fixed_size, check_hex_roundtrip};
    use crate::test_fixtures::arbitrary::check_minicbor_roundtrip;
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
