#!/usr/bin/env bash
# Workshop run verifier — two subcommands:
#
#   start  Cut a NEW branch from trunk for this run (workshop-run-<timestamp>).
#          Every run gets its own branch; never rehearse or present on trunk.
#
#   check  After the exercises, compare this working tree against the
#          completed workshop's `complete` branch in the reference repo and
#          report PASS/FAIL per artifact. The run is only a pass when every
#          check is green.
#
# Reference repo: $COMPLETE_REPO (default: this repo — the complete branch is
# resolved locally or from origin/complete). Base branch for `start`:
# $BASE_BRANCH (default trunk).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPLETE_REPO="${COMPLETE_REPO:-$ROOT}"
BASE_BRANCH="${BASE_BRANCH:-trunk}"

usage() {
    echo "usage: $0 start|check"
    echo "  start  cut a new workshop-run-<timestamp> branch from ${BASE_BRANCH}"
    echo "  check  diff the end state against ${COMPLETE_REPO} (complete branch)"
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
    echo "Next: bdd --version && scripts/preflight.sh, then run the exercises."
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
    local complete_ref=""
    for ref in complete origin/complete; do
        if git -C "$COMPLETE_REPO" rev-parse --verify "$ref" >/dev/null 2>&1; then
            complete_ref="$ref"
            break
        fi
    done
    if [ -z "$complete_ref" ]; then
        echo "FAIL: no 'complete' or 'origin/complete' branch in reference repo $COMPLETE_REPO (set COMPLETE_REPO to a clone that has one)."
        exit 1
    fi
    ROOT="$ROOT" COMPLETE_REPO="$COMPLETE_REPO" COMPLETE_REF="$complete_ref" python3 - <<'PY'
import json, os, re, subprocess, sys

root = os.environ["ROOT"]
ref_repo = os.environ["COMPLETE_REPO"]
ref_branch = os.environ["COMPLETE_REF"]
SPEC = "requirements/requirements.json"
FEATURE = "kata/src/test/resources/features/string_calculator.feature"
TEST = "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java"

def local(path):
    with open(os.path.join(root, path)) as f:
        return f.read()

def reference(path):
    return subprocess.run(["git", "-C", ref_repo, "show", f"{ref_branch}:{path}"],
                          capture_output=True, text=True, check=True).stdout

def bdd(*args):
    """Ask the bdd CLI, so Exercise 1 is graded by the same deterministic
    validator and refiner the workshop tools use. None when bdd is missing
    or did not answer with JSON."""
    try:
        out = subprocess.run(["bdd", "spec", *args, "--root", root],
                             capture_output=True, text=True, check=True).stdout
        return json.loads(out)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError):
        return None

fail = 0
def report(ok, label, detail=""):
    global fail
    print(f"  {'PASS' if ok else 'FAIL'}  {label}" + (f" - {detail}" if detail and not ok else ""))
    if not ok:
        fail = 1

def req(doc, rid):
    return next((r for r in doc["requirements"] if r["id"] == rid), None)

spec = json.loads(local(SPEC))

# ---- Exercise 1: the requirement you drafted -----------------------------
# The wording here is authored by you and your agent, so it is graded the
# way the workshop grades it - the spec validates, the refiner has nothing
# left to say, and the behavior Exercise 1 asked for is covered - never
# against one canned paragraph.
r7 = req(spec, "REQ-007")
report(r7 is not None, "REQ-007 was drafted into the spec",
       "missing - Exercise 1 drafts it")

validation = bdd("validate")
report(validation is not None and validation.get("valid") is True,
       "the spec is valid",
       "; ".join(validation["issues"]) if validation else "bdd is not on PATH - see scripts/preflight.sh")

if r7 is not None:
    refinement = bdd("refine", "REQ-007")
    report(refinement is not None and refinement.get("clean") is True,
           "REQ-007 wording is refine-clean",
           "; ".join(refinement["findings"]) if refinement else "bdd is not on PATH - see scripts/preflight.sh")
    report(any("//" in c for c in r7.get("acceptanceCriteria", [])),
           "REQ-007 covers the first-line delimiter declaration",
           'no criterion mentions the "//" declaration Exercise 1 asks for')

# ---- Exercise 2: REQ-003 taken to green ---------------------------------
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

mine = scenario_blocks(local(FEATURE), "@REQ-003")
theirs = scenario_blocks(reference(FEATURE), "@REQ-003")
report(sorted(mine) == sorted(theirs) and len(mine) == 2,
       f"@REQ-003 scenarios match the complete branch ({len(mine)} found, 2 expected)",
       "scenario text differs or count is wrong")

def extract_method(text, name):
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if name + "(" in line:
            start = i
            while start > 0 and lines[start - 1].strip().startswith("@"):
                start -= 1
            depth, j = 0, i
            while j < len(lines):
                depth += lines[j].count("{") - lines[j].count("}")
                if depth == 0 and "{" in "".join(lines[start:j + 1]):
                    return "\n".join(l.rstrip() for l in lines[start:j + 1])
                j += 1
    return None

m_mine = extract_method(local(TEST), "twoCommaSeparatedNumbersAreSummed")
m_ref = extract_method(reference(TEST), "twoCommaSeparatedNumbersAreSummed")
report(m_mine is not None and m_mine == m_ref,
       "REQ-003 unit test matches the complete branch",
       "method missing or text differs")

print()
if fail:
    print("Run does NOT match the complete branch - see FAIL lines above.")
else:
    print("Run matches the complete branch. Clean up with:")
    print("  git checkout trunk && git branch -D <this-run-branch>")
sys.exit(fail)
PY
}

case "$1" in
    start) cmd_start ;;
    check) cmd_check ;;
    *) usage ;;
esac
