use super::{Balance, BalanceKey, InsufficientBalanceError};
use crate::order::{Quantity, TokenId};
use crate::user::UserId;
use ic_stable_structures::{Memory, StableBTreeMap};
use std::collections::BTreeMap;

/// Canister-wide token accounting:
///
/// - Per-`(token, user)` `Balance` entries in a stable [`StableBTreeMap`]
///   (auto-survives upgrades via the memory ID).
/// - A heap-resident fee pool indexed by `TokenId`, accrued by fills via
///   [`TokenBalance::transfer`] and persisted across upgrades
///   through the [`fee_pool_snapshot`](Self::fee_pool_snapshot) /
///   [`restore_fee_pool`](Self::restore_fee_pool) pair plumbed through
///   `StateSnapshot`.
///
/// The fee pool lives on the heap because it is bounded by the number of
/// listed tokens (10s–100s), whereas user balances are unbounded and
/// belong in stable memory.
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
    fee_balances: BTreeMap<TokenId, Quantity>,
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
            fee_balances: BTreeMap::new(),
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
        bench_scopes!("balances", "balances::deposit");
        self.update(user, token, |b| b.deposit(amount));
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
        bench_scopes!("balances", "balances::withdraw");
        self.try_update(user, *token, |b| b.withdraw(amount))
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
        bench_scopes!("balances", "balances::reserve");
        self.try_update(user, *token, |b| b.reserve(amount))
    }

    /// Move `amount` from a user's reserved back to their free balance for
    /// the given token.
    ///
    /// # Panics
    ///
    /// Panics if the user has no balance entry for the token, or if the
    /// reserved balance is insufficient.
    pub fn unreserve(&mut self, user: UserId, token: &TokenId, amount: Quantity) {
        bench_scopes!("balances", "balances::unreserve");
        let key = BalanceKey::new(*token, user);
        let mut balance = self
            .balances
            .get(&key)
            .expect("BUG: user balance missing for unreserve");
        balance.unreserve(amount);
        self.balances.insert(key, balance);
    }

    /// Transfer `gross` from `debtor`'s reserved into `creditor`'s free,
    /// withholding `fee` for the canister-owned fee pool of `token`. The
    /// creditor receives exactly `gross - fee`; the fee pool gains `fee`.
    /// `fee = ZERO` is the non-fee case.
    ///
    /// Conserves `gross` units of `token` across the per-token invariant
    /// `Σ users(free + reserved) + fee_pool = Σ deposits − Σ withdrawals`.
    ///
    /// # Panics
    ///
    /// - `fee > gross` (preconditions are the caller's responsibility; the
    ///   per-pair `BasisPoint` invariant guarantees this at the fill path).
    /// - The debtor has no balance entry, or `gross` exceeds the debtor's
    ///   reserved balance.
    pub fn transfer(
        &mut self,
        debtor: UserId,
        creditor: UserId,
        token: &TokenId,
        gross: Quantity,
        fee: Quantity,
    ) {
        bench_scopes!("balances", "balances::transfer");
        let net = split_net_fee(&mut self.fee_balances, token, gross, fee);

        let debtor_key = BalanceKey::new(*token, debtor);
        let mut debtor_balance = self
            .balances
            .get(&debtor_key)
            .expect("BUG: debtor balance missing");
        debtor_balance.debit_reserved(&gross);
        self.balances.insert(debtor_key, debtor_balance);

        // Self-transfer: debtor and creditor are the same user, so the
        // credit must land on the just-updated balance — `update` re-reads
        // before depositing to avoid clobbering the debit.
        self.update(creditor, *token, |b| b.deposit(net));
    }

    /// Open a write-back buffer over the balance map, scoped to a single
    /// settling event. Each `(token, user)` row touched while the batch is
    /// live is read from the stable map at most once and written back at most
    /// once, on [`SettlingBatch::flush`]. Preserves the per-op accounting of
    /// [`transfer`](Self::transfer) / [`unreserve`](Self::unreserve) exactly,
    /// including the fee-pool accrual and the empty-row elision.
    pub fn settling_batch(&mut self) -> SettlingBatch<'_, M> {
        SettlingBatch {
            balances: &mut self.balances,
            fee_balances: &mut self.fee_balances,
            buffer: BTreeMap::new(),
        }
    }

    /// Read the accumulated fee balance for `token`. `None` if no fees have
    /// ever been accrued for this token.
    pub fn fee_balance(&self, token: &TokenId) -> Option<Quantity> {
        self.fee_balances.get(token).copied()
    }

    /// Iterate the fee pool. Order is by `TokenId` (BTreeMap ordering).
    pub fn iter_fee_balances(&self) -> impl Iterator<Item = (TokenId, Quantity)> + '_ {
        self.fee_balances.iter().map(|(k, v)| (*k, *v))
    }

    /// Snapshot the heap-resident fee pool for pre-upgrade serialization.
    /// Stable user balances are excluded; they survive upgrades on their
    /// own via the underlying [`StableBTreeMap`].
    pub fn fee_pool_snapshot(&self) -> Vec<FeeEntry> {
        self.fee_balances
            .iter()
            .map(|(token, amount)| FeeEntry {
                token: *token,
                amount: *amount,
            })
            .collect()
    }

    /// Restore the heap-resident fee pool after a `post_upgrade` decode.
    /// Replaces any existing pool; intended to run exactly once during
    /// post-upgrade. Duplicate `TokenId` entries in `snapshot` trap.
    pub fn restore_fee_pool(&mut self, snapshot: Vec<FeeEntry>) {
        self.fee_balances.clear();
        for entry in snapshot {
            assert!(
                self.fee_balances
                    .insert(entry.token, entry.amount)
                    .is_none(),
                "invalid snapshot: duplicate fee-pool entry for {:?}",
                entry.token,
            );
        }
    }

    /// Read-modify-write for an infallible mutation. Creates the entry if
    /// absent. Skips the write when the mutation left the balance at the
    /// default and no entry existed, so no-op closures (e.g. `deposit(ZERO)`)
    /// don't materialise empty `(0, 0)` rows.
    fn update<F: FnOnce(&mut Balance)>(&mut self, user: UserId, token: TokenId, f: F) {
        let key = BalanceKey::new(token, user);
        let prev = self.balances.get(&key);
        let existed = prev.is_some();
        let mut balance = prev.unwrap_or_default();
        f(&mut balance);
        if existed || !balance.is_zero() {
            self.balances.insert(key, balance);
        }
    }

    /// Read-modify-write for a fallible mutation. On `Err(_)` the stored
    /// entry is left untouched; on `Ok(_)` the updated value is persisted.
    fn try_update<F, T, E>(&mut self, user: UserId, token: TokenId, f: F) -> Result<T, E>
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

