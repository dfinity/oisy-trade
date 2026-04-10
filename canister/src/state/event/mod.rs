use crate::order::{LotSize, TickSize};
use candid::Principal;
use dex_types_internal::{InitArg, UpgradeArg};
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use minicbor::{Decode, Encode};
use std::borrow::Cow;

#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct Event {
    #[n(0)]
    pub timestamp: u64,
    #[n(1)]
    pub payload: EventType,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub enum EventType {
    #[n(0)]
    Init(#[n(0)] InitArg),
    #[n(1)]
    Upgrade(#[n(0)] UpgradeArg),
    #[n(2)]
    AddTradingPair(#[n(0)] AddTradingPairEvent),
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct AddTradingPairEvent {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub base: Principal,
    #[cbor(n(1), with = "icrc_cbor::principal")]
    pub quote: Principal,
    #[cbor(n(2), with = "cbor_tick_size")]
    pub tick_size: TickSize,
    #[cbor(n(3), with = "cbor_lot_size")]
    pub lot_size: LotSize,
}

mod cbor_tick_size {
    use super::*;
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};

    pub fn decode<Ctx>(d: &mut Decoder<'_>, _ctx: &mut Ctx) -> Result<TickSize, Error> {
        let v = d.u64()?;
        let nz =
            std::num::NonZeroU64::new(v).ok_or_else(|| Error::message("tick_size must be > 0"))?;
        Ok(TickSize::new(nz))
    }

    pub fn encode<Ctx, W: Write>(
        v: &TickSize,
        e: &mut Encoder<W>,
        _ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.u64(v.get())?;
        Ok(())
    }
}

mod cbor_lot_size {
    use super::*;
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};

    pub fn decode<Ctx>(d: &mut Decoder<'_>, _ctx: &mut Ctx) -> Result<LotSize, Error> {
        let v = d.u64()?;
        let nz =
            std::num::NonZeroU64::new(v).ok_or_else(|| Error::message("lot_size must be > 0"))?;
        Ok(LotSize::new(nz))
    }

    pub fn encode<Ctx, W: Write>(
        v: &LotSize,
        e: &mut Encoder<W>,
        _ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.u64(v.get())?;
        Ok(())
    }
}

impl Storable for Event {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("event encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("event encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode event bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}
