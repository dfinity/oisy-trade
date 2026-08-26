# Specs

A spec is a **directory** named after its ticket, `DEFI-XXXX-short-slug/`, holding exactly
two documents:

- **`requirements.md`** — the verifiable behavioural contract: introduction, glossary,
  non-goals, and numbered requirements, each a user story plus EARS acceptance criteria
  (`WHEN … THE <system> SHALL …`). It carries **no implementation detail**, and it is what
  a reviewer checks a pull request against.
- **`design.md`** — the solution, written to be handed to an implementer: overview,
  constraints, design decisions, implementation per area, test plan, PR sequence, and
  rejected alternatives. It cites criteria as `Req N` / `Req N.M` rather than restating
  them.

Start from the [requirements template](template/requirements.md) and the
[design template](template/design.md). Write `requirements.md` first and settle it before
designing, so the contract is not back-fitted to an implementation.

Each spec drives a spec-driven build loop (implementer → reviewer → ready) — see the
project's `CLAUDE.md` for the loop itself.

Specs written before this split live as a single `DEFI-XXXX-short-slug.md` file and are
kept as-is.
