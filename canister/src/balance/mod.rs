mod cache;
mod token;

pub use token::{FeeEntry, TokenBalance};

use crate::order::{Quantity, TokenId};
use crate::user::UserId;
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use std::borrow::Cow;

#[cfg(test)]
mod tests;

/// Represents a user's balance for a given token.
///
/// The balance is split into two parts:
/// - `free`: funds available for new orders or withdrawal.
/// - `reserved`: funds locked by open orders.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, minicbor::Encode, minicbor::Decode)]
pub struct Balance {
    #[n(0)]
    free: Quantity,
    #[n(1)]
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

    pub fn new_free(free: impl Into<Quantity>) -> Self {
        Self::new(free, Quantity::ZERO)
    }

    pub fn new_reserved(reserved: impl Into<Quantity>) -> Self {
        Self::new(Quantity::ZERO, reserved)
    }

    pub fn free(&self) -> &Quantity {
        &self.free
    }

    pub fn reserved(&self) -> &Quantity {
        &self.reserved
    }

    pub fn deposit(&mut self, amount: Quantity) {
        bench_scopes!("bal", "bal::deposit");
        self.free = self
            .free
            .checked_add(amount)
            .expect("BUG: deposit overflow");
    }

    /// Debit `amount` from the reserved balance.
    ///
    /// # Panics
    /// Panics if `amount` exceeds the reserved balance (invariant violation).
    pub fn debit_reserved(&mut self, amount: &Quantity) {
        bench_scopes!("bal", "bal::debit_reserved");
        self.reserved = self.reserved.checked_sub(*amount).unwrap_or_else(|| {
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
        self.reserved = self.reserved.checked_sub(amount).unwrap_or_else(|| {
            panic!(
                "BUG: unreserve underflow: reserved={:?}, amount={:?}",
                self.reserved, amount
            )
        });
        self.free = self
            .free
            .checked_add(amount)
            .expect("BUG: unreserve overflow");
    }

    pub fn withdraw(&mut self, amount: Quantity) -> Result<(), InsufficientBalanceError> {
        self.free = self
            .free
            .checked_sub(amount)
            .ok_or(InsufficientBalanceError {
                available: self.free,
                required: amount,
            })?;
        Ok(())
    }

    pub fn reserve(&mut self, required: Quantity) -> Result<(), InsufficientBalanceError> {
        bench_scopes!("bal", "bal::reserve");
        self.free = self
            .free
            .checked_sub(required)
            .ok_or(InsufficientBalanceError {
                available: self.free,
                required,
            })?;
        self.reserved = self
            .reserved
            .checked_add(required)
            .expect("BUG: reserve overflow");
        Ok(())
    }

    pub const fn is_zero(&self) -> bool {
        self.free.is_zero() && self.reserved.is_zero()
    }
}

impl From<Balance> for oisy_trade_types::Balance {
    fn from(b: Balance) -> Self {
        Self {
            free: b.free.into(),
            reserved: b.reserved.into(),
        }
    }
}

impl From<&Balance> for oisy_trade_types::Balance {
    fn from(b: &Balance) -> Self {
        Self {
            free: b.free.into(),
            reserved: b.reserved.into(),
        }
    }
}

impl Storable for Balance {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("balance encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("balance encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode balance bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// Composite key used to index [`Balance`] entries in
/// [`ic_stable_structures::StableBTreeMap`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Encode, minicbor::Decode,
)]
pub struct BalanceKey {
    #[n(0)]
    token: TokenId,
    #[n(1)]
    user: UserId,
}

impl BalanceKey {
    pub fn new(token: TokenId, user: UserId) -> Self {
        Self { token, user }
    }

    pub fn token(&self) -> &TokenId {
        &self.token
    }

    pub fn user(&self) -> &UserId {
        &self.user
    }
}

impl Storable for BalanceKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("balance key encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("balance key encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode balance key bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}
