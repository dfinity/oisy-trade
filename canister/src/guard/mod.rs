use crate::order::TokenId;
use crate::state::with_state_mut;
use candid::Principal;

#[cfg(test)]
mod tests;

/// RAII guard to prevent concurrent deposit/withdraw operations per
/// `(caller, token)`.
#[derive(Eq, PartialEq, Debug)]
pub struct UserOpGuard {
    key: (Principal, TokenId),
}

impl UserOpGuard {
    pub fn new(caller: Principal, token: TokenId) -> Option<Self> {
        let key = (caller, token);
        with_state_mut(|s| {
            if !s.in_flight_user_ops_mut().insert(key) {
                return None;
            }
            Some(Self { key })
        })
    }
}

impl Drop for UserOpGuard {
    fn drop(&mut self) {
        with_state_mut(|s| {
            s.in_flight_user_ops_mut().remove(&self.key);
        });
    }
}
