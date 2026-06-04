use crate::order::Quantity;
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

    /// Compute `ceil(amount × self / 10_000)` as a u256 [`Quantity`].
    pub fn mul_ceil(self, amount: Quantity) -> Quantity {
        let bps = u64::from(self.0);
        if bps == 0 {
            return Quantity::ZERO;
        }
        let (q, r) = amount
            .checked_div_rem_u64(10_000)
            .expect("BUG: division by 10_000 is non-zero");
        let main = q
            .checked_mul_u64(bps)
            .expect("BUG: q × bps overflow despite q ≤ u256::MAX / 10_000 and bps ≤ 10_000");
        let rem_num = r
            .checked_mul(bps)
            .expect("BUG: r × bps fits in u64 — r < 10_000 and bps ≤ 10_000");
        let rem_ceil = rem_num.div_ceil(10_000);
        main.checked_add(Quantity::from(rem_ceil))
            .expect("BUG: ceiled fee overflowed u256 — impossible if amount fits in u256")
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
