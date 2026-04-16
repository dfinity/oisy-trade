#[cfg(test)]
mod tests;

pub mod non_zero_u64 {
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};
    use std::num::NonZeroU64;

    pub fn decode<Ctx>(d: &mut Decoder<'_>, _ctx: &mut Ctx) -> Result<NonZeroU64, Error> {
        let v = d.u64()?;
        NonZeroU64::new(v).ok_or_else(|| Error::message("expected non-zero u64"))
    }

    pub fn encode<Ctx, W: Write>(
        v: &NonZeroU64,
        e: &mut Encoder<W>,
        _ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.u64(v.get())?;
        Ok(())
    }
}

/// Codec for `BTreeMap<Reverse<Price>, VecDeque<RestingOrder>>`.
///
/// minicbor has no built-in support for `Reverse<T>` as a map key,
/// so we manually encode/decode the map, unwrapping/wrapping `Reverse`
/// around each `Price` key.
pub mod reverse_price_map {
    use crate::order::{Price, RestingOrder};
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};
    use minicbor::{Decode, Encode};
    use std::cmp::Reverse;
    use std::collections::{BTreeMap, VecDeque};

    pub fn decode<Ctx>(
        d: &mut Decoder<'_>,
        ctx: &mut Ctx,
    ) -> Result<BTreeMap<Reverse<Price>, VecDeque<RestingOrder>>, Error> {
        let len = d
            .map()?
            .ok_or_else(|| Error::message("expected definite-length map"))?;
        let mut map = BTreeMap::new();
        for _ in 0..len {
            let price = Price::decode(d, ctx)?;
            let queue = VecDeque::<RestingOrder>::decode(d, ctx)?;
            map.insert(Reverse(price), queue);
        }
        Ok(map)
    }

    pub fn encode<Ctx, W: Write>(
        v: &BTreeMap<Reverse<Price>, VecDeque<RestingOrder>>,
        e: &mut Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.map(v.len() as u64)?;
        for (Reverse(price), queue) in v {
            price.encode(e, ctx)?;
            queue.encode(e, ctx)?;
        }
        Ok(())
    }
}
