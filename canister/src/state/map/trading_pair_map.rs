use crate::order::{OrderBookId, TradingPair};
use bimap::BiBTreeMap;

/// Bidirectional map between [`TradingPair`] and [`OrderBookId`].
///
/// Maintains a 1:1 mapping so that lookups in either direction are
/// O(log n). Wraps [`bimap::BiBTreeMap`] to keep the dependency private.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TradingPairMap(BiBTreeMap<TradingPair, OrderBookId>);

impl TradingPairMap {
    /// Inserts a new pair ↔ book_id mapping.
    ///
    /// # Panics
    ///
    /// Panics if either `pair` or `book_id` is already present.
    pub fn insert(&mut self, pair: TradingPair, book_id: OrderBookId) {
        assert_eq!(
            self.0.insert_no_overwrite(pair, book_id),
            Ok(()),
            "BUG: duplicate trading pair or book ID"
        );
    }

    /// Returns `true` if the map contains the given trading pair.
    pub fn contains(&self, pair: &TradingPair) -> bool {
        self.0.contains_left(pair)
    }

    /// Looks up the [`OrderBookId`] for a trading pair.
    pub fn get_book_id(&self, pair: &TradingPair) -> Option<&OrderBookId> {
        self.0.get_by_left(pair)
    }

    /// Looks up the [`TradingPair`] for an order book ID.
    pub fn get_pair(&self, book_id: &OrderBookId) -> Option<&TradingPair> {
        self.0.get_by_right(book_id)
    }

    /// Iterates over all `(TradingPair, OrderBookId)` entries.
    pub fn iter(&self) -> impl Iterator<Item = (&TradingPair, &OrderBookId)> {
        self.0.iter()
    }

    /// Returns the number of trading pairs in the map.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the map contains no trading pairs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
