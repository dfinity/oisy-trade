#!/usr/bin/env bash
set -Eexuo pipefail

# Collects benchmark results from artifact files and outputs them as a JSON array
# to be used in a GitHub Actions matrix.

matrix_json=$(
  python3 - <<'PY'
import glob
import json
import os

benchmarks = []

for directory in sorted(glob.glob("canbench_result_*")):
    if os.path.isdir(directory):
        result_path = os.path.join(directory, f"{directory}.md")
        if os.path.exists(result_path):
            with open(result_path, encoding="utf-8") as fh:
                benchmarks.append({
                    "title": directory,
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
