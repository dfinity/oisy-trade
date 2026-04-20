use super::{Balance, BalanceKey, InsufficientBalanceError};
use crate::order::{Quantity, TokenId};
use candid::Principal;
use ic_stable_structures::{Memory, StableBTreeMap};

/// Per-token balance ledger, stored in an `ic_stable_structures::StableBTreeMap`
/// keyed by `(TokenId, Principal)` (see [`BalanceKey`]).
pub struct TokenBalance<M: Memory> {
    balances: StableBTreeMap<BalanceKey, Balance, M>,
}

impl<M: Memory> std::fmt::Debug for TokenBalance<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenBalance")
            .field("len", &self.balances.len())
            .finish()
    }
}

impl<M: Memory> TokenBalance<M> {
    pub fn new(memory: M) -> Self {
        Self {
            balances: StableBTreeMap::init(memory),
        }
    }

    /// Read a user's full balance for a given token.
    pub fn get_balance(&self, user: &Principal, token: &TokenId) -> Option<Balance> {
        bench_scopes!("balances", "balances::get_balance");
        self.balances.get(&BalanceKey::new(*token, *user))
    }

    /// Deposit `amount` into a user's free balance for the given token.
    /// Creates the entry if absent.
    pub fn deposit(&mut self, user: Principal, token: TokenId, amount: Quantity) {
        bench_scopes!("balances", "balances::deposit");
        self.update(user, token, |b| b.deposit(amount));
    }

    /// Withdraw `amount` from a user's free balance for the given token.
    /// Returns `Err` if the entry is absent or the free balance is
    /// insufficient; on error the stored balance is untouched.
    pub fn withdraw(
        &mut self,
        user: &Principal,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        bench_scopes!("balances", "balances::withdraw");
        // `Balance::default()` has `free = 0`, so an absent entry flows
        // through `try_update` → `Balance::withdraw` →
        // `InsufficientBalanceError { available: 0, required: amount }`.
        // `try_update` does not persist on `Err`, so no empty row is created.
        self.try_update(*user, *token, |b| b.withdraw(amount))
    }

    /// Reserve `amount` from a user's free balance for the given token.
    /// Returns `Err` with the available balance when the entry is absent or
    /// the free balance is insufficient; on error the stored balance is
    /// untouched.
    pub fn reserve(
        &mut self,
        user: &Principal,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        bench_scopes!("balances", "balances::reserve");
        self.try_update(*user, *token, |b| b.reserve(amount))
    }

    /// Move `amount` from a user's reserved back to their free balance for
    /// the given token.
    ///
    /// # Panics
    ///
    /// Panics if the user has no balance entry for the token, or if the
    /// reserved balance is insufficient.
    pub fn unreserve(&mut self, user: &Principal, token: &TokenId, amount: Quantity) {
        bench_scopes!("balances", "balances::unreserve");
        let key = BalanceKey::new(*token, *user);
        let mut balance = self
            .balances
            .get(&key)
            .expect("BUG: user balance missing for unreserve");
        balance.unreserve(amount);
        self.balances.insert(key, balance);
    }

    /// Move `amount` from a debtor's reserved to a creditor's free balance
    /// for the given token. Creates the creditor entry if absent.
    ///
    /// # Panics
    ///
    /// Panics if the debtor has no balance entry, or if the debtor's
    /// reserved balance is insufficient.
    pub fn transfer(
        &mut self,
        debtor: &Principal,
        creditor: &Principal,
        token: &TokenId,
        amount: Quantity,
    ) {
        bench_scopes!("balances", "balances::transfer");
        let debtor_key = BalanceKey::new(*token, *debtor);
        let mut debtor_balance = self
            .balances
            .get(&debtor_key)
            .expect("BUG: debtor balance missing");
        debtor_balance.debit_reserved(&amount);
        self.balances.insert(debtor_key, debtor_balance);

        // Self-transfer: debtor and creditor are the same user, so the credit
        // must land on the just-updated balance — re-read before depositing
        // to avoid clobbering the debit we just persisted.
        self.update(*creditor, *token, |b| b.deposit(amount));
    }

    /// Read-modify-write for an infallible mutation. Creates the entry if
    /// absent. Skips the write when the mutation left the balance at the
    /// default and no entry existed, so no-op closures (e.g. `deposit(ZERO)`)
    /// don't materialise empty `(0, 0)` rows.
    fn update<F: FnOnce(&mut Balance)>(&mut self, user: Principal, token: TokenId, f: F) {
        let key = BalanceKey::new(token, user);
        let existed = self.balances.contains_key(&key);
        let mut balance = self.balances.get(&key).unwrap_or_default();
        f(&mut balance);
        if existed || balance != Balance::default() {
            self.balances.insert(key, balance);
        }
    }

    /// Read-modify-write for a fallible mutation. On `Err(_)` the stored
    /// entry is left untouched; on `Ok(_)` the updated value is persisted.
    fn try_update<F, T, E>(&mut self, user: Principal, token: TokenId, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut Balance) -> Result<T, E>,
    {
        let key = BalanceKey::new(token, user);
        let mut balance = self.balances.get(&key).unwrap_or_default();
        match f(&mut balance) {
            Ok(v) => {
                self.balances.insert(key, balance);
                Ok(v)
            }
            Err(e) => Err(e),
        }
    }

    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = (BalanceKey, Balance)> + '_ {
        self.balances
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }
}

// Tests that compare full `State` values clone the ledger and assert
// equality. Production code uses `TokenBalance<VMem>` (stable memory, not
// cloneable); tests use `TokenBalance<VectorMemory>`. Gate these impls on
// `cfg(test)` so they don't exist in release builds.
#[cfg(test)]
impl Clone for TokenBalance<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(ic_stable_structures::VectorMemory::default());
        for (key, balance) in self.iter() {
            fresh.balances.insert(key, balance);
        }
        fresh
    }
}

#[cfg(test)]
impl PartialEq for TokenBalance<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
impl Eq for TokenBalance<ic_stable_structures::VectorMemory> {}

#[cfg(test)]
impl Default for TokenBalance<ic_stable_structures::VectorMemory> {
    fn default() -> Self {
        Self::new(ic_stable_structures::VectorMemory::default())
    }
}
