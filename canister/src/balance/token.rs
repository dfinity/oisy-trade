use super::fee_pool::{FeeEntry, FeePool};
use super::write_back::BalanceWriteBack;
use super::{Balance, BalanceKey, InsufficientBalanceError};
use crate::order::{Quantity, TokenId};
use crate::user::UserId;
use ic_stable_structures::{Memory, StableBTreeMap};

/// Canister-wide token accounting:
///
/// - Per-`(token, user)` `Balance` entries in a stable [`StableBTreeMap`]
///   (auto-survives upgrades via the memory ID).
/// - A [`FeePool`] on the heap, accrued by fills via
///   [`BalanceWriteBack::transfer`] and persisted across upgrades through the
///   [`fee_pool_snapshot`](Self::fee_pool_snapshot) /
///   [`restore_fee_pool`](Self::restore_fee_pool) pair plumbed through
///   `StateSnapshot`.
///
/// All token-conservation operations route through this type, so the
/// canister-wide invariant
///
/// ```text
/// for each token: Σ users(free + reserved) + fee_pool[token]
///                  = Σ deposits − Σ withdrawals
/// ```
///
/// is enforceable at one API boundary.
pub struct TokenBalance<M: Memory> {
    balances: StableBTreeMap<BalanceKey, Balance, M>,
    fee_pool: FeePool,
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
            fee_pool: FeePool::default(),
        }
    }

    /// Read a user's full balance for a given token.
    pub fn get_balance(&self, user: UserId, token: &TokenId) -> Option<Balance> {
        bench_scopes!("balances", "balances::get_balance");
        self.balances.get(&BalanceKey::new(*token, user))
    }

    /// Deposit `amount` into a user's free balance for the given token.
    /// Creates the entry if absent.
    pub fn deposit(&mut self, user: UserId, token: TokenId, amount: Quantity) {
        self.with_write_back(|balances| balances.deposit(user, &token, amount));
    }

    /// Withdraw `amount` from a user's free balance for the given token.
    /// Returns `Err` if the entry is absent or the free balance is
    /// insufficient; on error the stored balance is untouched.
    pub fn withdraw(
        &mut self,
        user: UserId,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        self.with_write_back(|balances| balances.withdraw(user, token, amount))
    }

    /// Reserve `amount` from a user's free balance for the given token.
    /// Returns `Err` with the available balance when the entry is absent or
    /// the free balance is insufficient; on error the stored balance is
    /// untouched.
    pub fn reserve(
        &mut self,
        user: UserId,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        self.with_write_back(|balances| balances.reserve(user, token, amount))
    }

    /// Apply `operations` to a write-back buffer over the balance map — one
    /// settling event's worth of fills, or a single deposit — and flush it
    /// before returning. Each `(token, user)` row the operations touch is read
    /// from the stable map at most once and written back at most once.
    ///
    /// The buffer is only ever lent out, so no caller can drop it unflushed
    /// and silently discard the operations it applied.
    pub(crate) fn with_write_back<R>(
        &mut self,
        operations: impl FnOnce(&mut BalanceWriteBack<'_, M>) -> R,
    ) -> R {
        let mut write_back = BalanceWriteBack::new(&mut self.balances, &mut self.fee_pool);
        let result = operations(&mut write_back);
        write_back.flush();
        result
    }

    /// Read the accumulated fee balance for `token`. `None` if no fees have
    /// ever been accrued for this token.
    pub fn fee_balance(&self, token: &TokenId) -> Option<Quantity> {
        self.fee_pool.accrued(token)
    }

    /// Iterate the fee pool. Order is by `TokenId` (BTreeMap ordering).
    pub fn iter_fee_balances(&self) -> impl Iterator<Item = (TokenId, Quantity)> + '_ {
        self.fee_pool.iter()
    }

    /// Snapshot the heap-resident fee pool for pre-upgrade serialization.
    /// Stable user balances are excluded; they survive upgrades on their
    /// own via the underlying [`StableBTreeMap`].
    pub fn fee_pool_snapshot(&self) -> Vec<FeeEntry> {
        self.fee_pool.snapshot()
    }

    /// Restore the heap-resident fee pool after a `post_upgrade` decode.
    /// Replaces any existing pool; intended to run exactly once during
    /// post-upgrade. Duplicate `TokenId` entries in `snapshot` trap.
    pub fn restore_fee_pool(&mut self, snapshot: Vec<FeeEntry>) {
        self.fee_pool.restore(snapshot);
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
        fresh.fee_pool = self.fee_pool.clone();
        fresh
    }
}

#[cfg(test)]
impl PartialEq for TokenBalance<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter()) && self.fee_pool == other.fee_pool
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
