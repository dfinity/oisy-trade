#[cfg(test)]
mod tests;

/// CBOR codec for a `Vec<Principal>`, encoding each principal via
/// [`icrc_cbor::principal`]. Mirrors `oisy_trade_types_internal`'s
/// `btreeset_principal` codec.
pub mod vec_principal {
    use candid::Principal;
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};
    use minicbor::{Decode, Encode};

    #[derive(Decode, Encode)]
    #[cbor(transparent)]
    struct CborPrincipal(#[cbor(n(0), with = "icrc_cbor::principal")] pub Principal);

    pub fn decode<Ctx>(d: &mut Decoder<'_>, ctx: &mut Ctx) -> Result<Vec<Principal>, Error> {
        Ok(Vec::<CborPrincipal>::decode(d, ctx)?
            .into_iter()
            .map(|p| p.0)
            .collect())
    }

    pub fn encode<Ctx, W: Write>(
        v: &[Principal],
        e: &mut Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let vec: Vec<CborPrincipal> = v.iter().copied().map(CborPrincipal).collect();
        vec.encode(e, ctx)
    }
}

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

/// CBOR codec for a `u128` field that reuses [`crate::order::Quantity`]'s
/// u64-or-`PosBignum` encoding (a `u128` is a `Quantity` with a zero high
/// limb), so the bignum logic lives in exactly one place.
pub mod u128_via_quantity {
    use crate::order::Quantity;
    use minicbor::encode::Write;
    use minicbor::{Decode, Decoder, Encode, Encoder};

    pub fn encode<Ctx, W: Write>(
        v: &u128,
        e: &mut Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        Quantity::from_u128(*v).encode(e, ctx)
    }

    pub fn decode<Ctx>(
        d: &mut Decoder<'_>,
        ctx: &mut Ctx,
    ) -> Result<u128, minicbor::decode::Error> {
        Quantity::decode(d, ctx)?
            .as_u128()
            .ok_or_else(|| minicbor::decode::Error::message("value exceeds u128"))
    }
}

/// CBOR codec for a `NonZeroU128` field, layered on [`u128_via_quantity`].
pub mod non_zero_u128_via_quantity {
    use super::u128_via_quantity;
    use minicbor::encode::Write;
    use minicbor::{Decoder, Encoder};
    use std::num::NonZeroU128;

    pub fn encode<Ctx, W: Write>(
        v: &NonZeroU128,
        e: &mut Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        u128_via_quantity::encode(&v.get(), e, ctx)
    }

    pub fn decode<Ctx>(
        d: &mut Decoder<'_>,
        ctx: &mut Ctx,
    ) -> Result<NonZeroU128, minicbor::decode::Error> {
        let v = u128_via_quantity::decode(d, ctx)?;
        NonZeroU128::new(v)
            .ok_or_else(|| minicbor::decode::Error::message("expected non-zero u128"))
    }
}
