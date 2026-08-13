use crate::order::{Quantity, TokenId};
use std::collections::BTreeMap;

/// The canister-owned fees withheld from fills, per token.
///
/// Heap-resident because it is bounded by the number of listed tokens
/// (10s–100s), whereas user balances are unbounded and belong in stable
/// memory. Persisted across upgrades through the [`snapshot`](Self::snapshot) /
/// [`restore`](Self::restore) pair plumbed through `StateSnapshot`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeePool {
    per_token: BTreeMap<TokenId, Quantity>,
}

impl FeePool {
    /// Withhold `fee` out of `gross` for `token`, returning the net
    /// `gross - fee` owed to the creditor of the transfer.
    ///
    /// # Panics
    ///
    /// - `fee > gross`. Preconditions are the caller's responsibility; the
    ///   per-pair `BasisPoint` invariant guarantees this at the fill path.
    /// - The token's accrued fees would overflow.
    pub fn withhold(&mut self, token: &TokenId, gross: Quantity, fee: Quantity) -> Quantity {
        assert!(
            fee <= gross,
            "BUG: fee {fee:?} exceeds gross {gross:?} in transfer"
        );
        let net = gross
            .checked_sub(fee)
            .expect("BUG: fee <= gross checked above");
        if !fee.is_zero() {
            let accrued = self.per_token.entry(*token).or_default();
            *accrued = accrued.checked_add(fee).expect("BUG: fee accrual overflow");
        }
        net
    }

    /// The fees accrued for `token`. `None` if none ever were.
    pub fn accrued(&self, token: &TokenId) -> Option<Quantity> {
        self.per_token.get(token).copied()
    }

    /// Iterate the pool. Order is by `TokenId` (BTreeMap ordering).
    pub fn iter(&self) -> impl Iterator<Item = (TokenId, Quantity)> + '_ {
        self.per_token.iter().map(|(token, fee)| (*token, *fee))
    }

    /// Snapshot the pool for pre-upgrade serialization.
    pub fn snapshot(&self) -> Vec<FeeEntry> {
        self.per_token
            .iter()
            .map(|(token, amount)| FeeEntry {
                token: *token,
                amount: *amount,
            })
            .collect()
    }

    /// Restore the pool after a `post_upgrade` decode, replacing any existing
    /// entries.
    ///
    /// # Panics
    ///
    /// Panics if `snapshot` holds duplicate `TokenId` entries.
    pub fn restore(&mut self, snapshot: Vec<FeeEntry>) {
        self.per_token.clear();
        for entry in snapshot {
            assert!(
                self.per_token.insert(entry.token, entry.amount).is_none(),
                "invalid snapshot: duplicate fee-pool entry for {:?}",
                entry.token,
            );
        }
    }
}

/// CBOR-serializable entry of a [`FeePool`], used by
/// [`snapshot`](FeePool::snapshot) and `StateSnapshot`.
#[derive(Clone, Debug, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct FeeEntry {
    #[n(0)]
    pub token: TokenId,
    #[n(1)]
    pub amount: Quantity,
}
