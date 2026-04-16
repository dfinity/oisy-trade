use super::{Balance, InsufficientBalanceError};
use crate::order::Quantity;
use candid::Principal;
use std::collections::BTreeMap;

/// Per-user balance map for a single token.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserBalance(BTreeMap<Principal, Balance>);

impl UserBalance {
    /// Move `amount` from debitor's reserved to creditor's free.
    pub fn transfer(&mut self, debitor: &Principal, creditor: &Principal, amount: Quantity) {
        self.0
            .get_mut(debitor)
            .expect("BUG: debitor balance missing")
            .debit_reserved(amount.clone());
        self.0.entry(*creditor).or_default().deposit(amount);
    }

    /// Move `amount` from user's reserved back to user's free.
    pub fn unreserve(&mut self, user: &Principal, amount: Quantity) {
        self.0
            .get_mut(user)
            .expect("BUG: user balance missing for unreserve")
            .unreserve(amount);
    }

    /// Add `amount` to user's free balance (creates entry if needed).
    pub fn deposit(&mut self, user: Principal, amount: Quantity) {
        self.0.entry(user).or_default().deposit(amount);
    }

    /// Move `amount` from user's free to user's reserved.
    pub fn reserve(
        &mut self,
        user: &Principal,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        self.0
            .get_mut(user)
            .expect("BUG: user balance missing for reserve")
            .reserve(amount)
    }

    /// Read a user's balance.
    pub fn get(&self, user: &Principal) -> Option<&Balance> {
        self.0.get(user)
    }
}
