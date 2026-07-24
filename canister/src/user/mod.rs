//! Maps user identities (currently a `Principal`) to a compact, stable
//! [`UserId`], so per-user stable keys — balances and the per-user order index —
//! store an 8-byte id instead of the full identity. When subaccounts land only
//! the registry's key type changes; the id-keyed maps are unaffected.

#[cfg(test)]
mod tests;

use crate::Timestamp;
use crate::ids::{Seq, SeqMarker};
use candid::Principal;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;
use std::time::Duration;

/// Maximum number of trading accounts a single funding account may whitelist.
/// Hyperliquid grants one unnamed plus three named agents; no known integrator
/// needs more, and the cap is trivially raisable later.
pub const MAX_TRADING_ACCOUNTS_PER_USER: usize = 4;

/// Minimum time between two successful grants by the same funding account.
/// Key rotation happens on a timescale of weeks, so an hour between grants costs
/// legitimate users nothing while bounding whitelist-write amplification.
pub const TRADING_ACCOUNT_GRANT_COOLDOWN: Duration = Duration::from_secs(60 * 60);

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

/// A trading account's standing authorization.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct TradingGrant {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    funding: Principal,
}

impl Storable for TradingGrant {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("trading grant encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("trading grant encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode trading grant bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// A funding account's whitelist and its grant-cooldown anchor.
#[derive(Debug, Clone, PartialEq, Eq, Default, minicbor::Encode, minicbor::Decode)]
pub struct TradingAccountList {
    #[cbor(n(0), with = "crate::cbor::vec_principal")]
    accounts: Vec<Principal>,
    #[n(1)]
    last_granted_at: Timestamp,
}

impl Storable for TradingAccountList {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf)
            .expect("trading account list encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf)
            .expect("trading account list encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode trading account list bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// A funding account principal — the account a grant acts on behalf of. A
/// distinct type from [`TradingAccount`] so the two same-shaped arguments of
/// [`UserRegistry::validate_add_trading_account`] /
/// [`UserRegistry::record_add_trading_account`] cannot be transposed by
/// accident. Encodes as an indexed-field struct so it can grow fields later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct FundingAccount(#[cbor(n(0), with = "icrc_cbor::principal")] pub Principal);

/// A trading account principal — the key being whitelisted. See
/// [`FundingAccount`] for why this is a distinct newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct TradingAccount(#[cbor(n(0), with = "icrc_cbor::principal")] pub Principal);

/// Why [`UserRegistry::validate_add_trading_account`] rejected a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantError {
    /// The funding account is not a registered user.
    GranterNotRegistered,
    /// The funding account tried to whitelist itself.
    SelfGrant,
    /// The prospective trading account is already a trading account.
    AlreadyTradingAccount,
    /// The prospective trading account is already a registered user.
    AlreadyRegisteredUser,
    /// The funding account is itself a trading account.
    GranterIsTradingAccount,
    /// The funding account already holds the maximum number of trading
    /// accounts.
    TooManyTradingAccounts,
    /// The grant cooldown has not elapsed since the funding account's previous
    /// successful grant; the caller may retry after `retry_after_ns`
    /// nanoseconds.
    CooldownActive {
        /// Nanoseconds remaining until the caller may grant again.
        retry_after_ns: u64,
    },
}

/// Classification of a principal by the [`UserRegistry`]: either a funding
/// account (a registered user) or a trading account (a whitelisted delegate).
/// The two are mutually exclusive — a trading account is never registered — so
/// [`UserRegistry::lookup`] returns exactly one, or `None` for an unknown
/// principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAccount {
    /// A registered user, holding balances, orders, and trades under `id`.
    Funding {
        /// The compact, stable id keying the user's per-account state.
        id: UserId,
        /// The user's own principal.
        principal: Principal,
    },
    /// A whitelisted trading principal acting on its funding account's behalf.
    Trading {
        /// The trading principal itself.
        principal: Principal,
        /// The standing authorization naming the funding account it acts for.
        grant: TradingGrant,
    },
}

impl UserAccount {
    /// The account whose data this principal reads and acts on: a funding
    /// account is itself; a trading account is its funding account.
    pub fn effective_principal(&self) -> Principal {
        match self {
            UserAccount::Funding { principal, .. } => *principal,
            UserAccount::Trading { grant, .. } => grant.funding,
        }
    }

    /// The order owner and the acting key to attribute a placement or cancel
    /// to: a funding account acts as itself with no separate attribution; a
    /// trading account acts on its funding account's behalf, attributed to the
    /// trading principal.
    pub fn order_actor(&self) -> (Principal, Option<Principal>) {
        match self {
            UserAccount::Funding { principal, .. } => (*principal, None),
            UserAccount::Trading { principal, grant } => (grant.funding, Some(*principal)),
        }
    }

    /// This account's funding [`UserId`], present only for a funding account.
    pub fn funding_id(&self) -> Option<UserId> {
        match self {
            UserAccount::Funding { id, .. } => Some(*id),
            UserAccount::Trading { .. } => None,
        }
    }
}

/// Why [`UserRegistry::validate_remove_trading_account`] rejected a revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeError {
    /// The caller may not remove this trading account: it is not currently a
    /// trading account of the caller.
    NotAllowed,
}

/// Maps each user identity to a dense, stable [`UserId`], and holds the
/// per-funding-account trading-account whitelist. Lives in its own
/// stable-memory regions and survives upgrades on its own.
pub struct UserRegistry<M: Memory> {
    users: StableBTreeMap<PrincipalKey, UserId, M>,
    trading_accounts: StableBTreeMap<PrincipalKey, TradingGrant, M>,
    trading_accounts_by_funding: StableBTreeMap<UserId, TradingAccountList, M>,
}

impl<M: Memory> std::fmt::Debug for UserRegistry<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserRegistry")
            .field("len", &self.users.len())
            .finish()
    }
}

