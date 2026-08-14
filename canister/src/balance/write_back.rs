use super::fee_pool::FeePool;
use super::row::BalanceRow;
use super::{Balance, BalanceKey, InsufficientBalanceError};
use crate::order::{Quantity, TokenId};
use crate::user::UserId;
use ic_stable_structures::{Memory, StableBTreeMap};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

/// In-heap write-back buffer over the balance map for one batch of balance
/// operations, lent out by [`TokenBalance::with_write_back`].
///
/// The taker of a large sweep is party to every fill, so its two balance rows
/// would otherwise be read-modify-written on each fill. The buffer collapses
/// that to a single stable read per row on first touch and a single write-back
/// per dirty row on [`flush`](Self::flush), while the fee pool keeps accruing
/// on the heap.
///
/// [`TokenBalance::with_write_back`]: super::TokenBalance::with_write_back
pub(crate) struct BalanceWriteBack<'a, M: Memory> {
    balances: &'a mut StableBTreeMap<BalanceKey, Balance, M>,
    fee_pool: &'a mut FeePool,
    buffer: BTreeMap<BalanceKey, BalanceRow>,
}

impl<'a, M: Memory> BalanceWriteBack<'a, M> {
    pub(super) fn new(
        balances: &'a mut StableBTreeMap<BalanceKey, Balance, M>,
        fee_pool: &'a mut FeePool,
    ) -> Self {
        Self {
            balances,
            fee_pool,
            buffer: BTreeMap::new(),
        }
    }

    /// Debits `gross` from the debtor's reserved, credits `gross - fee` to the
    /// creditor's free, and accrues `fee` to the token's fee pool. A
    /// self-transfer lands the credit on the just-debited buffered row.
    ///
    /// Conserves `gross` units of `token` across the per-token invariant
    /// `Σ users(free + reserved) + fee_pool = Σ deposits − Σ withdrawals`.
    ///
    /// # Panics
    ///
    /// - `fee > gross`, or the token's accrued fees overflow — see
    ///   [`FeePool::withhold`].
    /// - The debtor has no balance entry for `token`, or `gross` exceeds the
    ///   debtor's reserved balance.
    pub(crate) fn transfer(
        &mut self,
        debtor: UserId,
        creditor: UserId,
        token: &TokenId,
        gross: Quantity,
        fee: Quantity,
    ) {
        bench_scopes!("balances", "balances::transfer");
        let net = self.fee_pool.withhold(token, gross, fee);

        self.load_existing(
            BalanceKey::new(*token, debtor),
            "BUG: debtor balance missing",
        )
        .debit_reserved(&gross);
        self.load_or_create(BalanceKey::new(*token, creditor))
            .deposit(net);
    }

    /// Moves `amount` from the user's reserved to their free balance.
    ///
    /// # Panics
    ///
    /// Panics if the user has no balance entry for `token`, or if `amount`
    /// exceeds their reserved balance.
    pub(crate) fn unreserve(&mut self, user: UserId, token: &TokenId, amount: Quantity) {
        bench_scopes!("balances", "balances::unreserve");
        self.load_existing(
            BalanceKey::new(*token, user),
            "BUG: user balance missing for unreserve",
        )
        .unreserve(amount);
    }

    /// Credits `amount` to the user's free balance for `token`, creating the
    /// row on demand.
    pub(crate) fn deposit(&mut self, user: UserId, token: &TokenId, amount: Quantity) {
        bench_scopes!("balances", "balances::deposit");
        self.load_or_create(BalanceKey::new(*token, user))
            .deposit(amount);
    }

    /// Withdraws `amount` from the user's free balance for `token`. Returns
    /// `Err` with the available balance when the free balance is insufficient,
    /// leaving the stored balance untouched.
    pub(crate) fn withdraw(
        &mut self,
        user: UserId,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        bench_scopes!("balances", "balances::withdraw");
        self.try_mutate(BalanceKey::new(*token, user), |balance| {
            balance.withdraw(amount)
        })
    }

    /// Moves `amount` from the user's free to their reserved balance. Returns
    /// `Err` with the available balance when the free balance is insufficient,
    /// leaving the stored balance untouched.
    pub(crate) fn reserve(
        &mut self,
        user: UserId,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        bench_scopes!("balances", "balances::reserve");
        self.try_mutate(BalanceKey::new(*token, user), |balance| {
            balance.reserve(amount)
        })
    }

    /// Write each buffered row back to the stable map exactly once, eliding
    /// rows that are not [`worth_persisting`](BalanceRow::worth_persisting).
    pub(super) fn flush(self) {
        bench_scopes!("balances", "balances::flush");
        for (key, row) in self.buffer {
            if row.worth_persisting() {
                self.balances.insert(key, row.balance);
            }
        }
    }

    /// Buffer a row that a debit requires to exist: the debtor of
    /// [`transfer`](Self::transfer) and the target of
    /// [`unreserve`](Self::unreserve). Traps with `msg` if the stable map held
    /// no such row; a later touch reuses the buffered row without re-reading.
    fn load_existing(&mut self, key: BalanceKey, msg: &'static str) -> &mut Balance {
        let row = self
            .buffer
            .entry(key)
            .or_insert_with(|| BalanceRow::load(self.balances, &key));
        assert!(row.existed, "{msg}");
        &mut row.balance
    }

    /// Buffer a row that may not yet exist, as required by
    /// [`deposit`](Self::deposit) and by the creditor credit in
    /// [`transfer`](Self::transfer), which create the entry on demand.
    fn load_or_create(&mut self, key: BalanceKey) -> &mut Balance {
        let row = self
            .buffer
            .entry(key)
            .or_insert_with(|| BalanceRow::load(self.balances, &key));
        &mut row.balance
    }

    /// Buffer a row only once `mutate` has succeeded, so that a failed
    /// operation leaves the buffer — and hence the stable map — untouched. A
    /// row an earlier operation already buffered stays buffered either way,
    /// since that operation's own write-back is still owed.
    fn try_mutate<T, E>(
        &mut self,
        key: BalanceKey,
        mutate: impl FnOnce(&mut Balance) -> Result<T, E>,
    ) -> Result<T, E> {
        match self.buffer.entry(key) {
            Entry::Occupied(mut buffered) => mutate(&mut buffered.get_mut().balance),
            Entry::Vacant(slot) => {
                let mut row = BalanceRow::load(self.balances, &key);
                let outcome = mutate(&mut row.balance)?;
                slot.insert(row);
                Ok(outcome)
            }
        }
    }
}
