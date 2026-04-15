use crate::order::{OrderBookId, TradingPair};
use bimap::BiBTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TradingPairMap(BiBTreeMap<TradingPair, OrderBookId>);

impl TradingPairMap {
    pub fn insert(&mut self, pair: TradingPair, book_id: OrderBookId) {
        assert_eq!(
            self.0.insert_no_overwrite(pair, book_id),
            Ok(()),
            "BUG: duplicate trading pair or book ID"
        );
    }

    pub fn contains(&self, pair: &TradingPair) -> bool {
        self.0.contains_left(pair)
    }

    pub fn get_book_id(&self, pair: &TradingPair) -> Option<&OrderBookId> {
        self.0.get_by_left(pair)
    }

    pub fn get_pair(&self, book_id: &OrderBookId) -> Option<&TradingPair> {
        self.0.get_by_right(book_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TradingPair, &OrderBookId)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
