#!/usr/bin/env bash
set -Eexuo pipefail

# Collects benchmark results from artifact files and outputs them as a JSON array
# to be used in a GitHub Actions matrix.
#
# The artifacts are produced by the `pull_request`-triggered CI and are fully
# controlled by the (possibly fork) PR author. They are extracted into
# `$ARTIFACTS_DIR`, a scratch directory outside the trusted checkout, and are
# read here as data only — never as code loaded by this privileged workflow.

matrix_json=$(
  ARTIFACTS_DIR="${ARTIFACTS_DIR:?}" python3 - <<'PY'
import glob
import json
import os

artifacts_dir = os.environ["ARTIFACTS_DIR"]
root = os.path.realpath(artifacts_dir)

candidates = set(
    glob.glob(os.path.join(artifacts_dir, "canbench_result_*.md"))
) | set(
    glob.glob(os.path.join(artifacts_dir, "canbench_result_*", "canbench_result_*.md"))
)


def is_safe(path):
    rel = os.path.relpath(path, artifacts_dir)
    current = artifacts_dir
    for part in rel.split(os.sep):
        current = os.path.join(current, part)
        if os.path.islink(current):
            return False
    return os.path.realpath(path).startswith(root + os.sep)


benchmarks = []

for result_path in sorted(candidates):
    if not is_safe(result_path):
        continue
    name = os.path.basename(result_path)[: -len(".md")]
    with open(result_path, encoding="utf-8") as fh:
        benchmarks.append({
            "title": name,
            "result": fh.read(),
        })

print(json.dumps({"benchmark": benchmarks}))
PY
)

# Output the benchmark matrix to be used by the next job. `json.dumps` above
# emits a single line (newlines are escaped), so this cannot inject extra
# step-output keys.
echo "matrix=$matrix_json" >> "$GITHUB_OUTPUT"

# Resolve the target PR number authoritatively from the GitHub API using the
# head SHA of the workflow run that produced these artifacts, instead of
# trusting the downloaded `pr_number` artifact.
#
# This workflow runs privileged (via `workflow_run`) and the artifacts are
# produced by the `pull_request`-triggered CI, whose definition and scripts are
# fully controlled by the (possibly fork) PR author. Their contents must never
# be `cat`'d raw into $GITHUB_OUTPUT (multi-line content could override other
# outputs such as `matrix` — step-output injection) nor be used to target an
# arbitrary PR (spoofed bot comments). Deriving the number from the SHA binds
# the comment to the PR that actually produced the results.
pr_number=""
if [ -n "${HEAD_SHA:-}" ]; then
  pr_number="$(
    gh api "repos/${WORKFLOW_RUN_REPO:-}/commits/${HEAD_SHA}/pulls" \
      --jq '[.[] | select(.head.sha == env.HEAD_SHA) | .number] | first // empty' 2>/dev/null || true
  )"
fi

# Defense in depth: only ever emit a bare positive integer (PR numbers start at 1).
if [[ "$pr_number" =~ ^[1-9][0-9]*$ ]]; then
  echo "pr_number=$pr_number" >> "$GITHUB_OUTPUT"
else
  echo "pr_number=" >> "$GITHUB_OUTPUT"
fi
