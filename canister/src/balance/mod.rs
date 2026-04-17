mod token;
mod user;

pub use token::TokenBalance;

use crate::order::Quantity;

#[cfg(test)]
mod tests;

/// Represents a user's balance for a given token.
///
/// The balance is split into two parts:
/// - `free`: funds available for new orders or withdrawal.
/// - `reserved`: funds locked by open orders.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct Balance {
    free: Quantity,
    reserved: Quantity,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InsufficientBalanceError {
    pub available: Quantity,
    pub required: Quantity,
}

impl Balance {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn new(free: impl Into<Quantity>, reserved: impl Into<Quantity>) -> Self {
        Self {
            free: free.into(),
            reserved: reserved.into(),
        }
    }

    pub fn free(&self) -> &Quantity {
        &self.free
    }

    pub fn reserved(&self) -> &Quantity {
        &self.reserved
    }

    pub fn deposit(&mut self, amount: Quantity) {
        bench_scopes!("bal", "bal::deposit");
        self.free += amount;
    }

    /// Debit `amount` from the reserved balance.
    ///
    /// # Panics
    /// Panics if `amount` exceeds the reserved balance (invariant violation).
    pub fn debit_reserved(&mut self, amount: Quantity) {
        bench_scopes!("bal", "bal::debit_reserved");
        self.reserved = self.reserved.checked_sub(&amount).unwrap_or_else(|| {
            panic!(
                "BUG: debit_reserved underflow: reserved={:?}, amount={:?}",
                self.reserved, amount
            )
        });
    }

    /// Move `amount` from reserved back to free.
    ///
    /// # Panics
    /// Panics if `amount` exceeds the reserved balance (invariant violation).
    pub fn unreserve(&mut self, amount: Quantity) {
        bench_scopes!("bal", "bal::unreserve");
        self.reserved = self.reserved.checked_sub(&amount).unwrap_or_else(|| {
            panic!(
                "BUG: unreserve underflow: reserved={:?}, amount={:?}",
                self.reserved, amount
            )
        });
        self.free += amount;
    }

    pub fn withdraw(&mut self, amount: Quantity) -> Result<(), InsufficientBalanceError> {
        self.free = self
            .free
            .checked_sub(&amount)
            .ok_or_else(|| InsufficientBalanceError {
                available: self.free.clone(),
                required: amount,
            })?;
        Ok(())
    }

    pub fn reserve(&mut self, required: Quantity) -> Result<(), InsufficientBalanceError> {
        bench_scopes!("bal", "bal::reserve");
        self.free = self
            .free
            .checked_sub(&required)
            .ok_or_else(|| InsufficientBalanceError {
                available: self.free.clone(),
                required: required.clone(),
            })?;
        self.reserved += required;
        Ok(())
    }
}

impl From<Balance> for dex_types::Balance {
    fn from(b: Balance) -> Self {
        Self {
            free: b.free.into(),
            reserved: b.reserved.into(),
        }
    }
}

impl From<&Balance> for dex_types::Balance {
    fn from(b: &Balance) -> Self {
        Self {
            free: b.free.clone().into(),
            reserved: b.reserved.clone().into(),
        }
    }
}
