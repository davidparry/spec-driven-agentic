#!/usr/bin/env bash
# Workshop run verifier — two subcommands:
#
#   start  Cut a NEW branch from trunk for this run (workshop-run-<timestamp>).
#          Every run gets its own branch; never rehearse or present on trunk.
#
#   check  After the exercises, grade this working tree artifact by artifact
#          and report PASS/FAIL for each. Every check is graded on the run's
#          own terms — the deterministic validator and refiner for wording,
#          the requirement's own acceptance criteria for coverage — never
#          against one recorded solution. The run is only a pass when every
#          check is green.
#
# Base branch for `start`: $BASE_BRANCH (default trunk).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASE_BRANCH="${BASE_BRANCH:-trunk}"

usage() {
    echo "usage: $0 start|check"
    echo "  start  cut a new workshop-run-<timestamp> branch from ${BASE_BRANCH}"
    echo "  check  grade the end state against the spec's acceptance criteria"
    exit 2
}

[ $# -eq 1 ] || usage

cmd_start() {
    cd "$ROOT"
    if [ -n "$(git status --porcelain)" ]; then
        echo "FAIL: working tree is not clean - commit, stash, or discard first."
        git status --short
        exit 1
    fi
    local branch="workshop-run-$(date +%Y%m%d-%H%M%S)"
    git checkout -b "$branch" "$BASE_BRANCH"
    echo
    echo "On new branch '$branch' (cut from ${BASE_BRANCH})."
    echo "Next: spec --version && scripts/preflight.sh, then run the exercises."
    echo "When done: scripts/verify-workshop-run.sh check"
}

cmd_check() {
    cd "$ROOT"
    local branch
    branch="$(git branch --show-current)"
    if [ "$branch" = "trunk" ] || [ "$branch" = "complete" ]; then
        echo "FAIL: refusing to check on '$branch' - run the workshop on a branch cut from trunk (scripts/verify-workshop-run.sh start)."
        exit 1
    fi
    ROOT="$ROOT" python3 - <<'PY'
import json, os, re, subprocess, sys

root = os.environ["ROOT"]
SPEC = "requirements/requirements.json"
FEATURE = "kata/src/test/resources/features/string_calculator.feature"
TEST = "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java"

def local(path):
    with open(os.path.join(root, path)) as f:
        return f.read()

def harness(*args):
    """Ask the spec harness, so Exercise 1 is graded by the same deterministic
    validator and refiner the workshop tools use. None when spec is missing
    or did not answer with JSON. A non-zero exit is expected and fine -
    spec validate exits 1 on an invalid spec and still prints the issues."""
    try:
        out = subprocess.run(["spec", *args, "--root", root],
                             capture_output=True, text=True).stdout
        return json.loads(out)
    except (OSError, json.JSONDecodeError):
        return None

fail = 0
def report(ok, label, detail=""):
    global fail
    print(f"  {'PASS' if ok else 'FAIL'}  {label}" + (f" - {detail}" if detail and not ok else ""))
    if not ok:
        fail = 1

def req(requirements, rid):
    return next((r for r in requirements if r["id"] == rid), None)

def merged_spec(path=SPEC, seen=None):
    """requirements.json is the entry point, not necessarily the whole spec:
    it may carry an `includes` list of child files, N levels deep, and the
    tools grade the merged tree. Extra C moves REQ-007 into a child, so
    reading only the root file would report it missing."""
    seen = set() if seen is None else seen
    if path in seen:
        return []
    seen.add(path)
    doc = json.loads(local(path))
    requirements = list(doc.get("requirements", []))
    parent = os.path.dirname(path)
    for child in doc.get("includes", []):
        requirements += merged_spec(os.path.join(parent, child), seen)
    return requirements

spec = merged_spec()

# ---- Exercise 1: the requirement you drafted -----------------------------
# The wording here is authored by you and your agent, so it is graded the
# way the workshop grades it - the spec validates, the refiner has nothing
# left to say, and the behavior Exercise 1 asked for is covered - never
# against one canned paragraph.
r7 = req(spec, "REQ-007")
report(r7 is not None, "REQ-007 was drafted into the spec",
       "missing - Exercise 1 drafts it")

validation = harness("validate")
report(validation is not None and validation.get("valid") is True,
       "the spec is valid",
       "; ".join(validation["issues"]) if validation else "spec is not on PATH - see scripts/preflight.sh")

if r7 is not None:
    refinement = harness("refine", "REQ-007")
    report(refinement is not None and refinement.get("clean") is True,
           "REQ-007 wording is refine-clean",
           "; ".join(refinement["findings"]) if refinement else "spec is not on PATH - see scripts/preflight.sh")
    report(any("//" in c for c in r7.get("acceptanceCriteria", [])),
           "REQ-007 covers the first-line delimiter declaration",
           'no criterion mentions the "//" declaration Exercise 1 asks for')

# ---- Exercise 2: REQ-003 taken to green ---------------------------------
# Graded like Exercise 1: the scenarios and the unit test have to cover
# REQ-003's own acceptance criteria, in whatever words the run chose. A
# recorded solution cannot be the bar here either - `spec unittest generate`
# names one method per criterion, so no harness-driven run would ever
# reproduce a hand-written method name. Status `implemented` already carries
# the green bar, since mark-implemented refuses off GREEN.
r3 = req(spec, "REQ-003")
report(r3 is not None and r3.get("status") == "implemented",
       "REQ-003 status is 'implemented' in the spec",
       f"status is {r3.get('status') if r3 else 'missing'}"
       + (" - Exercise 2 takes REQ-003, not the REQ-007 you drafted" if r3 and r3.get("status") == "pending" else ""))

def scenario_blocks(text, tag):
    blocks, lines = [], text.splitlines()
    i = 0
    while i < len(lines):
        if tag in lines[i].split():
            j = i
            while j < len(lines) and lines[j].strip():
                j += 1
            blocks.append("\n".join(l.rstrip() for l in lines[i:j]))
            i = j
        else:
            i += 1
    return blocks

def covers(block, criterion):
    """A scenario or test covers a criterion when it feeds the same input
    literals in and lands on the same expected value. Everything else -
    scenario names, method names, assertion style - is the author's."""
    inputs = re.findall(r'"([^"]*)"', criterion)
    if not all(f'"{value}"' in block for value in inputs):
        return False
    outcome = re.findall(r"-?\d+", re.split(r"(?i)\bthen\b", criterion)[-1])
    return not outcome or re.search(rf"(?<!\d){outcome[-1]}(?!\d)", block) is not None

criteria = r3.get("acceptanceCriteria", []) if r3 else []
scenarios = scenario_blocks(local(FEATURE), "@REQ-003")
uncovered = [c for c in criteria if not any(covers(s, c) for s in scenarios)]
report(bool(criteria) and bool(scenarios) and not uncovered,
       f"@REQ-003 scenarios cover every acceptance criterion "
       f"({len(scenarios)} tagged, {len(criteria)} criteria)",
       "REQ-003 carries no acceptance criteria to cover" if not criteria
       else "no scenario is tagged @REQ-003" if not scenarios
       else "no scenario covers: " + "; ".join(uncovered))

def test_methods(text):
    """Every @Test in a Java test class - annotations, signature and body -
    cut at the method's own closing brace, so a trailing comment that names
    a requirement is not mistaken for a test of it."""
    methods = []
    for part in re.split(r"(?=^[ \t]*@Test\b)", text, flags=re.M)[1:]:
        lines, depth, opened = part.splitlines(), 0, False
        for i, line in enumerate(lines):
            depth += line.count("{") - line.count("}")
            opened = opened or "{" in line
            if opened and depth <= 0:
                part = "\n".join(lines[:i + 1])
                break
        methods.append(part)
    return methods

def body(method):
    """The method without its annotations: a @DisplayName that quotes the
    criterion must not pass for an assertion of it."""
    return "\n".join(l for l in method.splitlines() if not l.strip().startswith("@"))

unit = [m for m in test_methods(local(TEST)) if "REQ-003" in m]
unasserted = [c for c in criteria if not any(covers(body(m), c) for m in unit)]
placeholder = [m for m in unit if 'fail("TODO' in m]
report(bool(criteria) and bool(unit) and not unasserted and not placeholder,
       f"REQ-003 unit test asserts every acceptance criterion "
       f"({len(unit)} @Test naming REQ-003)",
       "REQ-003 carries no acceptance criteria to assert" if not criteria
       else "no @Test names REQ-003 - the file groups tests by requirement id" if not unit
       else 'a generated fail("TODO ...") placeholder is still there' if placeholder
       else "nothing asserts: " + "; ".join(unasserted))

print()
if fail:
    print("Run is NOT complete - see FAIL lines above.")
else:
    print("Run covers both exercises. Clean up with:")
    print("  git checkout trunk && git branch -D <this-run-branch>")
sys.exit(fail)
PY
}

case "$1" in
    start) cmd_start ;;
    check) cmd_check ;;
    *) usage ;;
esac
