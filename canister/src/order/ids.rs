//! Shared machinery for the canister's opaque id newtypes.
//!
//! Two shapes repeat across the order book: a `u64` per-book *sequence* newtype
//! minted by an incrementing counter, and a fixed-width *composite id* that
//! concatenates two such fixed-width components and renders as an opaque hex
//! string. Both are expressed once, generically: [`Seq<M>`] is the sequence
//! newtype parameterized by a zero-sized [`SeqMarker`], and [`Composite<A, B>`]
//! pairs any two [`HexComponent`]s. Because a [`Composite`] is itself a
//! [`HexComponent`], the composites nest — `TradeId` is a `Composite` whose
//! first field is the `OrderId` `Composite`. The concrete ids
//! ([`OrderSeq`](super::OrderSeq) / [`OrderId`](super::OrderId) and the fill
//! identity types) are thin newtypes over these, so they stay byte- and
//! format-identical without duplicating their boilerplate.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;

/// A value that serializes as a fixed run of big-endian bytes and renders as
/// the equivalent lowercase hex. Sharing this codec across the sequence and
/// composite types keeps their `Display`, `FromStr` and `Storable` byte order
/// in lockstep — fixed-width big-endian fields make the field-wise `Ord` match
/// the [`Storable`] byte order that `StableBTreeMap` relies on.
pub(crate) trait HexComponent: Copy + Ord {
    /// Width of the big-endian byte serialization. `WIDTH * 2` is the hex
    /// length; both are compile-time constants so composites can sum them.
    const WIDTH: usize;

    fn write_be_bytes(&self, out: &mut Vec<u8>);

    fn write_hex(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    /// Parse from exactly `WIDTH * 2` lowercase/uppercase hex characters,
    /// `None` on any malformed input. Callers have already length-checked.
    fn from_hex(s: &str) -> Option<Self>;

    fn from_be_bytes(bytes: &[u8]) -> Self;
}

/// Names a [`Seq`] family so the overflow panic can report the concrete id.
pub trait SeqMarker {
    const NAME: &'static str;
}

/// A `u64`-backed, CBOR-encoded per-book sequence newtype with the shared
/// `new`/`get`/`increment` surface and a `ZERO` constant, distinguished by a
/// zero-sized [`SeqMarker`].
pub struct Seq<M>(u64, PhantomData<M>);

impl<M> Seq<M> {
    pub const ZERO: Self = Self(0, PhantomData);
    pub const ONE: Self = Self(1, PhantomData);

    pub const fn new(seq: u64) -> Self {
        Self(seq, PhantomData)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl<M: SeqMarker> Seq<M> {
    pub fn increment(&mut self) {
        self.0 = self
            .0
            .checked_add(1)
            .unwrap_or_else(|| panic!("{} overflow", M::NAME));
    }
}

impl<M> Clone for Seq<M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M> Copy for Seq<M> {}

impl<M> Default for Seq<M> {
    fn default() -> Self {
        Self::ZERO
    }
}

impl<M> fmt::Debug for Seq<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Seq").field(&self.0).finish()
    }
}

impl<M> PartialEq for Seq<M> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<M> Eq for Seq<M> {}

impl<M> PartialOrd for Seq<M> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<M> Ord for Seq<M> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<M> std::hash::Hash for Seq<M> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<C, M> minicbor::Encode<C> for Seq<M> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(1)?;
        e.u64(self.0)?;
        Ok(())
    }
}

impl<'b, C, M> minicbor::Decode<'b, C> for Seq<M> {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        let len = d.array()?;
        if len != Some(1) {
            return Err(minicbor::decode::Error::message(
                "expected array(1) for Seq",
            ));
        }
        Ok(Self::new(d.u64()?))
    }
}

impl<M: Copy + 'static> HexComponent for Seq<M> {
    const WIDTH: usize = 8;

    fn write_be_bytes(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0.to_be_bytes());
    }

    fn write_hex(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }

    fn from_hex(s: &str) -> Option<Self> {
        u64::from_str_radix(s, 16).ok().map(Self::new)
    }

    fn from_be_bytes(bytes: &[u8]) -> Self {
        Self::new(u64::from_be_bytes(bytes.try_into().expect("8-byte slice")))
    }
}

/// A fixed-width composite id pairing two [`HexComponent`]s, rendered as the
/// concatenation of their hex. Because `Composite` is itself a [`HexComponent`]
/// (when both fields are), it nests: `TradeId`'s first field is the `OrderId`
/// composite.
pub(crate) struct Composite<A, B> {
    first: A,
    second: B,
}

