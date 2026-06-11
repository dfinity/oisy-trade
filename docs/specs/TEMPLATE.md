<!-- Filename: docs/specs/DEFI-XXXX-short-slug.md -->
---
id:
title:
tags: []
---

# <title>

## Motivation

*Why are we doing this? What problem or risk does it address? Include the operational
context — e.g. who uses each mechanism and when it applies.*

## Requirements

*The verifiable behavioral contract, enumerated: `R1: if X, then Y.` This is the
canonical list — the Test plan and the per-PR acceptance criteria reference these `R#`
rather than restating behavior.*

## Non-goals

*What is explicitly out of scope, and any accepted residual limitations (so reviewers
know they were considered, not overlooked).*

## Design Decisions

*The few foundational decisions and their rationale ("chose X, not Y"). Keep at altitude:
the detailed "how" lives in Implementation; the "why not Z" lives in Discussed
Alternatives. Cross-reference instead of repeating.*

## Implementation

*The "how". One subsection per area. Collapses to a line — or "N/A" — for small specs;
expands for large ones. Prefer file paths and symbol names over brittle `file.rs:NN`
line anchors.*

### Constraints

*Pre-existing architectural constraints that shape the design.*

### <module / area>

*Concrete types, signatures, events, enforcement points.*

### Test plan

*Integration + unit tests; tag each to an `R#`. Note the verification commands.*

### Delivery / PR sequence

*Stacked PRs, each independently mergeable / compilable / testable, with per-PR
acceptance criteria expressed as `R#` coverage. This is the input the spec-driven build
loop consumes — keep it explicit.*

## Discussed Alternatives

*Approaches considered and why each was discarded, so reviewers don't relitigate.*
