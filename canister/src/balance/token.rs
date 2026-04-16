use super::user::UserBalance;
use super::{Balance, InsufficientBalanceError};
use crate::order::{Quantity, TokenId};
use candid::Principal;
use std::collections::BTreeMap;

/// Per-token balance ledger, keyed by token then by user.
///
/// This layout allows `settle_fill` to look up the token once and
/// then operate on multiple users within the same inner map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenBalance(BTreeMap<TokenId, UserBalance>);

impl TokenBalance {
    /// Get a mutable reference to the user-balance map for a token.
    pub fn token_mut(&mut self, token: &TokenId) -> &mut UserBalance {
        self.0.entry(*token).or_default()
    }

    /// Deposit `amount` into a user's free balance for the given token.
    pub fn deposit(&mut self, user: Principal, token: TokenId, amount: Quantity) {
        self.token_mut(&token).deposit(user, amount);
    }

    /// Read a user's balance for a given token.
    pub fn get_balance(&self, user: &Principal, token: &TokenId) -> Option<&Balance> {
        self.0.get(token).and_then(|ub| ub.get(user))
    }

    /// Read a user's free balance for a given token.
    pub fn get_free(&self, user: &Principal, token: &TokenId) -> Quantity {
        self.0
            .get(token)
            .and_then(|ub| ub.get(user))
            .map(|b| b.free().clone())
            .unwrap_or(Quantity::ZERO)
    }

    /// Reserve `amount` from a user's free balance for the given token.
    ///
    /// # Panics
    ///
    /// Panics if the token has no balance entries (i.e., no prior deposit
    /// for any user on this token).
    pub fn reserve(
        &mut self,
        user: &Principal,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        self.0
            .get_mut(token)
            .expect("BUG: token balance missing")
            .reserve(user, amount)
    }
}
