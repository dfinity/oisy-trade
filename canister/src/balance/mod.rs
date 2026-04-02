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
    pub fn zero() -> Self {
        Self::default()
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
