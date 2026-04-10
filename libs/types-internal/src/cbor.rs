pub mod btreeset_principal {
    use candid::Principal;
    use minicbor::decode::{Decoder, Error};
    use minicbor::encode::{Encoder, Write};
    use minicbor::{Decode, Encode};
    use std::collections::BTreeSet;

    #[derive(Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
    #[cbor(transparent)]
    struct CborPrincipal(#[cbor(n(0), with = "icrc_cbor::principal")] pub Principal);

    pub fn decode<Ctx>(d: &mut Decoder<'_>, ctx: &mut Ctx) -> Result<BTreeSet<Principal>, Error> {
        Ok(Vec::<CborPrincipal>::decode(d, ctx)?
            .into_iter()
            .map(|p| p.0)
            .collect())
    }

    pub fn encode<Ctx, W: Write>(
        v: &BTreeSet<Principal>,
        e: &mut Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let vec: Vec<CborPrincipal> = v.iter().copied().map(CborPrincipal).collect();
        vec.encode(e, ctx)
    }
}
