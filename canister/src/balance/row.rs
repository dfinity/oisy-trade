use super::{Balance, BalanceKey};
use ic_stable_structures::{Memory, StableBTreeMap};

/// A `(token, user)` balance row read out of the balance map for modification,
/// remembering whether the map held it so
/// [`worth_persisting`](Self::worth_persisting) can elide rows that were never
/// there and are still empty.
pub(super) struct BalanceRow {
    pub(super) balance: Balance,
    pub(super) existed: bool,
}

impl BalanceRow {
    /// Read `key`, standing in an empty balance when the map has no such row.
    pub(super) fn load<M: Memory>(
        balances: &StableBTreeMap<BalanceKey, Balance, M>,
        key: &BalanceKey,
    ) -> Self {
        let stored = balances.get(key);
        Self {
            existed: stored.is_some(),
            balance: stored.unwrap_or_default(),
        }
    }

    /// A row is worth persisting iff it existed before the modification or now
    /// holds a non-zero balance; empty never-existed rows are elided, so no-op
    /// mutations (e.g. `deposit(ZERO)`) don't materialise empty `(0, 0)` rows.
    pub(super) fn worth_persisting(&self) -> bool {
        self.existed || !self.balance.is_zero()
    }
}
