//! Maps user identities (currently a `Principal`) to a compact, stable
//! [`UserId`], so per-user stable keys — balances and the per-user order index —
//! store an 8-byte id instead of the full identity. When subaccounts land only
//! the registry's key type changes; the id-keyed maps are unaffected.

#[cfg(test)]
mod tests;

use crate::ids::{Seq, SeqMarker};
use candid::Principal;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;

/// Marker distinguishing the user-id family of [`Seq`].
#[derive(Debug, Clone, Copy)]
pub struct UserIdMarker;

impl SeqMarker for UserIdMarker {
    const NAME: &'static str = "UserId";
}

/// Compact, stable handle for a user identity. Assigned densely (`0..n`) by
/// [`UserRegistry`] and never reused — identities are never removed.
pub type UserId = Seq<UserIdMarker>;

/// Point-lookup key wrapping a `Principal` (the registry never range-scans, so
/// no order-preserving encoding is needed — CBOR like `BalanceKey`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, minicbor::Encode, minicbor::Decode)]
struct PrincipalKey(#[cbor(n(0), with = "icrc_cbor::principal")] Principal);

impl Storable for PrincipalKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("principal key encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("principal key encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode principal key bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// Maps each user identity to a dense, stable [`UserId`]. Lives in its own
/// stable-memory region and survives upgrades on its own.
pub struct UserRegistry<M: Memory> {
    users: StableBTreeMap<PrincipalKey, UserId, M>,
}

impl<M: Memory> std::fmt::Debug for UserRegistry<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserRegistry")
            .field("len", &self.users.len())
            .finish()
    }
}

impl<M: Memory> UserRegistry<M> {
    pub fn new(memory: M) -> Self {
        Self {
            users: StableBTreeMap::init(memory),
        }
    }

    /// Returns `principal`'s id, assigning a fresh one the first time the
    /// identity is seen.
    pub fn get_or_register(&mut self, principal: Principal) -> UserId {
        let key = PrincipalKey(principal);
        if let Some(id) = self.users.get(&key) {
            return id;
        }
        // No removals ⇒ live entries are exactly the ids `0..len`, so `len` is
        // the next free id.
        let id = UserId::new(self.users.len());
        self.users.insert(key, id);
        id
    }

    /// Returns `principal`'s id if it has been registered, without assigning
    /// one. Read paths use this so that merely querying a never-seen principal
    /// does not create an entry.
    pub fn lookup(&self, principal: Principal) -> Option<UserId> {
        self.users.get(&PrincipalKey(principal))
    }
}

#[cfg(test)]
impl UserRegistry<ic_stable_structures::VectorMemory> {
    fn iter(&self) -> impl Iterator<Item = (PrincipalKey, UserId)> + '_ {
        self.users
            .iter()
            .map(|entry| (entry.key().clone(), entry.value()))
    }
}

#[cfg(test)]
impl Clone for UserRegistry<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(ic_stable_structures::VectorMemory::default());
        for (key, id) in self.iter() {
            fresh.users.insert(key, id);
        }
        fresh
    }
}

#[cfg(test)]
impl PartialEq for UserRegistry<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
impl Eq for UserRegistry<ic_stable_structures::VectorMemory> {}