/// Assert `fee <= gross`, accrue `fee` into the token's fee pool, and return
/// the net `gross - fee` owed to the creditor. Shared by the per-op
/// [`TokenBalance::transfer`] and the buffered [`SettlingBatch::transfer`] so
/// the money-accounting lives in one place.
fn split_net_fee(
    fee_balances: &mut BTreeMap<TokenId, Quantity>,
    token: &TokenId,
    gross: Quantity,
    fee: Quantity,
) -> Quantity {
    assert!(
        fee <= gross,
        "BUG: fee {fee:?} exceeds gross {gross:?} in transfer"
    );
    let net = gross
        .checked_sub(fee)
        .expect("BUG: fee <= gross checked above");
    if !fee.is_zero() {
        let entry = fee_balances.entry(*token).or_default();
        *entry = entry.checked_add(fee).expect("BUG: fee accrual overflow");
    }
    net
}

/// In-heap write-back buffer over the balance map for the balance operations
/// of one settling event, opened by [`TokenBalance::settling_batch`].
///
/// The taker of a large sweep is party to every fill, so its two balance rows
/// would otherwise be read-modify-written on each fill. The buffer collapses
/// that to a single stable read per row on first touch and a single write-back
/// per dirty row on [`flush`](Self::flush), while the fee pool keeps accruing
/// on the heap as in [`TokenBalance::transfer`].
pub struct SettlingBatch<'a, M: Memory> {
    balances: &'a mut StableBTreeMap<BalanceKey, Balance, M>,
    fee_balances: &'a mut BTreeMap<TokenId, Quantity>,
    buffer: BTreeMap<BalanceKey, BufferedBalance>,
}

