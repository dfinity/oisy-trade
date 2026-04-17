use super::{Balance, InsufficientBalanceError};
use crate::order::{Quantity, TokenId};
use candid::Principal;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;
use std::cmp::Ordering;

// ---------------------------------------------------------------------------
// Storable key/value types
// ---------------------------------------------------------------------------

/// Composite key `(TokenId, Principal)` for the balance map.
///
/// Encoded as two length-prefixed principal blobs in a 60-byte fixed buffer.
/// Layout: `[token_len: 1][token: 29][user_len: 1][user: 29]`.
///
/// Ordering: token first, then user — matching the heap `TokenBalance`
/// structure's access pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceKey([u8; 60]);

impl BalanceKey {
    pub fn new(token: &TokenId, user: &Principal) -> Self {
        let mut buf = [0u8; 60];
        let t = token.as_principal().as_slice();
        buf[0] = t.len() as u8;
        buf[1..1 + t.len()].copy_from_slice(t);
        let u = user.as_slice();
        buf[30] = u.len() as u8;
        buf[31..31 + u.len()].copy_from_slice(u);
        Self(buf)
    }

    pub fn token(&self) -> TokenId {
        let len = self.0[0] as usize;
        TokenId::new(Principal::from_slice(&self.0[1..1 + len]))
    }

    pub fn user(&self) -> Principal {
        let len = self.0[30] as usize;
        Principal::from_slice(&self.0[31..31 + len])
    }
}

impl Ord for BalanceKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.token()
            .as_principal()
            .cmp(other.token().as_principal())
            .then(self.user().cmp(&other.user()))
    }
}

impl PartialOrd for BalanceKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Storable for BalanceKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let mut buf = [0u8; 60];
        buf.copy_from_slice(&bytes);
        Self(buf)
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 60,
        is_fixed_size: true,
    };
}

/// A [`Balance`] stored in stable memory as two 32-byte big-endian quantities.
/// Layout: `[free: 32][reserved: 32]` = 64 bytes fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorableBalance {
    pub free: Quantity,
    pub reserved: Quantity,
}

impl StorableBalance {
    fn from_balance(b: &Balance) -> Self {
        Self {
            free: *b.free(),
            reserved: *b.reserved(),
        }
    }

    fn to_balance(&self) -> Balance {
        Balance::new(self.free, self.reserved)
    }
}

impl Storable for StorableBalance {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; 64];
        buf[..32].copy_from_slice(&self.free.to_be_bytes());
        buf[32..].copy_from_slice(&self.reserved.to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = [0u8; 64];
        buf[..32].copy_from_slice(&self.free.to_be_bytes());
        buf[32..].copy_from_slice(&self.reserved.to_be_bytes());
        buf.to_vec()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let free = Quantity::from_be_bytes(&bytes[..32]).expect("invalid free Quantity");
        let reserved = Quantity::from_be_bytes(&bytes[32..]).expect("invalid reserved Quantity");
        Self { free, reserved }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 64,
        is_fixed_size: true,
    };
}

// ---------------------------------------------------------------------------
// StableTokenBalance
// ---------------------------------------------------------------------------

/// A balance ledger backed by stable memory.
///
/// Mirrors the API of [`super::TokenBalance`] but stores all `(token, user) →
/// Balance` entries in a single [`StableBTreeMap`].
pub struct StableTokenBalance<M: Memory> {
    balances: StableBTreeMap<BalanceKey, StorableBalance, M>,
}

impl<M: Memory> StableTokenBalance<M> {
    pub fn new(memory: M) -> Self {
        Self {
            balances: StableBTreeMap::init(memory),
        }
    }

    /// Deposit `amount` into a user's free balance for the given token.
    pub fn deposit(&mut self, user: Principal, token: TokenId, amount: Quantity) {
        let key = BalanceKey::new(&token, &user);
        let mut bal = self
            .balances
            .get(&key)
            .map(|sb| sb.to_balance())
            .unwrap_or_default();
        bal.deposit(amount);
        self.balances.insert(key, StorableBalance::from_balance(&bal));
    }

    /// Read a user's balance for a given token.
    pub fn get_balance(&self, user: &Principal, token: &TokenId) -> Option<Balance> {
        let key = BalanceKey::new(token, user);
        self.balances.get(&key).map(|sb| sb.to_balance())
    }

    /// Read a user's free balance for a given token.
    pub fn get_free(&self, user: &Principal, token: &TokenId) -> Quantity {
        self.get_balance(user, token)
            .map(|b| *b.free())
            .unwrap_or(Quantity::ZERO)
    }

    /// Withdraw `amount` from a user's free balance.
    pub fn withdraw(
        &mut self,
        user: &Principal,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        let key = BalanceKey::new(token, user);
        let mut bal = self
            .balances
            .get(&key)
            .map(|sb| sb.to_balance())
            .unwrap_or_default();
        bal.withdraw(amount)?;
        self.balances.insert(key, StorableBalance::from_balance(&bal));
        Ok(())
    }

    /// Reserve `amount` from a user's free balance for an open order.
    pub fn reserve(
        &mut self,
        user: &Principal,
        token: &TokenId,
        amount: Quantity,
    ) -> Result<(), InsufficientBalanceError> {
        let key = BalanceKey::new(token, user);
        let mut bal = self
            .balances
            .get(&key)
            .map(|sb| sb.to_balance())
            .ok_or(InsufficientBalanceError {
                available: Quantity::ZERO,
                required: amount,
            })?;
        bal.reserve(amount)?;
        self.balances.insert(key, StorableBalance::from_balance(&bal));
        Ok(())
    }

    /// Move `amount` from debitor's reserved to creditor's free for a given token.
    pub fn transfer(
        &mut self,
        token: &TokenId,
        debitor: &Principal,
        creditor: &Principal,
        amount: Quantity,
    ) {
        // Debit from debitor's reserved
        let deb_key = BalanceKey::new(token, debitor);
        let mut deb_bal = self
            .balances
            .get(&deb_key)
            .expect("BUG: debitor balance missing")
            .to_balance();
        deb_bal.debit_reserved(amount);
        self.balances
            .insert(deb_key, StorableBalance::from_balance(&deb_bal));

        // Credit to creditor's free
        let cred_key = BalanceKey::new(token, creditor);
        let mut cred_bal = self
            .balances
            .get(&cred_key)
            .map(|sb| sb.to_balance())
            .unwrap_or_default();
        cred_bal.deposit(amount);
        self.balances
            .insert(cred_key, StorableBalance::from_balance(&cred_bal));
    }

    /// Move `amount` from user's reserved back to user's free for a given token.
    pub fn unreserve(&mut self, token: &TokenId, user: &Principal, amount: Quantity) {
        let key = BalanceKey::new(token, user);
        let mut bal = self
            .balances
            .get(&key)
            .expect("BUG: user balance missing for unreserve")
            .to_balance();
        bal.unreserve(amount);
        self.balances.insert(key, StorableBalance::from_balance(&bal));
    }
}
