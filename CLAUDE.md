# CLAUDE.md

# Commits

* Run formatting (`cargo fmt --all`) and linting (`just lint`) before committing.

# Pull Requests

* When asked to create a PR, always create it in draft mode.
* When asked to set a PR description,
    * be concise
    * remove the "Test plan" section.
    * focus on the purpose of the PR and not on the implementation details (like name of structs). Do this by summarizing the prompts used in that session leading to the PR.

## Spec-driven build loop (GitHub PR)

Specs live in `docs/src/development/specs/` as a **directory per ticket**,
`DEFI-XXXX-short-slug/`, holding exactly two documents:

- `requirements.md` — the verifiable behavioural contract, and nothing else:
  **Introduction**, **Glossary**, **Non-goals**, then `### Requirement N` sections, each
  with a **User Story** and numbered EARS acceptance criteria (`WHEN <trigger>, THE
  <system> SHALL <observable response>`; `IF … THEN …` for conditional behaviour; a bare
  `THE … SHALL …` for an invariant). Every criterion must be independently testable
  through the public interface — which the **published** interface is part of, so naming an
  endpoint, a Candid type, or an error leaf is correct. It carries **no implementation
  detail**: no module paths, no internal type or function names, no algorithm, no data
  structure, no phase or pass of a procedure, and no test or benchmark harness. Where a
  criterion depends on a constant whose value is still open, it fixes the *behaviour* and
  leaves the *number* to `design.md`. Requirement titles are declarative statements of the
  property in Title Case; multi-word concepts get a `Capitalized_Snake` name in the
  Glossary and are used by that name in the criteria.
- `design.md` — how to build it, written to be handed to an implementer: **Overview**,
  **Constraints**, **Design Decisions** (`D1…`), **Implementation** per area, **Test
  plan**, **Delivery / PR sequence**, **Discussed Alternatives**.

Criteria are cited as `Req N` for a whole requirement and `Req N.M` for one criterion — by
`design.md`, by the PR sequence, by commits, and by review comments. `design.md` **cites,
never restates**: a behaviour written down in both documents will drift. Requirement
numbering is append-only once a spec is on a PR — add `N.7`, never renumber `N.3`.

When I ask you to write a spec, start from `docs/src/development/specs/template/`, and
write `requirements.md` **first** — settle it with me before you write `design.md`, so the
contract is not back-fitted to an implementation. Specs written before this split remain
single `DEFI-XXXX-short-slug.md` files and are consumed as-is.

The build loop below consumes `requirements.md`'s **Requirements** and `design.md`'s
**Delivery / PR sequence**.

When I give you a specification to build:

1. Delegate to the `implementer` subagent with the full spec — **both** documents: build
   on a feature branch, open a DRAFT PR, report the PR number.
2. Delegate to the `reviewer` subagent. Pass it ONLY the spec (both documents) and the PR
   number — do NOT paste the diff, so it can read the title/description before the code and
   review without anchoring. It posts its verdict to the PR via `gh`.
3. If the reviewer returns VERDICT: CHANGES_REQUESTED, OR the PR carries any unresolved
   comment — a review thread not marked resolved, or a comment (from the reviewer, me,
   Copilot, or any other reviewer/bot) not yet answered with a commit and/or a reply —
   send those comments back to `implementer`, which answers with NEW commits (no
   amend/rebase/squash/force-push) and replies on each thread. Repeat from 2.
   Check for unresolved comments via `gh`, not by eye — all via `gh api`: conversation
   comments at `repos/{owner}/{repo}/issues/<n>/comments`, inline review comments at
   `repos/{owner}/{repo}/pulls/<n>/comments`, review summaries at
   `repos/{owner}/{repo}/pulls/<n>/reviews`; review-thread resolution state via the
   GraphQL `reviewThreads` field (not exposed by `gh pr view --json`).
   After ANY push to the PR — an `implementer` fix, a `main`-merge, a rebase — re-run
   *all three* of these before going idle, never one without the others (a push that
   re-triggers CI also re-triggers bot review):
     - the CI check (`gh pr checks`);
     - this unresolved-comment check;
     - the PR's mergeability (`gh pr view <n> --json mergeable,mergeStateStatus`): the
       conflict signal is the `mergeable` field equal to `CONFLICTING` (equivalently, the
       `mergeStateStatus` field equal to `DIRTY`) — these are two fields with distinct
       value sets, so always compare each value against its own field. `mergeable:
       UNKNOWN` (GitHub recomputes mergeability asynchronously, common right after a push
       or a `main` advance) is NOT a verdict — treat it as a re-poll state: wait briefly
       and query again until it settles to `MERGEABLE` or `CONFLICTING`. Bound the retries
       (a handful of polls over ~a minute); if it never settles, fall back to attempting a
       trial `origin/main` merge to decide, and surface the stuck state rather than looping
       forever.
   Mergeability must ALSO be polled on every idle tick even with no push, because `main`
   advancing independently can turn the PR `mergeable: CONFLICTING` while CI stays green
   and no new comment appears — a CI run reflects the merge commit GitHub generated for the
   triggering push, so it will not catch a conflict introduced by a later `main` commit.
   When `mergeable` is
   `CONFLICTING`, merge `origin/main` into the PR (`git merge`, never rebase), resolve
   conflicts keeping the PR's own changes, re-run checks, and continue the loop.
4. The automated loop is DONE only when the reviewer returns VERDICT: READY AND the PR
   has no unresolved comments AND its `mergeable` has settled to `MERGEABLE` (not
   `CONFLICTING`, not `UNKNOWN`). Then: do NOT mark the PR ready for review — leave it as a
   draft and post a comment saying the PR is ready for my review, then summarize the
   state and STOP.
   This STOP is a pause, not the end: while the PR stays open it keeps accruing activity
   (my review, a bot re-review, `main` advancing), so any new commit, unresolved comment,
   or the PR turning unmergeable (`mergeable: CONFLICTING`) re-enters the loop at step 3 —
   do not treat an open PR as finished.
   Do NOT approve and do NOT merge — marking ready, final approval, and merge are mine
   to do manually.

For stacked PRs: whenever a PR is added to, removed from, or reordered within a stack,
refresh the "📚 PR stack" section of every *other* PR in the stack (and the
**Delivery / PR sequence** in the spec's `design.md`) so they all stay consistent — e.g. opening PR N means going
back to PRs 1..N-1 and adding PR N to their stack lists. This is the orchestrator's job:
the `implementer` only maintains the stack section of the PR it is building, since it is
scoped to a single PR and shouldn't reach into siblings.

When a stack's base PR merges, bring each dependent PR up to date: retarget its base to
the new parent (usually `main`), merge the parent in and resolve conflicts (keeping the
dependent's own changes), re-run its checks until green, and refresh its stack section
(mark the merged PR ✅). Don't leave a dependent PR pointing at a deleted branch or
showing a stale diff.

Never end the loop on your own judgment — it ends only when the reviewer's VERDICT is
READY and no unresolved comments remain. Final approval is always mine.
