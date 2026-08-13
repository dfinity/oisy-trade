use super::{OrderId, OrderStatus, Price, Quantity, Side, TimeInForce};
use crate::Timestamp;
use crate::history::{CursorNotFound, History, InsertionSeq};
use crate::user::UserId;
use candid::Principal;
use ic_stable_structures::Memory;

#[cfg(test)]
mod tests;

/// Per-order data, from submission through terminal state.
///
/// Once the canister is launched its CBOR layout is an upgrade-durable schema:
/// removing or renumbering a field — or adding one without an `Option<T>` /
/// `#[cbor(default)]` fallback — breaks decoding of records written by prior
/// versions. Pre-launch there are no persisted records, so schema-breaking
/// changes are acceptable.
/// The trading pair is deliberately not stored — it is derivable from the
/// `OrderBookId` embedded in the [`OrderId`].
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct OrderRecord {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub owner: Principal,
    #[n(1)]
    pub side: Side,
    #[n(2)]
    pub price: Price,
    #[n(3)]
    pub quantity: Quantity,
    #[n(4)]
    pub status: OrderStatus,
    /// Submission time, taken from the add-limit-order event. Display-only —
    /// no matching or ordering logic reads it.
    #[n(5)]
    pub created_at: Timestamp,
    /// Cumulative quantity filled so far, in base token units. Remaining is
    /// `quantity − filled_quantity`.
    #[n(6)]
    pub filled_quantity: Quantity,
    /// Time of the most recent modifying event (fill, status transition, or
    /// cancel); `None` until the order is first modified.
    #[n(7)]
    pub last_updated_at: Option<Timestamp>,
    /// Time-in-force policy the order was placed with.
    #[n(8)]
    pub time_in_force: TimeInForce,
    /// Cumulative realized quote notional transacted across the order's fills,
    /// `Σ (maker_price × fill_quantity / base_scale)`. Always quote-denominated;
    /// a buy taker's released reservation surplus is excluded.
    #[n(9)]
    pub filled_quote: Quantity,
    /// Cumulative realized fee charged across the order's fills, denominated in
    /// the order's receive token — base for a buy, quote for a sell.
    #[n(10)]
    pub filled_fee: Quantity,
    /// The acting caller when it differs from `owner`; `None` when the owner
    /// placed the order itself.
    #[cbor(n(11), with = "icrc_cbor::principal::option")]
    pub placed_by: Option<Principal>,
    /// The acting caller that canceled the order, when it differs from `owner`.
    /// `None` if the order is not canceled, or was canceled by the owner itself.
    #[cbor(n(12), with = "icrc_cbor::principal::option")]
    pub canceled_by: Option<Principal>,
}

impl From<OrderRecord> for oisy_trade_types::OrderRecord {
    fn from(record: OrderRecord) -> Self {
        oisy_trade_types::OrderRecord {
            owner: record.owner,
            side: record.side.into(),
            price: candid::Nat::from(record.price),
            quantity: record.quantity.into(),
            filled_quantity: record.filled_quantity.into(),
            status: record.status.into(),
            created_at: record.created_at.as_nanos(),
            last_updated_at: record.last_updated_at.map(|t| t.as_nanos()),
            time_in_force: record.time_in_force.into(),
            filled_quote: record.filled_quote.into(),
            filled_fee: record.filled_fee.into(),
            placed_by: record.placed_by,
            canceled_by: record.canceled_by,
        }
    }
}

/// A combined update to an order record, applied in a single read-modify-write
/// by [`OrderHistory::apply_update`]: an optional status transition plus the
/// fill, quote, and fee deltas to add to `filled_quantity`, `filled_quote`, and
/// `filled_fee`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrderUpdate {
    pub status: Option<OrderStatus>,
    pub filled_delta: Quantity,
    pub quote_delta: Quantity,
    pub fee_delta: Quantity,
    pub canceled_by: Option<Principal>,
}

impl OrderUpdate {
    /// A status-only update (no fill delta).
    pub fn status(status: OrderStatus) -> Self {
        Self {
            status: Some(status),
            filled_delta: Quantity::ZERO,
            quote_delta: Quantity::ZERO,
            fee_delta: Quantity::ZERO,
            canceled_by: None,
        }
    }

    /// A cancel transition to [`OrderStatus::Canceled`] recording the acting
    /// caller (`None` when the owner canceled the order itself).
    pub fn cancel(canceled_by: Option<Principal>) -> Self {
        Self {
            status: Some(OrderStatus::Canceled),
            filled_delta: Quantity::ZERO,
            quote_delta: Quantity::ZERO,
            fee_delta: Quantity::ZERO,
            canceled_by,
        }
    }

    /// A fill-only update (no status change).
    pub fn filled(filled_delta: Quantity) -> Self {
        Self {
            status: None,
            filled_delta,
            quote_delta: Quantity::ZERO,
            fee_delta: Quantity::ZERO,
            canceled_by: None,
        }
    }

