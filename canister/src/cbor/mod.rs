#[cfg(test)]
mod tests;

pub mod order_status {
    //! CBOR codec for [`dex_types::OrderStatus`] used by stored
    //! [`crate::order::OrderRecord`]s. `NotFound` is synthesized by the
    //! `order_history` accessor when a record is missing — it must never reach
    //! this codec.
    use dex_types::OrderStatus;
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};

    const PENDING: u8 = 1;
    const OPEN: u8 = 2;
    const FILLED: u8 = 3;
    const CANCELED: u8 = 4;

    pub fn decode<Ctx>(d: &mut Decoder<'_>, _ctx: &mut Ctx) -> Result<OrderStatus, Error> {
        match d.u8()? {
            PENDING => Ok(OrderStatus::Pending),
            OPEN => Ok(OrderStatus::Open),
            FILLED => Ok(OrderStatus::Filled),
            CANCELED => Ok(OrderStatus::Canceled),
            other => Err(Error::message(format!("unknown order status tag: {other}"))),
        }
    }

    pub fn encode<Ctx, W: Write>(
        v: &OrderStatus,
        e: &mut Encoder<W>,
        _ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let tag = match v {
            OrderStatus::NotFound => {
                return Err(minicbor::encode::Error::message(
                    "BUG: OrderStatus::NotFound must never be persisted",
                ));
            }
            OrderStatus::Pending => PENDING,
            OrderStatus::Open => OPEN,
            OrderStatus::Filled => FILLED,
            OrderStatus::Canceled => CANCELED,
        };
        e.u8(tag)?;
        Ok(())
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
