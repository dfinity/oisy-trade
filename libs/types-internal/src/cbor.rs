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

#[cfg(test)]
mod tests {
    use super::btreeset_principal;
    use candid::Principal;
    use proptest::collection::{btree_set, vec as prop_vec};
    use proptest::prelude::{Strategy, any, prop_assert_eq, proptest};

    fn arb_principal() -> impl Strategy<Value = Principal> {
        prop_vec(any::<u8>(), 0..=29).prop_map(|bytes| Principal::from_slice(&bytes))
    }

    proptest! {
        #[test]
        fn btreeset_principal_roundtrips(principals in btree_set(arb_principal(), 0..=8)) {
            let mut buf = vec![];
            btreeset_principal::encode(&principals, &mut minicbor::Encoder::new(&mut buf), &mut ())
                .unwrap();
            let decoded =
                btreeset_principal::decode(&mut minicbor::Decoder::new(&buf), &mut ()).unwrap();
            prop_assert_eq!(decoded, principals);
        }
    }
}
