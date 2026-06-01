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
    /// Rounding is up, in the protocol's favor — see the Fees section
    /// in `docs/design.md`.
    ///
    /// Decomposing `amount = q × 10_000 + r` with `q = amount / 10_000`
    /// and `r = amount % 10_000 < 10_000` gives
    ///
    /// ```text
    /// ceil(amount × bps / 10_000)
    ///     = q × bps + ceil((r × bps) / 10_000)
    /// ```
    ///
    /// which is overflow-safe at every step: `q × bps` fits in u256
    /// (`q ≤ u256::MAX / 10_000` and `bps ≤ 10_000`), and `r × bps`
    /// fits in u32 (`r < 10_000` and `bps ≤ 10_000`, so the product is
    /// below 10^8). A naive `(amount × bps) / 10_000` would trap on
    /// amounts in the top 1/10_000 of u256.
    pub fn apply_to(self, amount: Quantity) -> Quantity {
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
        let rem_num = u128::from(r) * u128::from(bps);
        let rem_ceil = (rem_num / 10_000 + u128::from(rem_num % 10_000 != 0)) as u64;
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
