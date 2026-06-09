use crate::order::OrderBookId;
use candid::Principal;
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Permissions {
    trading_halted: bool,
    halted_pairs: BTreeSet<OrderBookId>,
    frozen_accounts: BTreeSet<Principal>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UnauthorizedError {
    TradingHalted,
    PairHalted,
    AccountFrozen,
    NotController,
}

impl From<UnauthorizedError> for dex_types::UnauthorizedError {
    fn from(err: UnauthorizedError) -> Self {
        match err {
            UnauthorizedError::TradingHalted => dex_types::UnauthorizedError::TradingHalted,
            UnauthorizedError::PairHalted => dex_types::UnauthorizedError::PairHalted,
            UnauthorizedError::AccountFrozen => dex_types::UnauthorizedError::AccountFrozen,
            UnauthorizedError::NotController => dex_types::UnauthorizedError::NotController,
        }
    }
}

/// Proof that a synchronous admission check ran and passed.
///
/// Permit tokens are capability tokens: the recorder *consumes* a permit to
/// persist a state change, so holding one is proof that permission was checked.
/// They are deliberately neither `Clone` nor `Copy` — a permit cannot be reused
/// for a second state change, and a stale permit cannot be replayed. The only
/// way to obtain one is to ask [`Permissions`] via a `permit_*` method (the
/// private field makes `SyncPermit` non-constructible outside this module), so
/// "a state change was recorded" implies "permission was checked".
pub struct SyncPermit(());

/// Proof that an asynchronous admission check ran and passed *pre-await*.
///
/// A `PreAsyncPermit` carries the obligation to be reconciled *post-await*
/// before the event can be recorded: it is `#[must_use]`, neither `Clone` nor
/// `Copy`, and the only way to turn it into a recordable [`Permit`] is
/// [`PreAsyncPermit::reconcile`], which consumes it. The only way to obtain one
/// is to ask [`Permissions`] via `permit_deposit` / `permit_withdraw`.
#[must_use]
pub struct PreAsyncPermit {
    caller: Principal,
    kind: AsyncKind,
}

/// Proof that an asynchronous action was reconciled post-await.
///
/// Produced *only* by [`PreAsyncPermit::reconcile`]; the recorder consumes it to
/// persist a deposit/withdraw. Carries the reconciliation verdict so the policy
/// for a mid-await permission change lives in one place (the recorder).
pub struct PostAsyncPermit {
    verdict: Reconciliation,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AsyncKind {
    Deposit,
    Withdraw,
}

/// Whether the permission state changed across the `await`. `Raced` means the
/// caller was frozen mid-await; the external effect already committed, so it is
/// flagged, never reverted.
#[derive(Debug, PartialEq, Eq)]
pub enum Reconciliation {
    Clean,
    Raced,
}

impl PreAsyncPermit {
    pub fn kind(&self) -> &AsyncKind {
        &self.kind
    }

    /// Re-checks the caller's permission after the `await`. Observational only:
    /// the ledger effect already committed, so it never denies — it can only
    /// flag a mid-await permission change as [`Reconciliation::Raced`]: `Raced`
    /// iff the caller is now frozen, `Clean` otherwise.
    pub fn reconcile(self, permissions: &Permissions) -> PostAsyncPermit {
        let verdict = if permissions.is_frozen(&self.caller) {
            Reconciliation::Raced
        } else {
            Reconciliation::Clean
        };
        PostAsyncPermit { verdict }
    }
}

impl PostAsyncPermit {
    pub fn verdict(&self) -> &Reconciliation {
        &self.verdict
    }
}

pub enum Permit {
    Sync(SyncPermit),
    Async(PostAsyncPermit),
}

impl From<SyncPermit> for Permit {
    fn from(permit: SyncPermit) -> Self {
        Permit::Sync(permit)
    }
}

impl From<PostAsyncPermit> for Permit {
    fn from(permit: PostAsyncPermit) -> Self {
        Permit::Async(permit)
    }
}

impl Permissions {
    pub fn set_trading_halted(&mut self, halted: bool) {
        self.trading_halted = halted;
    }

    pub fn trading_halted(&self) -> bool {
        self.trading_halted
    }

    pub fn set_pair_halted(&mut self, book: OrderBookId, halted: bool) {
        if halted {
            self.halted_pairs.insert(book);
        } else {
            self.halted_pairs.remove(&book);
        }
    }

    pub fn is_pair_halted(&self, book: &OrderBookId) -> bool {
        self.halted_pairs.contains(book)
    }

    pub fn halted_pairs(&self) -> impl Iterator<Item = &OrderBookId> {
        self.halted_pairs.iter()
    }

    pub fn set_account_frozen(&mut self, account: Principal, frozen: bool) {
        if frozen {
            self.frozen_accounts.insert(account);
        } else {
            self.frozen_accounts.remove(&account);
        }
    }

    pub fn is_frozen(&self, caller: &Principal) -> bool {
        self.frozen_accounts.contains(caller)
    }

    pub fn frozen_accounts(&self) -> impl Iterator<Item = &Principal> {
        self.frozen_accounts.iter()
    }

    pub fn permit_trading(
        &self,
        caller: Principal,
        book: OrderBookId,
    ) -> Result<SyncPermit, UnauthorizedError> {
        if self.is_frozen(&caller) {
            return Err(UnauthorizedError::AccountFrozen);
        }
        if self.trading_halted {
            return Err(UnauthorizedError::TradingHalted);
        }
        if self.is_pair_halted(&book) {
            return Err(UnauthorizedError::PairHalted);
        }
        Ok(SyncPermit(()))
    }

    pub fn permit_matching(&self, book: OrderBookId) -> Result<SyncPermit, UnauthorizedError> {
        if self.trading_halted {
            return Err(UnauthorizedError::TradingHalted);
        }
        if self.is_pair_halted(&book) {
            return Err(UnauthorizedError::PairHalted);
        }
        Ok(SyncPermit(()))
    }

    pub fn permit_deposit(&self, caller: Principal) -> Result<PreAsyncPermit, UnauthorizedError> {
        if self.is_frozen(&caller) {
            return Err(UnauthorizedError::AccountFrozen);
        }
        Ok(PreAsyncPermit {
            caller,
            kind: AsyncKind::Deposit,
        })
    }

    pub fn permit_withdraw(&self, caller: Principal) -> Result<PreAsyncPermit, UnauthorizedError> {
        if self.is_frozen(&caller) {
            return Err(UnauthorizedError::AccountFrozen);
        }
        Ok(PreAsyncPermit {
            caller,
            kind: AsyncKind::Withdraw,
        })
    }

    pub fn permit_cancel(&self) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
    }

    pub fn permit_settling(&self) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
    }

    pub fn permit_add_trading_pair(&self) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
    }

    pub fn permit_admin(&self) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
    }
}