struct BufferedBalance {
    balance: Balance,
    existed: bool,
}

impl<M: Memory> SettlingBatch<'_, M> {
    /// Buffered counterpart of [`TokenBalance::transfer`]: debits `gross` from
    /// the debtor's reserved, credits `gross - fee` to the creditor's free, and
    /// accrues `fee` to the token's fee pool. A self-transfer lands the credit
    /// on the just-debited buffered row.
    pub fn transfer(
        &mut self,
        debtor: UserId,
        creditor: UserId,
        token: &TokenId,
        gross: Quantity,
        fee: Quantity,
    ) {
        bench_scopes!("balances", "balances::transfer");
        let net = split_net_fee(self.fee_balances, token, gross, fee);

        self.load_existing(
            BalanceKey::new(*token, debtor),
            "BUG: debtor balance missing",
        )
        .debit_reserved(&gross);
        self.load_or_create(BalanceKey::new(*token, creditor))
            .deposit(net);
    }

    /// Buffered counterpart of [`TokenBalance::unreserve`]: moves `amount` from
    /// the user's reserved to their free balance.
    pub fn unreserve(&mut self, user: UserId, token: &TokenId, amount: Quantity) {
        bench_scopes!("balances", "balances::unreserve");
        self.load_existing(
            BalanceKey::new(*token, user),
            "BUG: user balance missing for unreserve",
        )
        .unreserve(amount);
    }

    /// Write each buffered row back to the stable map exactly once, eliding
    /// rows that neither existed before the batch nor hold a non-zero balance —
    /// matching the empty-row elision of [`TokenBalance::update`].
    pub fn flush(self) {
        bench_scopes!("balances", "balances::flush");
        for (key, buffered) in self.buffer {
            if buffered.existed || !buffered.balance.is_zero() {
                self.balances.insert(key, buffered.balance);
            }
        }
    }

    /// Buffer a row whose stable entry must already exist, mirroring the
    /// `expect(...)` of the debtor read in [`TokenBalance::transfer`] and the
    /// target read in [`TokenBalance::unreserve`]. Traps with `msg` if the row
    /// is absent on its first touch this batch.
    fn load_existing(&mut self, key: BalanceKey, msg: &'static str) -> &mut Balance {
        let entry = self.buffer.entry(key).or_insert_with(|| BufferedBalance {
            existed: true,
            balance: self.balances.get(&key).expect(msg),
        });
        &mut entry.balance
    }

    /// Buffer a row that may not yet exist, mirroring the creditor credit in
    /// [`TokenBalance::transfer`], which creates the entry on demand.
    fn load_or_create(&mut self, key: BalanceKey) -> &mut Balance {
        let entry = self.buffer.entry(key).or_insert_with(|| {
            let prev = self.balances.get(&key);
            BufferedBalance {
                existed: prev.is_some(),
                balance: prev.unwrap_or_default(),
            }
        });
        &mut entry.balance
    }
}

/// CBOR-serializable entry of the fee pool, used by
/// [`TokenBalance::fee_pool_snapshot`] and `StateSnapshot`.
#[derive(Clone, Debug, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct FeeEntry {
    #[n(0)]
    pub token: TokenId,
    #[n(1)]
    pub amount: Quantity,
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
        fresh.fee_balances = self.fee_balances.clone();
        fresh
    }
}

#[cfg(test)]
impl PartialEq for TokenBalance<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter()) && self.fee_balances == other.fee_balances
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
