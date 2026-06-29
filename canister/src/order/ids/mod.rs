use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use minicbor::encode::{Error, Write};
use minicbor::{Decoder, Encoder};
use std::borrow::Cow;
use std::fmt;
use std::fmt::Formatter;
use std::marker::PhantomData;

#[cfg(test)]
mod tests;

/// Names a [`Seq`] to report the concrete id.
pub trait SeqMarker {
    const NAME: &'static str;
}

/// A monotonically increasing identifier backed by a u64.
///
/// The `M` type parameter is a [`SeqMarker`] that distinguishes otherwise
/// identical sequence families, so an id of one kind cannot be mixed up with
/// another at compile time. [`OrderSeq`](crate::order::OrderSeq) is one such
/// concrete instantiation:
///
/// ```
/// use oisy_trade_canister::order::OrderSeq;
///
/// let mut seq = OrderSeq::ZERO;
/// assert_eq!(seq.get(), 0);
///
/// seq.increment();
/// assert_eq!(seq, OrderSeq::ONE);
/// assert_eq!(seq.get(), 1);
/// ```
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

impl<M: SeqMarker> fmt::Debug for Seq<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(M::NAME).field(&self.0).finish()
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

impl<C, M> minicbor::Encode<C> for Seq<M> {
    fn encode<W: Write>(&self, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), Error<W::Error>> {
        e.u64(self.0)?;
        Ok(())
    }
}

impl<'b, C, M> minicbor::Decode<'b, C> for Seq<M> {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.u64().map(Self::new)
    }
}

/// An identifier represented by a fix number of bytes.
pub trait FixedWidthId: Sized {
    /// Number of bytes taken by the identifier
    const WIDTH: usize;

    fn write_be_bytes(&self, bytes: &mut Vec<u8>);
    fn from_be_bytes(bytes: &[u8]) -> Result<Self, ParseFixedWithIdError>;

    fn write_hex(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn from_hex(s: &str) -> Result<Self, ParseFixedWithIdError>;
}

pub struct ParseFixedWithIdError {}

impl<M> FixedWidthId for Seq<M> {
    const WIDTH: usize = 8;

    fn write_be_bytes(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.0.to_be_bytes());
    }

    fn from_be_bytes(bytes: &[u8]) -> Result<Self, ParseFixedWithIdError> {
        Ok(Self::new(u64::from_be_bytes(
            bytes.try_into().map_err(|_e| ParseFixedWithIdError {})?,
        )))
    }

    fn write_hex(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }

    fn from_hex(s: &str) -> Result<Self, ParseFixedWithIdError> {
        if s.len() != 2 * Self::WIDTH || !s.is_ascii() {
            return Err(ParseFixedWithIdError {});
        }
        u64::from_str_radix(s, 16)
            .map(Self::new)
            .map_err(|_e| ParseFixedWithIdError {})
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, minicbor::Encode, minicbor::Decode)]
pub struct CompositeId<A, B>(#[n(0)] A, #[n(1)] B);

impl<A: FixedWidthId, B: FixedWidthId> FixedWidthId for CompositeId<A, B> {
    const WIDTH: usize = A::WIDTH + B::WIDTH;

    fn write_be_bytes(&self, bytes: &mut Vec<u8>) {
        self.0.write_be_bytes(bytes);
        self.1.write_be_bytes(bytes);
    }

    fn from_be_bytes(bytes: &[u8]) -> Result<Self, ParseFixedWithIdError> {
        if bytes.len() != Self::WIDTH {
            return Err(ParseFixedWithIdError {});
        }
        let split = A::WIDTH;
        Ok(Self(
            A::from_be_bytes(&bytes[..split])?,
            B::from_be_bytes(&bytes[split..])?,
        ))
    }

    fn write_hex(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.write_hex(f)?;
        self.1.write_hex(f)
    }

    fn from_hex(s: &str) -> Result<Self, ParseFixedWithIdError> {
        if s.len() != 2 * Self::WIDTH || !s.is_ascii() {
            return Err(ParseFixedWithIdError {});
        }
        let split = A::WIDTH * 2;
        let first = A::from_hex(&s[..split])?;
        let second = B::from_hex(&s[split..])?;
        Ok(Self(first, second))
    }
}

impl<A: FixedWidthId, B: FixedWidthId> Storable for CompositeId<A, B> {
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
        Self::from_be_bytes(bytes)
            .unwrap_or_else(|_e| panic!("BUG: expected exactly {} bytes.", Self::WIDTH))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: Self::WIDTH as u32,
        is_fixed_size: true,
    };
}