impl<M: Memory> UserRegistry<M> {
    /// `users_memory`, `trading_accounts_memory`, and
    /// `trading_accounts_by_funding_memory` **must be distinct memory
    /// regions**: the three maps share no isolation beyond their backing
    /// memory, so reusing a handle would let them overwrite each other.
    pub fn new(
        users_memory: M,
        trading_accounts_memory: M,
        trading_accounts_by_funding_memory: M,
    ) -> Self {
        Self {
            users: StableBTreeMap::init(users_memory),
            trading_accounts: StableBTreeMap::init(trading_accounts_memory),
            trading_accounts_by_funding: StableBTreeMap::init(trading_accounts_by_funding_memory),
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

    /// Classifies `principal` without assigning an id: a registered user is a
    /// [`UserAccount::Funding`], a whitelisted trading principal a
    /// [`UserAccount::Trading`], and an unknown principal `None`. Read paths use
    /// this so that merely querying a never-seen principal does not create an
    /// entry. `trading_accounts` is consulted first; the two maps are mutually
    /// exclusive (a trading account is never registered), asserted in debug
    /// builds. Preferring `Trading` on a (never-reachable) conflict keeps the
    /// safe default — such a principal stays denied funding operations.
    pub fn lookup(&self, principal: Principal) -> Option<UserAccount> {
        let key = PrincipalKey(principal);
        if let Some(grant) = self.trading_accounts.get(&key) {
            debug_assert!(
                !self.users.contains_key(&key),
                "BUG: principal is both a funding and a trading account"
            );
            return Some(UserAccount::Trading { principal, grant });
        }
        self.users
            .get(&key)
            .map(|id| UserAccount::Funding { id, principal })
    }

    /// Checks the grant preconditions for whitelisting `trading` under funding
    /// account `funding` at time `now`, without mutating anything. Encodes the
    /// identity and cap rules and the grant cooldown; the caller records the
    /// event and applies it via [`Self::record_add_trading_account`].
    pub fn validate_add_trading_account(
        &self,
        funding: FundingAccount,
        trading: TradingAccount,
        now: Timestamp,
    ) -> Result<(), GrantError> {
        let FundingAccount(funding) = funding;
        let TradingAccount(trading) = trading;
        // A trading account is unregistered by design, so the delegate-granter
        // case is reported before the registration case would swallow it as a
        // plain `GranterNotRegistered`.
        let funding_id = match self.lookup(funding) {
            Some(UserAccount::Trading { .. }) => return Err(GrantError::GranterIsTradingAccount),
            None => return Err(GrantError::GranterNotRegistered),
            Some(UserAccount::Funding { id, .. }) => id,
        };
        if trading == funding {
            return Err(GrantError::SelfGrant);
        }
        match self.lookup(trading) {
            Some(UserAccount::Trading { .. }) => return Err(GrantError::AlreadyTradingAccount),
            Some(UserAccount::Funding { .. }) => return Err(GrantError::AlreadyRegisteredUser),
            None => {}
        }
        if let Some(list) = self.trading_accounts_by_funding.get(&funding_id) {
            // Cap first (permanent) then cooldown (retryable): a permanent
            // rejection still wins over a retryable one.
            if list.accounts.len() >= MAX_TRADING_ACCOUNTS_PER_USER {
                return Err(GrantError::TooManyTradingAccounts);
            }
            let cooldown = TRADING_ACCOUNT_GRANT_COOLDOWN.as_nanos();
            let elapsed = u128::from(
                now.as_nanos()
                    .saturating_sub(list.last_granted_at.as_nanos()),
            );
            if elapsed < cooldown {
                return Err(GrantError::CooldownActive {
                    retry_after_ns: (cooldown - elapsed) as u64,
                });
            }
        }
        Ok(())
    }

    /// Checks that `trading` is currently a trading account of `funding` (the
    /// revoke precondition), without mutating anything. Revocation is never
    /// rate-limited.
    pub fn validate_remove_trading_account(
        &self,
        funding: FundingAccount,
        trading: TradingAccount,
    ) -> Result<(), RevokeError> {
        let FundingAccount(funding) = funding;
        let TradingAccount(trading) = trading;
        match self.trading_accounts.get(&PrincipalKey(trading)) {
            Some(grant) if grant.funding == funding => Ok(()),
            _ => Err(RevokeError::NotAllowed),
        }
    }

    /// Records a grant of `trading` under funding account `funding`, stamping
    /// `now` as the grant-cooldown anchor. Preconditions must already have been
    /// checked via [`Self::validate_add_trading_account`]; `funding` must be
    /// registered and `trading` must be a fresh key (adds are not idempotent).
    pub fn record_add_trading_account(
        &mut self,
        funding: FundingAccount,
        trading: TradingAccount,
        now: Timestamp,
    ) {
        let FundingAccount(funding) = funding;
        let TradingAccount(trading) = trading;
        let funding_id = self
            .lookup(funding)
            .and_then(|account| account.funding_id())
            .expect("BUG: record_add_trading_account on an unregistered funding account");
        let previous = self
            .trading_accounts
            .insert(PrincipalKey(trading), TradingGrant { funding });
        debug_assert!(
            previous.is_none(),
            "BUG: record_add_trading_account overwrote an existing trading account"
        );
        let mut list = self
            .trading_accounts_by_funding
            .get(&funding_id)
            .unwrap_or_default();
        list.accounts.push(trading);
        list.last_granted_at = now;
        self.trading_accounts_by_funding.insert(funding_id, list);
    }

    /// Revokes `trading` from funding account `funding`: deletes its
    /// `trading_accounts` entry and drops it from `funding`'s list. The
    /// [`TradingAccountList`] entry itself is **never removed, only shrunk** —
    /// the `last_granted_at` cooldown anchor must survive so revoke-all →
    /// re-grant cannot bypass the cooldown. Preconditions must already have been
    /// checked via [`Self::validate_remove_trading_account`].
    pub fn record_remove_trading_account(
        &mut self,
        funding: FundingAccount,
        trading: TradingAccount,
    ) {
        let FundingAccount(funding) = funding;
        let TradingAccount(trading) = trading;
        let funding_id = self
            .lookup(funding)
            .and_then(|account| account.funding_id())
            .expect("BUG: record_remove_trading_account on an unregistered funding account");
        let removed = self
            .trading_accounts
            .remove(&PrincipalKey(trading))
            .expect("BUG: record_remove_trading_account removed no grant");
        debug_assert_eq!(
            removed.funding, funding,
            "BUG: record_remove_trading_account on a grant owned by a different funding account"
        );
        let mut list = self.trading_accounts_by_funding.get(&funding_id).expect(
            "BUG: record_remove_trading_account on a funding account with no trading-account list",
        );
        list.accounts.retain(|p| *p != trading);
        self.trading_accounts_by_funding.insert(funding_id, list);
    }

    /// Returns `funding`'s current whitelist (empty if it has granted none, or
    /// is unregistered). Acts on the raw principal; never resolves delegation.
    pub fn trading_accounts_of(&self, funding: Principal) -> Vec<Principal> {
        self.lookup(funding)
            .and_then(|account| account.funding_id())
            .and_then(|funding_id| self.trading_accounts_by_funding.get(&funding_id))
            .map(|list| list.accounts)
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl UserRegistry<ic_stable_structures::VectorMemory> {
    fn iter(&self) -> impl Iterator<Item = (PrincipalKey, UserId)> + '_ {
        self.users
            .iter()
            .map(|entry| (entry.key().clone(), entry.value()))
    }

    fn iter_trading_accounts(&self) -> impl Iterator<Item = (PrincipalKey, TradingGrant)> + '_ {
        self.trading_accounts
            .iter()
            .map(|entry| (entry.key().clone(), entry.value()))
    }

    fn iter_trading_accounts_by_funding(
        &self,
    ) -> impl Iterator<Item = (UserId, TradingAccountList)> + '_ {
        self.trading_accounts_by_funding
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }
}

#[cfg(test)]
impl Clone for UserRegistry<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
        );
        for (key, id) in self.iter() {
            fresh.users.insert(key, id);
        }
        for (key, grant) in self.iter_trading_accounts() {
            fresh.trading_accounts.insert(key, grant);
        }
        for (funding_id, list) in self.iter_trading_accounts_by_funding() {
            fresh.trading_accounts_by_funding.insert(funding_id, list);
        }
        fresh
    }
}

#[cfg(test)]
impl PartialEq for UserRegistry<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
            && self
                .iter_trading_accounts()
                .eq(other.iter_trading_accounts())
            && self
                .iter_trading_accounts_by_funding()
                .eq(other.iter_trading_accounts_by_funding())
    }
}

#[cfg(test)]
impl Eq for UserRegistry<ic_stable_structures::VectorMemory> {}
