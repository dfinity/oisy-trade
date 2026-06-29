use minicbor::encode::{Error, Write};
use minicbor::{Decoder, Encoder};
use std::fmt;
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
