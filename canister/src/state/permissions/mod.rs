use crate::order::OrderBookId;
use candid::Principal;
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Permissions {
    trading_halted: bool,
    halted_pairs: BTreeSet<OrderBookId>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UnauthorizedError {
    TradingHalted,
    NotController,
}

/// Why a funding operation (deposit / withdraw) was denied admission.
#[derive(Debug, PartialEq, Eq)]
pub enum FundingDenied {
    /// The caller is a trading account, which can never hold DEX balances and
    /// so cannot deposit or withdraw.
    TradingAccountForbidden,
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
/// A `PreAsyncPermit` carries a compile-time obligation to be reconciled
/// *post-await* before the event can be recorded: it is `#[must_use]`, neither
/// `Clone` nor `Copy`, and the only way to turn it into a recordable [`Permit`]
/// is [`PreAsyncPermit::reconcile`], which consumes it. The only way to obtain
/// one is to ask [`Permissions`] via `permit_deposit` / `permit_withdraw`.
/// This is purely a reconcile-before-record gate; it does not re-check
/// permissions post-await.
#[must_use]
pub struct PreAsyncPermit(());

/// Proof that an asynchronous action was reconciled post-await.
///
/// Produced *only* by [`PreAsyncPermit::reconcile`]; the recorder consumes it to
/// persist a deposit/withdraw.
pub struct PostAsyncPermit(());

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
    /// Discharges the must-use `PreAsyncPermit` obligation, yielding the
    /// recordable [`PostAsyncPermit`]. This is a compile-time
    /// reconcile-before-record gate only: it does not re-check permissions and
    /// never denies.
    pub fn reconcile(self) -> PostAsyncPermit {
        PostAsyncPermit(())
    }
}

impl Permissions {
    pub fn halt_trading_globally(&mut self) {
        self.trading_halted = true;
    }

    pub fn resume_trading_globally(&mut self) {
        self.trading_halted = false;
        self.halted_pairs.clear();
    }

    pub fn trading_halted(&self) -> bool {
        self.trading_halted
    }

    pub fn halt_trading(&mut self, book: OrderBookId) {
        self.halted_pairs.insert(book);
    }

    pub fn resume_trading(&mut self, book: OrderBookId) {
        self.halted_pairs.remove(&book);
    }

    pub fn is_halted(&self, book: &OrderBookId) -> bool {
        self.trading_halted || self.halted_pairs.contains(book)
    }

    pub fn halted_pairs(&self) -> impl Iterator<Item = &OrderBookId> {
        self.halted_pairs.iter()
    }

    pub fn permit_trading(
        &self,
        _caller: Principal,
        book: OrderBookId,
    ) -> Result<SyncPermit, UnauthorizedError> {
        if self.is_halted(&book) {
            return Err(UnauthorizedError::TradingHalted);
        }
        Ok(SyncPermit(()))
    }

    pub fn permit_matching(&self, book: OrderBookId) -> Result<SyncPermit, UnauthorizedError> {
        if self.is_halted(&book) {
            return Err(UnauthorizedError::TradingHalted);
        }
        Ok(SyncPermit(()))
    }

    /// Admits a deposit unless the caller is a trading account, which can never
    /// hold DEX balances. `caller_is_trading_account` is whether the calling
    /// principal is a trading account, computed by the canister from the
    /// registry (via `State::lookup_account`); it is derived internally and
    /// must never be treated as caller-supplied input.
    pub fn permit_deposit(
        &self,
        _caller: Principal,
        caller_is_trading_account: bool,
    ) -> Result<PreAsyncPermit, FundingDenied> {
        if caller_is_trading_account {
            return Err(FundingDenied::TradingAccountForbidden);
        }
        Ok(PreAsyncPermit(()))
    }

    /// Admits a withdrawal unless the caller is a trading account, which can
    /// never hold DEX balances. `caller_is_trading_account` is whether the
    /// calling principal is a trading account, computed by the canister from the
    /// registry (via `State::lookup_account`); it is derived internally and
    /// must never be treated as caller-supplied input.
    pub fn permit_withdraw(
        &self,
        _caller: Principal,
        caller_is_trading_account: bool,
    ) -> Result<PreAsyncPermit, FundingDenied> {
        if caller_is_trading_account {
            return Err(FundingDenied::TradingAccountForbidden);
        }
        Ok(PreAsyncPermit(()))
    }

    pub fn permit_cancel(&self) -> SyncPermit {
        SyncPermit(())
    }

    pub fn permit_settling(&self) -> SyncPermit {
        SyncPermit(())
    }

    pub fn permit_add_trading_pair(&self) -> SyncPermit {
        SyncPermit(())
    }

    pub fn permit_admin(&self) -> SyncPermit {
        SyncPermit(())
    }

    pub fn permit_add_trading_account(&self) -> SyncPermit {
        SyncPermit(())
    }

    pub fn permit_remove_trading_account(&self) -> SyncPermit {
        SyncPermit(())
    }
}
