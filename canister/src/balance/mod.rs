use candid::Nat;

#[cfg(test)]
mod tests;

/// Represents a user's balance for a given token.
///
/// The balance is split into two parts:
/// - `free`: funds available for new orders or withdrawal.
/// - `reserved`: funds locked by open orders.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct Balance {
    free: Nat,
    reserved: Nat,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InsufficientBalanceError {
    pub available: Nat,
    pub required: Nat,
}

impl Balance {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn new(free: impl Into<Nat>, reserved: impl Into<Nat>) -> Self {
        Self {
            free: free.into(),
            reserved: reserved.into(),
        }
    }

    pub fn free(&self) -> &Nat {
        &self.free
    }

    pub fn reserved(&self) -> &Nat {
        &self.reserved
    }

    pub fn deposit(&mut self, amount: Nat) {
        self.free += amount;
    }

    /// Debit `amount` from the reserved balance.
    ///
    /// # Panics
    /// Panics if `amount` exceeds the reserved balance (invariant violation).
    pub fn debit_reserved(&mut self, amount: Nat) {
        assert!(
            self.reserved >= amount,
            "BUG: debit_reserved underflow: reserved={}, amount={}",
            self.reserved,
            amount
        );
        self.reserved -= amount;
    }

    /// Move `amount` from reserved back to free.
    ///
    /// # Panics
    /// Panics if `amount` exceeds the reserved balance (invariant violation).
    pub fn unreserve(&mut self, amount: Nat) {
        assert!(
            self.reserved >= amount,
            "BUG: unreserve underflow: reserved={}, amount={}",
            self.reserved,
            amount
        );
        self.reserved -= amount.clone();
        self.free += amount;
    }

    pub fn reserve(&mut self, required: Nat) -> Result<(), InsufficientBalanceError> {
        if self.free < required {
            return Err(InsufficientBalanceError {
                available: self.free.clone(),
                required,
            });
        }
        self.free -= required.clone();
        self.reserved += required;
        Ok(())
    }
}

impl From<Balance> for dex_types::Balance {
    fn from(b: Balance) -> Self {
        Self {
            free: b.free,
            reserved: b.reserved,
        }
    }
}

impl From<&Balance> for dex_types::Balance {
    fn from(b: &Balance) -> Self {
        Self {
            free: b.free.clone(),
            reserved: b.reserved.clone(),
        }
    }
}
