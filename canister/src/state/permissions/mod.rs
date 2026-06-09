use crate::order::OrderBookId;
use candid::Principal;

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Permissions {
    trading_halted: bool,
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
    // TODO(DEFI-2849): `reconcile` reads these to re-check the freeze once the
    // freeze check lands (PR 4); unused while reconcile is always `Clean`.
    #[allow(dead_code)]
    caller: Principal,
    #[allow(dead_code)]
    kind: AsyncKind,
}

/// Proof that an asynchronous action was reconciled post-await.
///
/// Produced *only* by [`PreAsyncPermit::reconcile`]; the recorder consumes it to
/// persist a deposit/withdraw. Carries the reconciliation verdict so the policy
/// for a mid-await permission change lives in one place (the recorder).
pub struct PostAsyncPermit {
    // TODO(DEFI-2849): the recorder reads this verdict to emit the `Raced`
    // observability log once the freeze check lands (PR 4); always `Clean` here.
    #[allow(dead_code)]
    verdict: Reconciliation,
}

pub enum AsyncKind {
    Deposit,
    Withdraw,
}

/// Whether the permission state changed across the `await`. `Raced` means the
/// caller was frozen mid-await; the external effect already committed, so it is
/// flagged, never reverted.
pub enum Reconciliation {
    Clean,
    Raced,
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

impl PreAsyncPermit {
    /// Re-checks the caller's permission after the `await`. Observational only:
    /// the ledger effect already committed, so it never denies — it can only
    /// flag a mid-await permission change as [`Reconciliation::Raced`].
    pub fn reconcile(self, _permissions: &Permissions) -> PostAsyncPermit {
        PostAsyncPermit {
            verdict: Reconciliation::Clean,
        }
    }
}

impl Permissions {
    pub fn set_trading_halted(&mut self, halted: bool) {
        self.trading_halted = halted;
    }

    pub fn trading_halted(&self) -> bool {
        self.trading_halted
    }

    pub fn permit_trading(
        &self,
        _caller: Principal,
        _book: OrderBookId,
    ) -> Result<SyncPermit, UnauthorizedError> {
        if self.trading_halted {
            return Err(UnauthorizedError::TradingHalted);
        }
        Ok(SyncPermit(()))
    }

    pub fn permit_matching(&self, _book: OrderBookId) -> Result<SyncPermit, UnauthorizedError> {
        if self.trading_halted {
            return Err(UnauthorizedError::TradingHalted);
        }
        Ok(SyncPermit(()))
    }

    pub fn permit_deposit(&self, caller: Principal) -> Result<PreAsyncPermit, UnauthorizedError> {
        Ok(PreAsyncPermit {
            caller,
            kind: AsyncKind::Deposit,
        })
    }

    pub fn permit_withdraw(&self, caller: Principal) -> Result<PreAsyncPermit, UnauthorizedError> {
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
