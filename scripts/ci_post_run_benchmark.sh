#!/usr/bin/env bash
set -Eexuo pipefail

if [ "$EXIT_STATUS" -eq 1 ]; then
  echo "canbench_results.yml is not up to date."
  echo "If the performance change is expected, run 'canbench --persist' locally and commit the updated results."
  exit 1
fi
