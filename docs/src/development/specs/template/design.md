---
id:
title:
tags: []
---

# `<title>` — Design

*Companion to [`requirements.md`](requirements.md), and the document an implementer is
handed. Every decision here serves numbered criteria, and every requirement is served by
something here. **Cite criteria (`Req 5`, `Req 5.1`), never restate them** — a behaviour
described in both places will drift.*

## Overview

*The shape of the solution in a few paragraphs: the approach chosen and how the pieces fit.
Enough for a reader to predict the diff before reading further. Collapses to a line for
small specs.*

## Constraints

*Pre-existing architectural facts that shape the design and that the implementer must not
violate — invariants of the surrounding system, not choices made here. Prefer file paths
and symbol names over brittle `file.rs:NN` line anchors.*

## Design Decisions

*The few foundational decisions and their rationale ("chose X, not Y"). Keep at altitude:
the detailed "how" lives in Implementation; the "why not Z" lives in Discussed
Alternatives. Cross-reference instead of repeating.*

### D1 — `<the decision, stated as a choice>`

*What was chosen, which criteria it serves, and why. If a criterion left a constant open,
this is where its value is settled and the evidence recorded.*

## Implementation

*The "how". One subsection per area. Collapses to a line — or "N/A" — for small specs;
expands for large ones. Concrete types, signatures, events, and enforcement points, so the
implementer is not re-deriving them.*

### `<module / area>`

## Test plan

*How each requirement becomes evidence, split by level (unit / integration / benchmark),
including the cases that would pass a plausible-but-wrong implementation and therefore
have to be written deliberately. Name the criteria each case pins. End with the
verification commands to run.*

*This is where the **methods and instruments** live. A requirement states a property ("cost
grows at worst as `O(N log N)`"); the harness that measures it, the fixture shapes, the
sizes swept, and the file the numbers are committed to are all design. Where a criterion
obliges the system to record evidence, say here exactly what gets recorded and where. Where
a criterion is observable only through a seam that does not exist yet — a counter, an
injectable clock, a scheduler abstraction — this section owes the design of that seam, and
Implementation owes the change that introduces it.*

*The method living here does not move the **files** out of Implementation. If a PR edits a
benchmark module, a fixture module, or a test harness, that module still gets its
Implementation subsection; the Test plan says what is measured and why, Implementation says
what changes and where. A file no section mentions is a file the implementer will not know
to touch.*

## Delivery / PR sequence

*Stacked PRs, each independently mergeable, compilable, and testable, with per-PR
acceptance expressed as criteria coverage — e.g. `*Acceptance:* Req 1, Req 5 (5.1, 5.2,
5.7), Req 8`. This is the input the spec-driven build loop consumes — keep it explicit,
and make sure the union of the PRs covers every requirement.*

## Discussed Alternatives

*Approaches considered and why each was discarded, so reviewers don't relitigate.*
