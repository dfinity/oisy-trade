mod trading_pair {
    use crate::order::{OrderBookId, TokenId, TradingPair};
    use crate::state::TradingPairMap;
    use candid::Principal;

    #[test]
    fn should_insert_and_lookup_by_pair() {
        let mut map = TradingPairMap::default();
        let p = pair(1, 2);
        let book_id = OrderBookId::new(0);
        map.insert(p.clone(), book_id);

        assert_eq!(map.get_book_id(&p), Some(&book_id));
        assert_eq!(map.get_pair(&book_id), Some(&p));
    }

    #[test]
    fn should_report_contains() {
        let mut map = TradingPairMap::default();
        let p = pair(1, 2);
        assert!(!map.contains(&p));

        map.insert(p.clone(), OrderBookId::new(0));
        assert!(map.contains(&p));
    }

    #[test]
    fn should_return_none_for_unknown_entries() {
        let map = TradingPairMap::default();
        assert_eq!(map.get_book_id(&pair(1, 2)), None);
        assert_eq!(map.get_pair(&OrderBookId::new(0)), None);
    }

    #[test]
    fn should_track_len_and_is_empty() {
        let mut map = TradingPairMap::default();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);

        map.insert(pair(1, 2), OrderBookId::new(0));
        assert!(!map.is_empty());
        assert_eq!(map.len(), 1);

        map.insert(pair(3, 4), OrderBookId::new(1));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn should_iterate_all_entries() {
        let mut map = TradingPairMap::default();
        map.insert(pair(1, 2), OrderBookId::new(0));
        map.insert(pair(3, 4), OrderBookId::new(1));

        let entries: Vec<_> = map.iter().collect();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    #[should_panic(expected = "BUG: duplicate trading pair or book ID")]
    fn should_panic_on_duplicate_pair() {
        let mut map = TradingPairMap::default();
        map.insert(pair(1, 2), OrderBookId::new(0));
        map.insert(pair(1, 2), OrderBookId::new(1));
    }

    #[test]
    #[should_panic(expected = "BUG: duplicate trading pair or book ID")]
    fn should_panic_on_duplicate_book_id() {
        let mut map = TradingPairMap::default();
        map.insert(pair(1, 2), OrderBookId::new(0));
        map.insert(pair(3, 4), OrderBookId::new(0));
    }

    fn pair(base: u8, quote: u8) -> TradingPair {
        TradingPair {
            base: TokenId::new(Principal::from_slice(&[base])),
            quote: TokenId::new(Principal::from_slice(&[quote])),
        }
    }
}
