mod fill_id {
    use crate::order::{FillId, FillIdParseError, FillSeq, OrderBookId};
    use ic_stable_structures::Storable;
    use proptest::prelude::{Strategy, any, prop_assert, prop_assert_eq, proptest};

    fn arb_fill_id() -> impl Strategy<Value = FillId> {
        (any::<u64>(), any::<u64>())
            .prop_map(|(book, seq)| FillId::new(OrderBookId::new(book), FillSeq::new(seq)))
    }

    proptest! {
        #[test]
        fn should_roundtrip_through_display_and_parse(book: u64, seq: u64) {
            let id = FillId::new(OrderBookId::new(book), FillSeq::new(seq));
            let parsed: FillId = id.to_string().parse().unwrap();
            prop_assert_eq!(parsed, id);
        }

        #[test]
        fn should_always_encode_as_32_char_hex(book: u64, seq: u64) {
            let id = FillId::new(OrderBookId::new(book), FillSeq::new(seq));
            let s = id.to_string();
            prop_assert_eq!(s.len(), 32);
            prop_assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
        }

        #[test]
        fn should_reject_wrong_length(s in ".{0,31}|.{33,64}") {
            prop_assert_eq!(s.parse::<FillId>(), Err(FillIdParseError));
        }

        #[test]
        fn should_reject_non_hex(s in "[^0-9a-fA-F]{32}") {
            prop_assert_eq!(s.parse::<FillId>(), Err(FillIdParseError));
        }

        #[test]
        fn should_roundtrip_through_stable_bytes(id in arb_fill_id()) {
            let bytes = id.to_bytes();
            prop_assert_eq!(FillId::from_bytes(bytes), id);
        }

        #[test]
        fn should_preserve_order_under_be_encoding(
            a in any::<(u64, u64)>(),
            b in any::<(u64, u64)>(),
        ) {
            let id_a = FillId::new(OrderBookId::new(a.0), FillSeq::new(a.1));
            let id_b = FillId::new(OrderBookId::new(b.0), FillSeq::new(b.1));
            prop_assert_eq!(id_a.cmp(&id_b), id_a.to_bytes().cmp(&id_b.to_bytes()));
        }
    }
}

mod trade_id {
    use crate::order::{FillId, FillSeq, OrderBookId, OrderId, OrderSeq, TradeId};
    use ic_stable_structures::Storable;
    use proptest::prelude::{Strategy, any, prop_assert_eq, proptest};

    fn arb_trade_id() -> impl Strategy<Value = TradeId> {
        (any::<u64>(), any::<u64>(), any::<u64>()).prop_map(|(book, order_seq, fill_seq)| {
            TradeId::new(
                OrderId::new(OrderBookId::new(book), OrderSeq::new(order_seq)),
                FillSeq::new(fill_seq),
            )
        })
    }

    proptest! {
        #[test]
        fn should_roundtrip_through_stable_bytes(id in arb_trade_id()) {
            let bytes = id.to_bytes();
            prop_assert_eq!(TradeId::from_bytes(bytes), id);
        }

        #[test]
        fn should_preserve_order_under_be_encoding(a in arb_trade_id(), b in arb_trade_id()) {
            prop_assert_eq!(a.cmp(&b), a.to_bytes().cmp(&b.to_bytes()));
        }

        #[test]
        fn should_derive_fill_id_from_owning_book_and_shared_seq(book: u64, order_seq: u64, fill_seq: u64) {
            let order = OrderId::new(OrderBookId::new(book), OrderSeq::new(order_seq));
            let trade = TradeId::new(order, FillSeq::new(fill_seq));
            prop_assert_eq!(trade.order_id(), order);
            prop_assert_eq!(trade.seq(), FillSeq::new(fill_seq));
            prop_assert_eq!(trade.fill_id(), FillId::new(OrderBookId::new(book), FillSeq::new(fill_seq)));
        }
    }
}
