# Review Rules

## Challenge

* First read the PR title and PR description without reading the PR diff.
* Brainstorm possible solutions, focus on minimal code changes, long-term maintainability and security.
* Compare you solutions against the PR diff:
    * What are the pros/cons? 
    * Report the alternative if it offers a better or similar trade-off as the PR implementation. Do not report the alternative if it's clearly worse.

## Testing

* Any change of behaviour is accompanied by at least a new test or a modification of existing tests.
* Follow the test pyramid:
    * Maximize test coverage at the unit test level.
        * Favor parameterized tests instead of copy/pasting the test setup.
        * Favor proptests to test arbitrary inputs.
        * Common helper test methods, proptests strategies are in `test_fixtures`
    * Any change of API is accompanied by at least a new or a modified integration test.
    * Challenge an integration test if it's already covered by unit tests.
* Assertions must be able to fail on a regression:
    * Tautological assertions — when an expected literal pulls a field from the actual value (`Foo { ..., bar: actual.bar }`) — are trivially self-equal and reduce that field to a no-op in the equality check. Flag them.
    * Constant-mock blind spots — when a mock returns a single constant (e.g. `expect_time().return_const(EPOCH)`), assertions on that constant don't prove the production code read it at the right moment. If a new field carries temporal meaning (placement vs. cancel time, request vs. response, before vs. after), require the mock to return distinct values so the assertion pins *which* call's value was kept.
* When a new field is added to a type that has an `arb_*` strategy, the strategy must fuzz the new field, not hard-code a sentinel. A hard-coded constant in `arb_*` is a coverage hole the type's own roundtrip proptest can't see.

## Maintainability

* Flag any code duplication.
* Point to easy refactorings that could reduce code duplication.
* For new types, every derived trait (`Hash`, `Ord`, `PartialOrd`, `Default`, …) must be used somewhere. Unused derives are dead capability and can mislead future use — e.g. `Hash` on a clock reading suggests hashmap-keying, which is rarely the right semantic.
* Flag primitive parameters (`u64`, `usize`, `bool`, …) where the surrounding module already uses **domain primitives** (newtype wrappers like `OrderId`, `OrderSeq`, `Timestamp`) for the same *kind* of quantity. A bare `u64` sliding next to typed wrappers is primitive obsession / an ambiguous parameter list — silent on swap at compile time. Promote it to a newtype that names the concept and enforces its invariants. (Cf. *Secure by Design* Ch 5.1 / 12.2.)
* When the same invariant condition is checked at multiple sites (e.g. "fee-pool entry has registered token metadata", "user is registered", "stable-memory region is initialized"), every site must handle violations the same way. Divergent handling — `panic` here, `unwrap_or_default()` there, `Result::Err` somewhere else — means at least one site is wrong. Either the condition truly can't happen (drop the fallbacks as dead) or it can (drop the panics and degrade consistently). Reviewers should ask: *"is this same situation handled the same way everywhere it appears?"*
* Flag silent fallbacks on a failure path — `f64::NAN` returns, `unwrap_or_default()`, `Result::ok()` discards, `eprintln!` without surfacing, `let _ = result;` — that turn an invariant violation into a value indistinguishable from success. Operators and monitoring can't see the failure. Either propagate the error, log at a level that pages, or expose a `*_errors_total` counter (Prometheus treats `NaN` as no-sample, so emitting it from a metric encoder is invisible). Defaults are fine for *expected* missing inputs; never for an invariant breach.

## Docs

* Public facing API is well-documented (what it is about, corner-cases, requirements, examples, etc.)
* No JIRA ticket or internal information in docs. JIRA ticket are acceptable in comments pointing for a TODO.

## What to Ignore

* Anything checked by CI: lint, compilation

## Reporting

* **Blocker** 🔴: MUST be changed. Otherwise PR won't be approved.
* **Medium** 🟠: SHOULD be changed for PR to be approved.
* **Nit** 🔵: PR can be merged with or without considering the comment.
