use crate::order::OrderBookId;
use candid::Principal;

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Permissions {}

#[derive(Debug, PartialEq, Eq)]
pub enum UnauthorizedError {
    TradingHalted,
    PairHalted,
    AccountFrozen,
    NotController,
}

pub struct SyncPermit(());

pub enum Permit {
    Sync(SyncPermit),
}

impl From<SyncPermit> for Permit {
    fn from(permit: SyncPermit) -> Self {
        Permit::Sync(permit)
    }
}

impl Permissions {
    pub fn permit_trading(
        &self,
        _caller: Principal,
        _book: OrderBookId,
    ) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
    }

    pub fn permit_matching(&self) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
    }

    pub fn permit_deposit(&self, _caller: Principal) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
    }

    pub fn permit_withdraw(&self, _caller: Principal) -> Result<SyncPermit, UnauthorizedError> {
        Ok(SyncPermit(()))
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