impl<A, B> Composite<A, B> {
    pub const fn new(first: A, second: B) -> Self {
        Self { first, second }
    }
}

impl<A: Copy, B: Copy> Composite<A, B> {
    pub fn first(&self) -> A {
        self.first
    }

    pub fn second(&self) -> B {
        self.second
    }
}

impl<A: Clone, B: Clone> Clone for Composite<A, B> {
    fn clone(&self) -> Self {
        Self {
            first: self.first.clone(),
            second: self.second.clone(),
        }
    }
}

impl<A: Copy, B: Copy> Copy for Composite<A, B> {}

impl<A: fmt::Debug, B: fmt::Debug> fmt::Debug for Composite<A, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Composite")
            .field("first", &self.first)
            .field("second", &self.second)
            .finish()
    }
}

impl<A: PartialEq, B: PartialEq> PartialEq for Composite<A, B> {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first && self.second == other.second
    }
}

impl<A: Eq, B: Eq> Eq for Composite<A, B> {}

impl<A: Ord, B: Ord> PartialOrd for Composite<A, B> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<A: Ord, B: Ord> Ord for Composite<A, B> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.first
            .cmp(&other.first)
            .then_with(|| self.second.cmp(&other.second))
    }
}

impl<A: std::hash::Hash, B: std::hash::Hash> std::hash::Hash for Composite<A, B> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.first.hash(state);
        self.second.hash(state);
    }
}

impl<A: HexComponent, B: HexComponent> fmt::Display for Composite<A, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.first.write_hex(f)?;
        self.second.write_hex(f)
    }
}

impl<A: HexComponent, B: HexComponent> HexComponent for Composite<A, B> {
    const WIDTH: usize = A::WIDTH + B::WIDTH;

    fn write_be_bytes(&self, out: &mut Vec<u8>) {
        self.first.write_be_bytes(out);
        self.second.write_be_bytes(out);
    }

    fn write_hex(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.first.write_hex(f)?;
        self.second.write_hex(f)
    }

    fn from_hex(s: &str) -> Option<Self> {
        let split = A::WIDTH * 2;
        let first = A::from_hex(&s[..split])?;
        let second = B::from_hex(&s[split..])?;
        Some(Self::new(first, second))
    }

    fn from_be_bytes(bytes: &[u8]) -> Self {
        let split = A::WIDTH;
        Self::new(
            A::from_be_bytes(&bytes[..split]),
            B::from_be_bytes(&bytes[split..]),
        )
    }
}

impl<A: HexComponent, B: HexComponent> Storable for Composite<A, B> {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = Vec::with_capacity(Self::WIDTH);
        self.write_be_bytes(&mut buf);
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            Self::WIDTH,
            "composite id must decode from exactly {} bytes",
            Self::WIDTH
        );
        <Self as HexComponent>::from_be_bytes(bytes)
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: <Composite<A, B> as HexComponent>::WIDTH as u32,
        is_fixed_size: true,
    };
}

impl<Ctx, A: minicbor::Encode<Ctx>, B: minicbor::Encode<Ctx>> minicbor::Encode<Ctx>
    for Composite<A, B>
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(2)?;
        self.first.encode(e, ctx)?;
        self.second.encode(e, ctx)?;
        Ok(())
    }
}

impl<'b, Ctx, A: minicbor::Decode<'b, Ctx>, B: minicbor::Decode<'b, Ctx>> minicbor::Decode<'b, Ctx>
    for Composite<A, B>
{
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        ctx: &mut Ctx,
    ) -> Result<Self, minicbor::decode::Error> {
        let len = d.array()?;
        if len != Some(2) {
            return Err(minicbor::decode::Error::message(
                "expected array(2) for Composite",
            ));
        }
        let first = A::decode(d, ctx)?;
        let second = B::decode(d, ctx)?;
        Ok(Self::new(first, second))
    }
}

/// Parse a [`HexComponent`] from a hex string, enforcing the exact `WIDTH * 2`
/// length and ASCII-hex content. Shared by the concrete ids' `FromStr` so the
/// length and hex checks stay identical across them.
pub(crate) fn parse_hex<T: HexComponent>(s: &str) -> Option<T> {
    if s.len() != T::WIDTH * 2 || !s.is_ascii() {
        return None;
    }
    T::from_hex(s)
}
