---
id:
title:
tags: []
---

# `<title>` — Requirements

*Companion to [`design.md`](design.md). This document is the **behavioural contract**:
what the system must do, stated so that each statement can be tested against the public
interface. It is written before the design and must stay readable without it — if a
sentence only makes sense once you know how the code is structured, it belongs in
`design.md`.*

## Introduction

*Why are we doing this, in prose: the problem or risk, who hits it and when, and what the
feature adds at the level of observable behaviour. Name the endpoints or user-facing
surfaces involved; do not name the types, modules, or algorithms behind them. Two to five
paragraphs.*

*If the work is split into child specs, say so here and link them.*

## Glossary

*Every term the acceptance criteria lean on, defined once. A criterion must never be the
first place a term is explained. Cover the domain concepts, the named constants, and the
existing public types the criteria refer to by name. Nothing here is a design choice — this
is vocabulary, so that the criteria below can be short.*

*Give a multi-word concept a single `Capitalized_Snake` name and use that name in the
criteria, so a criterion reads as one clause instead of re-deriving the concept
("resolve the Caller to its Effective_Account", not "resolve the caller to the funding
account it is registered against, or to itself if it is a funding account"). Constants are
`SCREAMING_SNAKE`. Existing published types keep their real names, backticked.*

*Coin a term only if the criteria use it **three or more times**. A one-or-two-use coinage
costs the reader a lookup to save the author a clause, and every coined term is a name a
reviewer must hold to read the document.*

- **Term**: *definition, in one or two sentences.*
- **Capitalized_Snake_Term**: *definition of a multi-word concept the criteria lean on.*
- **CONSTANT_NAME**: *what it bounds. If its value is not settled here, say which
  requirement fixes the behaviour and where the number is settled.*

## Non-goals

*What is explicitly out of scope, and any accepted residual limitation, so a reviewer
knows it was considered rather than overlooked. Behaviour only — "modifying an order in
place is out of scope", not "we will not add a `ModifyOrder` type". Give the reason each
exclusion is safe or deferred.*

*An accepted limitation that is nonetheless **assertable** belongs in both places: state it
as a criterion so a test can pin it, and explain here why it is accepted. A limitation that
lives only in Non-goals is one no test is obliged to hold in place, and it will quietly
stop being true.*

## Requirements

*The canonical, verifiable contract. The implementer derives tests from these and the
reviewer checks the PR against them, so completeness matters: **anything not stated here is
not required**, and anything stated here must be checkable.*

*Granularity: one `### Requirement N` per coherent **behaviour**, with the individual
obligations enumerated beneath it as criteria — not one requirement per obligation. Expect a
handful to a dozen or so requirements, each carrying two to eight criteria.*

*Two independent authors will group the same material differently unless the seam is
forced, so apply this test in order:*

1. *If two candidate requirements would share a user story, they are **one** requirement.*
2. *If one candidate is only meaningful given the other — it refines, bounds, or excepts
   it — it is a **criterion** of that one, not a sibling.*
3. *If a candidate has its own stakeholder who would notice it missing on its own, it is
   **standalone**, even when it is small.*

*Rule 3 wins ties: prefer the standalone requirement. A criterion buried inside a larger
requirement is easy to lose track of in a PR-acceptance list, and splitting later means
renumbering, which the append-only rule forbids.*

*Title each requirement with a **declarative statement of the property**, in Title Case —
"Batch Size Is Capped", "An Empty Batch Is a Successful No-Op" — not an imperative and not
a topic label ("Batching", "Caps").*

*The user story names a **real stakeholder** (operator, client developer, market maker,
canister operator), not "user". Vary it: which stakeholder is harmed if the requirement is
missed is often the clearest statement of why it exists.*

*Rules for acceptance criteria:*

- *Use `SHALL` / `SHALL NOT`, one obligation per numbered item, each independently
  checkable.*
- *Pick the form that matches the obligation: `WHEN <trigger>, THE <system> SHALL <response>`
  for events; `IF <precondition>, THEN THE <system> SHALL <response>` for conditional
  behaviour; `WHILE <state>, THE <system> SHALL <response>` for behaviour that holds
  throughout a state; `THE <system> SHALL <property>` for an unconditional invariant.*
- *State the **observable** response — a returned value, an error leaf, a balance, an
  emitted event, a persisted state visible through a query. The **published interface** is
  fair game and often *is* the contract, so naming an endpoint, a Candid type, or an error
  leaf is correct. What must not appear is anything internal: a module, an internal
  function or type, a data structure, an algorithm, or a phase/pass of a procedure.*
- *Two other obligation kinds are legitimate and belong here rather than in `design.md`,
  because they bind the whole feature rather than one implementation of it: **contract
  evolution** ("SHALL NOT lower `X` once published, because that would reject requests
  that previously succeeded") and **evidence** ("SHALL record the state the measurement was
  taken at alongside the chosen value"). State the property and the obligation; the
  instrument that produces the evidence — the benchmark harness, the test framework — is
  `design.md`'s Test plan.*
- *Fix the behaviour, not the number. When a constant's value is still open, the criterion
  says what the constant governs and which measurement settles it; `design.md` settles it.*
- *Keep **THE `<system>`** as the subject of every criterion, including the
  contract-evolution and evidence kinds — "THE Canister SHALL NOT lower `X` once
  published", "THE Canister SHALL record the envelope alongside the value". Do not slip
  into "THE spec SHALL …" or "THE measurement SHALL …": a criterion is an obligation on
  the thing being built, and a shifting subject is the first sign an obligation has drifted
  into design prose.*
- *Spell out **boundaries and negatives** rather than leaving them implied. A criterion
  capping something needs its companion stating what happens exactly at the cap and what
  the floor is; a criterion describing a success path needs its companion stating what is
  NOT done. These are the criteria an author under time pressure drops, and they are the
  ones an implementation gets wrong.*
- *A criterion may be observable only through a seam the design has to introduce — a
  counter, an injectable clock, a scheduler abstraction. That is fine and it is still a
  requirement; note that it needs one, and `design.md`'s Test plan owes you the seam. What
  is not fine is weakening the criterion to whatever is observable today.*
- *A criterion is **one sentence**. A short trailing `because …` clause is allowed, and
  worth it, when the obligation looks arbitrary without it — that is the difference between
  a rule a reviewer enforces and one they argue with. Anything longer is rationale and
  belongs in the user story, the Introduction, or `design.md`'s Design Decisions.*
- *Cross-reference rather than restate: a criterion that depends on another says "per 3.2"
  instead of repeating it. The same duplication that makes `design.md` drift makes
  `requirements.md` drift against itself.*
- *Numbering is append-only once the spec is on a PR: add `N.7`, never renumber `N.3`.
  The Delivery / PR sequence, commits, and review comments cite criteria as `Req N` for a
  whole requirement or `Req N.M` for one criterion.*

### Requirement 1: `<Declarative Statement Of The Property, In Title Case>`

**User Story:** As a `<stakeholder>`, I want `<capability>`, so that `<benefit>`.

#### Acceptance Criteria

1. WHEN `<trigger>`, THE `<system>` SHALL `<observable response>`.
2. IF `<precondition>`, THEN THE `<system>` SHALL `<observable response>`.
3. THE `<system>` SHALL `<unconditional invariant>`.
4. THE `<system>` SHALL NOT `<prohibited observable behaviour>`, because `<reason the
   prohibition is not arbitrary>`.

### Requirement 2: `<Declarative Statement Of The Property, In Title Case>`

**User Story:** As a `<role>`, I want `<capability>`, so that `<benefit>`.

#### Acceptance Criteria

1. …
