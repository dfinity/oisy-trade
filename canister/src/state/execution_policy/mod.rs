use oisy_trade_types_internal::{
    DEFAULT_INSTRUCTION_BUDGET, DEFAULT_MAX_FILLS_PER_SETTLING_EVENT, DEFAULT_MAX_ORDERS_PER_CHUNK,
};
use std::num::{NonZeroU32, NonZeroU64};

#[cfg(test)]
mod tests;

/// Highest instruction budget the validator accepts. Rejects obvious
/// typos (e.g. `u64::MAX`) without constraining the production policy —
/// see [`Default`] for the value the canister actually ships with.
///
/// Spec: <https://docs.internetcomputer.org/references/resource-limits/#instruction-limits>
pub const MAX_INSTRUCTION_BUDGET: u64 = 40_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionPolicy {
    max_orders_per_chunk: NonZeroU32,
    instruction_budget: NonZeroU64,
    max_fills_per_settling_event: NonZeroU32,
}

impl ExecutionPolicy {
    /// Largest valid `ExecutionPolicy`: unbounded orders per chunk and the
    /// IC per-message instruction cap. Intended for tests and benchmarks
    /// that want to drain matching in a single `run_once`.
    pub const MAX: Self = Self {
        max_orders_per_chunk: NonZeroU32::MAX,
        instruction_budget: NonZeroU64::new(MAX_INSTRUCTION_BUDGET).unwrap(),
        max_fills_per_settling_event: NonZeroU32::MAX,
    };

    /// Build a validated `ExecutionPolicy`. Returns `Err` if any field
    /// is zero or if `instruction_budget` exceeds the IC system-subnet
    /// per-message cap.
    pub fn try_new(
        max_orders_per_chunk: u32,
        instruction_budget: u64,
        max_fills_per_settling_event: u32,
    ) -> Result<Self, String> {
        let max_orders_per_chunk = NonZeroU32::new(max_orders_per_chunk)
            .ok_or_else(|| "max_orders_per_chunk must be non-zero".to_string())?;
        let instruction_budget = NonZeroU64::new(instruction_budget)
            .ok_or_else(|| "instruction_budget must be non-zero".to_string())?;
        let max_fills_per_settling_event = NonZeroU32::new(max_fills_per_settling_event)
            .ok_or_else(|| "max_fills_per_settling_event must be non-zero".to_string())?;
        if instruction_budget.get() > MAX_INSTRUCTION_BUDGET {
            return Err(format!(
                "instruction_budget {} exceeds IC per-message cap ({})",
                instruction_budget.get(),
                MAX_INSTRUCTION_BUDGET,
            ));
        }
        Ok(Self {
            max_orders_per_chunk,
            instruction_budget,
            max_fills_per_settling_event,
        })
    }

    pub fn max_orders_per_chunk(&self) -> u32 {
        self.max_orders_per_chunk.get()
    }

    pub fn instruction_budget(&self) -> u64 {
        self.instruction_budget.get()
    }

    pub fn max_fills_per_settling_event(&self) -> u32 {
        self.max_fills_per_settling_event.get()
    }
}

impl Default for ExecutionPolicy {
    /// Conservative production policy: see [`DEFAULT_MAX_ORDERS_PER_CHUNK`],
    /// [`DEFAULT_INSTRUCTION_BUDGET`], and
    /// [`DEFAULT_MAX_FILLS_PER_SETTLING_EVENT`].
    fn default() -> Self {
        Self::try_new(
            DEFAULT_MAX_ORDERS_PER_CHUNK,
            DEFAULT_INSTRUCTION_BUDGET,
            DEFAULT_MAX_FILLS_PER_SETTLING_EVENT,
        )
        .expect("BUG: DEFAULT_* constants must produce a valid ExecutionPolicy")
    }
}
