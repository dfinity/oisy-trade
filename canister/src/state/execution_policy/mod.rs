use minicbor::{Decode, Encode};

#[cfg(test)]
mod tests;

/// IC system-subnet per-message instruction cap. Anything beyond this is
/// unreachable in practice — refuse it at construction so an operator
/// notices the typo before deploying.
const MAX_INSTRUCTION_BUDGET: u64 = 40_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct ExecutionPolicy {
    #[n(0)]
    max_orders_per_chunk: u64,
    #[n(1)]
    instruction_budget: u64,
}

impl ExecutionPolicy {
    /// Build a validated `ExecutionPolicy`. Panics if `max_orders_per_chunk`
    /// or `instruction_budget` is zero, or if `instruction_budget` exceeds
    /// the IC system-subnet per-message cap.
    pub fn new(max_orders_per_chunk: u64, instruction_budget: u64) -> Self {
        assert!(
            max_orders_per_chunk > 0,
            "max_orders_per_chunk must be non-zero",
        );
        assert!(
            instruction_budget > 0,
            "instruction_budget must be non-zero",
        );
        assert!(
            instruction_budget <= MAX_INSTRUCTION_BUDGET,
            "instruction_budget {instruction_budget} exceeds IC per-message cap ({MAX_INSTRUCTION_BUDGET})",
        );
        Self {
            max_orders_per_chunk,
            instruction_budget,
        }
    }

    pub fn max_orders_per_chunk(&self) -> u64 {
        self.max_orders_per_chunk
    }

    pub fn instruction_budget(&self) -> u64 {
        self.instruction_budget
    }
}

impl Default for ExecutionPolicy {
    /// Conservative production policy: 1 000 orders per chunk, 1B
    /// instructions (~5% of the IC's 20B app-subnet cap).
    fn default() -> Self {
        Self::new(1_000, 1_000_000_000)
    }
}
