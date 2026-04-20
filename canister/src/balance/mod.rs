mod token;

pub use token::TokenBalance;

use crate::order::{Quantity, TokenId};
use candid::Principal;
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
///
/// Persisted in an [`ic_stable_structures::StableBTreeMap`] (see
/// [`TokenBalance`]) so the CBOR layout is an upgrade-durable schema:
/// removing or renumbering a field breaks decoding of balances written by
/// prior canister versions. New fields must be added with
/// `#[cbor(n(N), default)]` or `Option<T>`.
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
        self.reserved = self.reserved.checked_sub(amount).unwrap_or_else(|| {
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
        self.free = self
            .free
            .checked_add(amount)
            .expect("BUG: unreserve overflow");
    }

    pub fn withdraw(&mut self, amount: Quantity) -> Result<(), InsufficientBalanceError> {
        self.free = self
            .free
            .checked_sub(&amount)
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
            .checked_sub(&required)
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
///
/// Layout: 30 bytes for the token, followed by 30 bytes for the owner. Each
/// principal is encoded as `[len, bytes..]` with trailing zeros padded to the
/// full 30-byte slot, so byte-lexicographic ordering matches `Principal::cmp`
/// (which compares by length first, then by bytes) and iteration is grouped
/// by token — the "token-first" layout `docs/design.md` §Upgrade Strategy
/// calls out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BalanceKey {
    token: TokenId,
    owner: Principal,
}

impl BalanceKey {
    pub const ENCODED_SIZE: u32 = 60;
    const PRINCIPAL_SLOT: usize = 30;

    pub fn new(token: TokenId, owner: Principal) -> Self {
        Self { token, owner }
    }

    pub fn token(&self) -> &TokenId {
        &self.token
    }

    pub fn owner(&self) -> &Principal {
        &self.owner
    }

    fn encode_principal_into(slot: &mut [u8], p: &Principal) {
        debug_assert_eq!(slot.len(), Self::PRINCIPAL_SLOT);
        let bytes = p.as_slice();
        assert!(
            bytes.len() <= Principal::MAX_LENGTH_IN_BYTES,
            "BUG: Principal longer than spec allows"
        );
        slot.fill(0);
        slot[0] = bytes.len() as u8;
        slot[1..1 + bytes.len()].copy_from_slice(bytes);
    }

    fn decode_principal_from(slot: &[u8]) -> Principal {
        debug_assert_eq!(slot.len(), Self::PRINCIPAL_SLOT);
        let len = slot[0] as usize;
        assert!(
            len <= Principal::MAX_LENGTH_IN_BYTES,
            "BUG: encoded principal length exceeds spec"
        );
        Principal::from_slice(&slot[1..1 + len])
    }
}

impl Storable for BalanceKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; Self::ENCODED_SIZE as usize];
        let (token_slot, owner_slot) = buf.split_at_mut(Self::PRINCIPAL_SLOT);
        Self::encode_principal_into(token_slot, self.token.as_principal());
        Self::encode_principal_into(owner_slot, &self.owner);
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            Self::ENCODED_SIZE as usize,
            "BalanceKey must decode from exactly {} bytes",
            Self::ENCODED_SIZE
        );
        let (token_slot, owner_slot) = bytes.split_at(Self::PRINCIPAL_SLOT);
        Self {
            token: TokenId::new(Self::decode_principal_from(token_slot)),
            owner: Self::decode_principal_from(owner_slot),
        }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: Self::ENCODED_SIZE,
        is_fixed_size: true,
    };
}
