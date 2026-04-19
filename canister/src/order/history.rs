use super::{OrderId, Price, Quantity, Side};
use candid::Principal;
use dex_types::OrderStatus;
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
    #[cbor(n(4), with = "crate::cbor::order_status")]
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

impl Storable for OrderId {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let (book, seq) = self.into_parts();
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&book.get().to_be_bytes());
        buf[8..].copy_from_slice(&seq.get().to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(bytes.len(), 16, "OrderId must decode from exactly 16 bytes");
        let book = u64::from_be_bytes(bytes[..8].try_into().expect("8-byte slice"));
        let seq = u64::from_be_bytes(bytes[8..].try_into().expect("8-byte slice"));
        OrderId::new(super::OrderBookId::new(book), super::OrderSeq::new(seq))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 16,
        is_fixed_size: true,
    };
}
