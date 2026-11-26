#!/usr/bin/env bash
set -Eexuo pipefail

# Collects the benchmark result from the artifact files and outputs it as a
# JSON array to be used in a GitHub Actions matrix, together with the number of
# the PR to post it to.
#
# The artifacts are produced by the `pull_request`-triggered CI and are fully
# controlled by the (possibly fork) PR author. They are extracted into
# `$ARTIFACTS_DIR`, a scratch directory outside the trusted checkout, and are
# read here as data only — never as code loaded by this privileged workflow.
#
# The result is posted verbatim by `github-actions[bot]`, so it is restricted
# before it is posted: only the one expected result is accepted (so a PR cannot
# fan out into many comments with titles of its choosing), its length is
# capped, and `@` mentions are neutralised (so it cannot notify users or
# teams).

matrix_json=$(
  ARTIFACTS_DIR="${ARTIFACTS_DIR:?}" GITHUB_OUTPUT="${GITHUB_OUTPUT:?}" python3 - <<'PY'
import json
import os

# The only result the `benchmark` job in ci.yml uploads.
TITLE = "canbench_result_benchmark"
# canbench results are a few KiB; GitHub rejects comments over 65536 characters.
MAX_RESULT_CHARS = 32_000

artifacts_dir = os.environ["ARTIFACTS_DIR"]
root = os.path.realpath(artifacts_dir)


def is_safe(path):
    rel = os.path.relpath(path, artifacts_dir)
    current = artifacts_dir
    for part in rel.split(os.sep):
        current = os.path.join(current, part)
        if os.path.islink(current):
            return False
    return os.path.realpath(path).startswith(root + os.sep)


def sanitize(text):
    if len(text) > MAX_RESULT_CHARS:
        text = text[:MAX_RESULT_CHARS] + "\n\n… (truncated)\n"
    # A zero-width joiner after `@` keeps the text readable but stops GitHub
    # from turning it into a mention.
    return text.replace("@", "@‍")


benchmarks = []
for result_path in (
    os.path.join(artifacts_dir, f"{TITLE}.md"),
    os.path.join(artifacts_dir, TITLE, f"{TITLE}.md"),
):
    if os.path.isfile(result_path) and is_safe(result_path):
        with open(result_path, encoding="utf-8", errors="replace") as fh:
            benchmarks.append({"title": TITLE, "result": sanitize(fh.read())})
        break

with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
    out.write(f"has_results={'true' if benchmarks else 'false'}\n")

print(json.dumps({"benchmark": benchmarks}))
PY
)

# Output the benchmark matrix to be used by the next job. `json.dumps` above
# emits a single line (newlines are escaped), so this cannot inject extra
# step-output keys.
echo "matrix=$matrix_json" >> "$GITHUB_OUTPUT"

if [ "$matrix_json" = '{"benchmark": []}' ]; then
  echo "::warning::No canbench result found in the CI run's artifacts (did CI fail before uploading it?); not posting a comment."
fi

# Resolve the target PR number authoritatively from the GitHub API using the
# head SHA of the workflow run that produced these artifacts, instead of
# trusting the downloaded `pr_number` artifact.
#
# This workflow runs privileged (via `workflow_run`) and the artifacts are
# produced by the `pull_request`-triggered CI, whose definition and scripts are
# fully controlled by the (possibly fork) PR author. Their contents must never
# be `cat`'d raw into $GITHUB_OUTPUT (multi-line content could override other
# outputs such as `matrix` — step-output injection) nor be used to target an
# arbitrary PR (spoofed bot comments). Requiring the PR's head to be the run's
# head SHA binds the comment to the PR that actually produced the results.
#
# The PR is looked up by its head `<owner>:<branch>` rather than via
# `commits/<sha>/pulls`, which returns no PRs when the commit lives in a fork.
# The owner and branch come from the run's head repository and are chosen by
# the PR author, so they are passed as query parameters (URL-encoded by `gh`)
# and only used to narrow the lookup; the head SHA check decides the match.
#
# A failing API call (permissions, rate limit, ...) fails this job loudly;
# only a successful lookup that finds no matching open PR skips the comment.
: "${HEAD_SHA:?}"
pr_number="$(
  gh api --method GET "repos/${WORKFLOW_RUN_REPO:?}/pulls" \
    -f head="${HEAD_OWNER:?}:${HEAD_BRANCH:?}" -f state=open \
    --jq '[.[] | select(.head.sha == env.HEAD_SHA) | .number] | first // empty'
)"

# Defense in depth: only ever emit a bare positive integer (PR numbers start at 1).
if [[ "$pr_number" =~ ^[1-9][0-9]*$ ]]; then
  echo "pr_number=$pr_number" >> "$GITHUB_OUTPUT"
else
  echo "pr_number=" >> "$GITHUB_OUTPUT"
  # The owner and branch are PR-author-controlled, so they are kept out of
  # this workflow command.
  echo "::warning::No open PR whose head is ${HEAD_SHA} (closed, or pushed to since?); not posting a comment."
fi