    /// Apply the update to the order record. Returns whether the record was changed.
    ///
    /// # Panics
    ///
    /// `filled_quantity` is monotonic non-decreasing and must never exceed
    /// `quantity`; `filled_quote` and `filled_fee` are monotonic
    /// non-decreasing. These invariants are enforced by always-on checks that
    /// trap on violation.
    pub fn apply(self, order: &mut OrderRecord) -> bool {
        let mut changed = false;
        let OrderUpdate {
            status,
            filled_delta,
            quote_delta,
            fee_delta,
            canceled_by,
        } = self;

        if let Some(new_status) = status
            && new_status != order.status
        {
            changed = true;
            order.status = new_status;
            if new_status == OrderStatus::Canceled {
                order.canceled_by = canceled_by;
            }
        }

        if filled_delta != Quantity::ZERO {
            changed = true;
            order.filled_quantity = order
                .filled_quantity
                .checked_add(filled_delta)
                .expect("BUG: filled_quantity overflow");

            assert!(
                order.filled_quantity <= order.quantity,
                "BUG: filled_quantity {:?} exceeds quantity {:?} for order created_at {:?}",
                order.filled_quantity,
                order.quantity,
                order.created_at,
            );
        }

        if quote_delta != Quantity::ZERO {
            changed = true;
            order.filled_quote = order
                .filled_quote
                .checked_add(quote_delta)
                .expect("BUG: filled_quote overflow");
        }

        if fee_delta != Quantity::ZERO {
            changed = true;
            order.filled_fee = order
                .filled_fee
                .checked_add(fee_delta)
                .expect("BUG: filled_fee overflow");
        }
        changed
    }
}

/// Record of every order from submission through terminal state, built on the
/// shared [`History`] core keyed by [`OrderId`]. Each order is inserted once and
/// thereafter its record may be updated in place (status/fill) via
/// [`OrderHistory::apply_update`]; the per-user index entry is immutable.
pub struct OrderHistory<M: Memory>(History<M, OrderId, OrderRecord>);

impl<M: Memory> std::fmt::Debug for OrderHistory<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrderHistory")
            .field("len", &self.0.len())
            .finish()
    }
}

impl<M: Memory> OrderHistory<M> {
    /// `orders_memory` and `by_user_memory` **must be distinct memory regions**:
    /// the two maps share no isolation beyond their backing memory, so passing
    /// the same handle twice would let them overwrite each other.
    pub fn new(orders_memory: M, by_user_memory: M) -> Self {
        Self(History::new(orders_memory, by_user_memory))
    }

    /// Insert a new order record and index it under `user`. Panics if the order
    /// ID is present.
    pub fn insert_once(&mut self, user: UserId, id: OrderId, record: OrderRecord) {
        bench_scopes!("order_history", "order_history::insert_once");
        self.0.insert_once(user, id, record);
    }

    /// Returns a copy of the record for the given order, or `None` if absent.
    pub fn get(&self, id: &OrderId) -> Option<OrderRecord> {
        bench_scopes!("order_history", "order_history::get");
        self.0.get(id)
    }

    /// Applies a combined [`OrderUpdate`] to an existing order in a single
    /// read-modify-write: sets the status if present, adds the deltas to the
    /// cumulative fields, and stamps `last_updated_at = Some(now)`. A no-op
    /// update writes nothing and leaves `last_updated_at` unchanged.
    pub fn apply_update(&mut self, id: &OrderId, update: OrderUpdate, now: Timestamp) {
        bench_scopes!("order_history", "order_history::apply_update");
        self.0.modify(id, |record| {
            let changed = update.apply(record);
            if changed {
                record.last_updated_at = Some(now);
            }
            changed
        });
    }

    /// Returns up to `length` of `user`'s orders in newest-first order. With
    /// `after: None` the page starts at the newest order; otherwise `after` is
    /// a cursor — the last order of the previous page — and the page continues
    /// with the next-older order. An `after` that names an unknown order — or
    /// one that does not belong to `user` — yields [`CursorNotFound`]; a valid
    /// cursor with no older orders is `Ok(vec![])`. Each page is an `O(length)`
    /// range scan from the cursor.
    pub fn orders_after(
        &self,
        user: UserId,
        after: Option<OrderId>,
        length: usize,
    ) -> Result<Vec<OrderId>, CursorNotFound> {
        bench_scopes!("order_history", "order_history::orders_after");
        self.0.page_by_user(user, after, length)
    }

    /// Iterates every order record as `(order id, record)` in key order.
    pub fn iter(&self) -> impl Iterator<Item = (OrderId, OrderRecord)> + '_ {
        self.0.iter_primary()
    }

    /// Iterates the per-user order index as `(user, insertion sequence, order
    /// id)` in index order.
    pub fn iter_by_user(&self) -> impl Iterator<Item = (UserId, InsertionSeq, OrderId)> + '_ {
        self.0.iter_by_user()
    }
}

#[cfg(test)]
impl Clone for OrderHistory<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[cfg(test)]
impl PartialEq for OrderHistory<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[cfg(test)]
impl Eq for OrderHistory<ic_stable_structures::VectorMemory> {}
