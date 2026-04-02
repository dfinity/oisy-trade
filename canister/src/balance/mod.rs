use candid::Nat;

/// Represents a user's balance for a given token.
///
/// The balance is split into two parts:
/// - `free`: funds available for new orders or withdrawal.
/// - `reserved`: funds locked by open orders.
#[derive(Debug, Clone, Default)]
pub struct Balance {
    free: Nat,
    reserved: Nat,
}

impl Balance {
    pub fn free(&self) -> &Nat {
        &self.free
    }

    pub fn reserved(&self) -> &Nat {
        &self.reserved
    }

    pub fn deposit(&mut self, amount: Nat) {
        self.free += amount;
    }
}
