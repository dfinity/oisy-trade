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

Specs live in `docs/specs/` as `DEFI-XXXX-short-slug.md` and follow the template at
`docs/specs/TEMPLATE.md`. When I ask you to write a spec, start from that template; the
build loop below consumes the spec's **Requirements** and its **Delivery / PR sequence**.

When I give you a specification to build:

1. Delegate to the `implementer` subagent with the full spec: build on a feature
   branch, open a DRAFT PR, report the PR number.
2. Delegate to the `reviewer` subagent. Pass it ONLY the spec and the PR number —
   do NOT paste the diff, so it can read the title/description before the code and
   review without anchoring. It posts its verdict to the PR via `gh`.
3. If the reviewer returns VERDICT: CHANGES_REQUESTED, OR the PR carries any unresolved
   comment — a review thread not marked resolved, or a comment (from the reviewer, me,
   Copilot, or any other reviewer/bot) not yet answered with a commit and/or a reply —
   send those comments back to `implementer`, which answers with NEW commits (no
   amend/rebase/squash/force-push) and replies on each thread. Repeat from 2.
   Check for unresolved comments via `gh`, not by eye: conversation comments via
   `gh api repos/{owner}/{repo}/issues/<n>/comments`, inline review comments via
   `repos/{owner}/{repo}/pulls/<n>/comments`, review summaries via
   `repos/{owner}/{repo}/pulls/<n>/reviews`, and review-thread resolution state via the
   GraphQL `reviewThreads` field (not exposed by `gh pr view --json`).
4. The automated loop is DONE only when the reviewer returns VERDICT: READY AND the PR
   has no unresolved comments. Then: do NOT mark the PR ready for review — leave it as a
   draft and post a comment saying the PR is ready for my review, then summarize the
   state and STOP.
   Do NOT approve and do NOT merge — marking ready, final approval, and merge are mine
   to do manually.

Never end the loop on your own judgment — it ends only when the reviewer's VERDICT is
READY and no unresolved comments remain. Final approval is always mine.
