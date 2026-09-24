#!/usr/bin/env bash
# Verifies this checkout is the 60-minute workshop STARTING point:
#   - the spec still has REQ-003..006 pending and no REQ-007 (Exercise 1 drafts it)
#   - no scenarios beyond REQ-001/002 exist yet (Exercise 2 writes @REQ-003 live)
#   - StringCalculator only handles a single number (the RED bar is still ahead)
#
# This is the inverse of check-class-complete.sh: it PASSES on trunk (the
# branch attendees clone) and FAILS on the complete branch. The build itself
# is green on both branches by design - "incomplete" lives in the spec, not
# in a failing build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPEC="$ROOT/requirements/requirements.json"
FEATURE="$ROOT/kata/src/test/resources/features/string_calculator.feature"
CALCULATOR="$ROOT/kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java"
fail=0

for req in REQ-003 REQ-004 REQ-005 REQ-006; do
    status="$(jq -r --arg id "$req" '.requirements[] | select(.id == $id) | .status' "$SPEC")"
    if [ "$status" = "pending" ]; then
        echo "OK: $req is pending (waiting to be driven during the talk)."
    else
        echo "NOT START STATE: $req status is '${status:-missing}' - expected 'pending'."
        fail=1
    fi
done

if jq -e '.requirements[] | select(.id == "REQ-007")' "$SPEC" > /dev/null; then
    echo "NOT START STATE: REQ-007 already exists - Exercise 1 drafts it live."
    fail=1
else
    echo "OK: REQ-007 absent (Exercise 1 will draft it with the agent)."
fi

if grep -qE '@REQ-00[3-9]' "$FEATURE"; then
    echo "NOT START STATE: feature file already has scenarios beyond REQ-001/002:"
    grep -nE '@REQ-00[3-9]' "$FEATURE" | sed 's/^/    /'
    fail=1
else
    echo "OK: feature file only covers REQ-001/002 (the worked example)."
fi

if grep -q 'split(' "$CALCULATOR"; then
    echo "NOT START STATE: StringCalculator already splits on delimiters - REQ-003+ appears implemented."
    fail=1
else
    echo "OK: StringCalculator is unimplemented past REQ-002 (RED bar still ahead)."
fi

echo
if [ "$fail" -ne 0 ]; then
    echo "This checkout is NOT the workshop starting point."
    echo "Reset with: git checkout -- kata requirements   (or check out trunk)"
else
    echo "trunk is ready for the 60-minute talk."
fi
exit "$fail"
