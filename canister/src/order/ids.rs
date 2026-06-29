//! Shared machinery for the canister's opaque id newtypes.
//!
//! Two shapes repeat across the order book: a `u64` per-book *sequence* newtype
//! minted by an incrementing counter, and a fixed-width *composite id* that
//! concatenates an [`OrderBookId`](super::OrderBookId) with such a sequence and
//! renders as an opaque hex string. The [`seq_newtype!`] and [`book_scoped_id!`]
//! macros capture that shared behavior so [`OrderSeq`](super::OrderSeq) /
//! [`OrderId`](super::OrderId) and the fill identity types stay byte- and
//! format-identical without duplicating their boilerplate.

/// Declare a `u64`-backed, CBOR-encoded per-book sequence newtype with the
/// shared `new`/`get`/`increment` surface and a `ZERO` constant. Extra
/// associated constants (e.g. `ONE`) are passed as a trailing list. The
/// `increment` overflow panic names the type.
macro_rules! seq_newtype {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
        $(const $const_name:ident = $const_val:expr;)*
    ) => {
        $(#[$meta])*
        #[derive(
            Debug,
            Clone,
            Copy,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            minicbor::Encode,
            minicbor::Decode,
        )]
        $vis struct $name(#[n(0)] u64);

        impl $name {
            pub const ZERO: Self = Self(0);
            $(pub const $const_name: Self = Self($const_val);)*

            pub const fn new(seq: u64) -> Self {
                Self(seq)
            }

            pub fn get(self) -> u64 {
                self.0
            }

            pub fn increment(&mut self) {
                self.0 = self
                    .0
                    .checked_add(1)
                    .expect(concat!(stringify!($name), " overflow"));
            }
        }
    };
}

/// Declare a book-scoped composite id: an [`OrderBookId`](super::OrderBookId)
/// paired with a per-book sequence newtype, rendered as an opaque 32-character
/// hex string (8 big-endian bytes of book id + 8 of sequence).
///
/// Both fields are fixed-width big-endian, so the derived field-wise `Ord`
/// matches the [`Storable`](ic_stable_structures::Storable) byte order that
/// `StableBTreeMap` relies on. The macro emits the struct, the `book_id`/`seq`
/// accessors, its hex `Display`, a dedicated parse-error type with `FromStr`,
/// the `From<Self> for String` conversion, and a fixed-size `Storable`.
macro_rules! book_scoped_id {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident($seq:ty);
        $(derive($($extra_derive:path),+ $(,)?);)?
        $(field_attrs($(#[$book_attr:meta])* ; $(#[$seq_attr:meta])*);)?
        $(#[$err_meta:meta])*
        error $err:ident = $err_msg:literal;
        $(extra { $($extra:item)* })?
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash $($(, $extra_derive)+)?)]
        $vis struct $name {
            $($(#[$book_attr])*)? book_id: $crate::order::OrderBookId,
            $($(#[$seq_attr])*)? seq: $seq,
        }

        impl $name {
            pub fn new(book_id: $crate::order::OrderBookId, seq: $seq) -> Self {
                Self { book_id, seq }
            }

            pub fn book_id(&self) -> $crate::order::OrderBookId {
                self.book_id
            }

            pub fn seq(&self) -> $seq {
                self.seq
            }

            $($($extra)*)?
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:016x}{:016x}", self.book_id.get(), self.seq.get())
            }
        }

        $(#[$err_meta])*
        #[derive(Debug, PartialEq, Eq)]
        $vis struct $err;

        impl std::fmt::Display for $err {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, $err_msg)
            }
        }

        impl std::str::FromStr for $name {
            type Err = $err;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() != 32 || !s.is_ascii() {
                    return Err($err);
                }
                let book_id = u64::from_str_radix(&s[..16], 16).map_err(|_| $err)?;
                let seq = u64::from_str_radix(&s[16..], 16).map_err(|_| $err)?;
                Ok(Self {
                    book_id: $crate::order::OrderBookId::new(book_id),
                    seq: <$seq>::new(seq),
                })
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> Self {
                id.to_string()
            }
        }

        impl ic_stable_structures::Storable for $name {
            fn to_bytes(&self) -> std::borrow::Cow<'_, [u8]> {
                let mut buf = [0u8; 16];
                buf[..8].copy_from_slice(&self.book_id.get().to_be_bytes());
                buf[8..].copy_from_slice(&self.seq.get().to_be_bytes());
                std::borrow::Cow::Owned(buf.to_vec())
            }

            fn into_bytes(self) -> Vec<u8> {
                self.to_bytes().into_owned()
            }

            fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
                let bytes: &[u8] = bytes.as_ref();
                assert_eq!(
                    bytes.len(),
                    16,
                    concat!(stringify!($name), " must decode from exactly 16 bytes")
                );
                let book_id = u64::from_be_bytes(bytes[..8].try_into().expect("8-byte slice"));
                let seq = u64::from_be_bytes(bytes[8..].try_into().expect("8-byte slice"));
                Self::new(
                    $crate::order::OrderBookId::new(book_id),
                    <$seq>::new(seq),
                )
            }

            const BOUND: ic_stable_structures::storable::Bound =
                ic_stable_structures::storable::Bound::Bounded {
                    max_size: 16,
                    is_fixed_size: true,
                };
        }
    };
}

pub(crate) use {book_scoped_id, seq_newtype};
