use std::fmt;

/// A rate in basis points (1 bps = 0.01 %). Constructed only via
/// [`BasisPoint::new`], which enforces `value <= 10_000`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, minicbor::Encode, minicbor::Decode)]
pub struct BasisPoint(#[n(0)] u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidBasisPoint {
    OutOfRange(u16),
}

impl fmt::Display for InvalidBasisPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange(v) => write!(
                f,
                "basis point {v} out of range (max {})",
                BasisPoint::MAX.0
            ),
        }
    }
}

impl BasisPoint {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(10_000);

    pub const fn new(value: u16) -> Result<Self, InvalidBasisPoint> {
        if value > Self::MAX.0 {
            return Err(InvalidBasisPoint::OutOfRange(value));
        }
        Ok(Self(value))
    }
}

/// Maker/taker fee rates for a trading pair. Each rate is a [`BasisPoint`]
/// (`0..=10_000`); zero means "no fee on that role".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct FeeRates {
    #[n(0)]
    pub maker: BasisPoint,
    #[n(1)]
    pub taker: BasisPoint,
}
