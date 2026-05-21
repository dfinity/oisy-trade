use std::num::NonZeroU64;

#[cfg(test)]
mod tests;

/// Highest instruction budget the validator accepts. Rejects obvious
/// typos (e.g. `u64::MAX`) without constraining the production policy —
/// see [`Default`] for the value the canister actually ships with.
///
/// Spec: <https://docs.internetcomputer.org/references/resource-limits/#instruction-limits>
const MAX_INSTRUCTION_BUDGET: u64 = 40_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionPolicy {
    max_orders_per_chunk: NonZeroU64,
    instruction_budget: NonZeroU64,
}

impl ExecutionPolicy {
    /// Build a validated `ExecutionPolicy`. Panics if `max_orders_per_chunk`
    /// or `instruction_budget` is zero, or if `instruction_budget` exceeds
    /// the IC system-subnet per-message cap.
    pub fn new(max_orders_per_chunk: u64, instruction_budget: u64) -> Self {
        let max_orders_per_chunk =
            NonZeroU64::new(max_orders_per_chunk).expect("max_orders_per_chunk must be non-zero");
        let instruction_budget =
            NonZeroU64::new(instruction_budget).expect("instruction_budget must be non-zero");
        assert!(
            instruction_budget.get() <= MAX_INSTRUCTION_BUDGET,
            "instruction_budget {} exceeds IC per-message cap ({})",
            instruction_budget.get(),
            MAX_INSTRUCTION_BUDGET,
        );
        Self {
            max_orders_per_chunk,
            instruction_budget,
        }
    }

    pub fn max_orders_per_chunk(&self) -> u64 {
        self.max_orders_per_chunk.get()
    }

    pub fn instruction_budget(&self) -> u64 {
        self.instruction_budget.get()
    }
}

impl Default for ExecutionPolicy {
    /// Conservative production policy: 1 000 orders per chunk, 1B
    /// instructions per chunk.
    fn default() -> Self {
        Self::new(1_000, 1_000_000_000)
    }
}
