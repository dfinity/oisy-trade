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
