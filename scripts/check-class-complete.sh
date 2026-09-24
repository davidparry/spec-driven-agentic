#!/usr/bin/env bash
# Verifies this checkout reflects the completed 60-minute class:
#   - Exercise 1 drafted REQ-007 into the spec (validate_spec + refine_requirement loops)
#   - Exercise 2 took REQ-003 from pending to implemented (Red/Green/Refactor)
#
# By design this FAILS on trunk (the workshop starting point) and PASSES on
# the complete branch. REQ-004..006 and REQ-007 implementation are homework
# and intentionally not gated here.
set -euo pipefail

SPEC="$(cd "$(dirname "$0")/.." && pwd)/requirements/requirements.json"
fail=0

req003_status="$(jq -r '.requirements[] | select(.id == "REQ-003") | .status' "$SPEC")"
if [ "$req003_status" = "implemented" ]; then
    echo "OK: REQ-003 is implemented (Exercise 2: spec -> Gherkin -> Red/Green/Refactor)."
else
    echo "INCOMPLETE: REQ-003 status is '${req003_status:-missing}' - Exercise 2 has not been done on this branch."
    fail=1
fi

if jq -e '.requirements[] | select(.id == "REQ-007")' "$SPEC" > /dev/null; then
    echo "OK: REQ-007 exists in the spec (Exercise 1: draft and refine with the agent)."
else
    echo "INCOMPLETE: REQ-007 not found - Exercise 1 has not been done on this branch."
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo "This branch is the workshop starting point, not the end-of-class state."
fi
exit "$fail"
