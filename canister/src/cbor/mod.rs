#[cfg(test)]
mod tests;

pub mod u128_codec {
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};

    pub fn decode<Ctx>(d: &mut Decoder<'_>, _ctx: &mut Ctx) -> Result<u128, Error> {
        let pos = d.position();
        match d.u64() {
            Ok(n) => return Ok(n as u128),
            Err(e) if e.is_type_mismatch() => d.set_position(pos),
            Err(e) => return Err(e),
        }
        let tag = d.tag()?;
        if tag != minicbor::data::Tag::PosBignum {
            return Err(Error::message("expected u64 or PosBignum for u128"));
        }
        let bytes = d.bytes()?;
        if bytes.len() > 16 {
            return Err(Error::message("value exceeds u128"));
        }
        let mut buf = [0u8; 16];
        buf[16 - bytes.len()..].copy_from_slice(bytes);
        Ok(u128::from_be_bytes(buf))
    }

    pub fn encode<Ctx, W: Write>(
        v: &u128,
        e: &mut Encoder<W>,
        _ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        if *v <= u64::MAX as u128 {
            e.u64(*v as u64)?;
        } else {
            let buf = v.to_be_bytes();
            let start = buf.iter().position(|&b| b != 0).unwrap_or(buf.len());
            e.tag(minicbor::data::Tag::PosBignum)?
                .bytes(&buf[start..])?;
        }
        Ok(())
    }
}

pub mod non_zero_u128 {
    use super::u128_codec;
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};
    use std::num::NonZeroU128;

    pub fn decode<Ctx>(d: &mut Decoder<'_>, ctx: &mut Ctx) -> Result<NonZeroU128, Error> {
        let v = u128_codec::decode(d, ctx)?;
        NonZeroU128::new(v).ok_or_else(|| Error::message("expected non-zero u128"))
    }

    pub fn encode<Ctx, W: Write>(
        v: &NonZeroU128,
        e: &mut Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        u128_codec::encode(&v.get(), e, ctx)
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
