use super::{OrderStatus, Price, Quantity, Side};
use candid::Principal;
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use std::borrow::Cow;

/// Record of an order from submission through terminal state.
///
/// Persisted in a [`ic_stable_structures::StableBTreeMap`] keyed by [`OrderId`],
/// so the CBOR layout is an upgrade-durable schema: removing or renumbering a
/// field breaks decoding of records written by prior canister versions. New
/// fields must be added with `#[cbor(n(N), default)]` or an `Option<T>` type.
/// The trading pair is deliberately not stored — it is derivable from the
/// `OrderBookId` embedded in the [`OrderId`] via the trading-pair registry.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct OrderRecord {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub owner: Principal,
    #[n(1)]
    pub side: Side,
    #[n(2)]
    pub price: Price,
    #[n(3)]
    pub quantity: Quantity,
    #[n(4)]
    pub status: OrderStatus,
}

impl Storable for OrderRecord {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("order record encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("order record encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode order record bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}
