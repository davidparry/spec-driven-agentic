#!/usr/bin/env bash
# Grade one requirement delivered into the harness's own catalog - the
# morning `student-follow-docs/a-day-in-the-life.md` walks, whether
# `spec deliver` ran it or a person did.
#
#   check <REQ-ID> [deliver-report.json]
#
# Every row is graded on the run's own terms, never against one recorded
# solution: the tagged scenario is whatever the run wrote, the unit test
# is read for whether it still carries the generated placeholder, the
# production files are whatever the requirement says they are. The
# optional deliver report is the JSON `spec deliver` printed (it has no
# `--json`: the report is always JSON, after the narration);
# with it the judgments the decision plane recorded are tabled per gate,
# and without it that row says so rather than passing.
#
# The sibling `verify-workshop-run.sh` grades the kata at the repository
# root; this one grades `harness/`, so it runs from there and refuses to
# grade `trunk`, which is the starting point and not a run.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HARNESS="$ROOT/harness"
BASE_BRANCH="${BASE_BRANCH:-trunk}"

usage() {
    echo "usage: $0 check <REQ-ID> [deliver-report.json]"
    exit 2
}

[ $# -ge 2 ] && [ "$1" = "check" ] || usage
REQ_ID="$2"
REPORT="${3:-}"
# Resolved before the cd below, so a path typed relative to wherever the
# grader was invoked is still the file it named.
if [ -n "$REPORT" ] && [ "${REPORT#/}" = "$REPORT" ]; then
    REPORT="$PWD/$REPORT"
fi

cd "$HARNESS"
branch="$(git branch --show-current)"
if [ "$branch" = "$BASE_BRANCH" ]; then
    echo "FAIL: refusing to grade on '$branch' - the morning runs on a branch cut from it."
    exit 1
fi

HARNESS="$HARNESS" REQ_ID="$REQ_ID" REPORT="$REPORT" BASE_BRANCH="$BASE_BRANCH" python3 - <<'PY'
import glob, json, os, re, subprocess, sys

harness = os.environ["HARNESS"]
rid = os.environ["REQ_ID"]
report_path = os.environ["REPORT"]
base = os.environ["BASE_BRANCH"]

fail = 0
def report(ok, label, detail=""):
    global fail
    print(f"  {'PASS' if ok else 'FAIL'}  {label}" + (f" - {detail}" if detail and not ok else ""))
    if not ok:
        fail = 1

def local(path):
    with open(os.path.join(harness, path)) as f:
        return f.read()

def run(*args, **kw):
    return subprocess.run(args, cwd=harness, capture_output=True, text=True, **kw)

def harness_json(*args):
    """Ask the spec on PATH, from harness/ so it finds this catalog. Non-zero
    exits are fine - several commands exit 1 and still print the reply."""
    try:
        return json.loads(run("spec", *args, "--json").stdout)
    except (OSError, json.JSONDecodeError):
        try:
            return json.loads(run("spec", *args).stdout)
        except (OSError, json.JSONDecodeError):
            return None

def merged_spec(path="requirements/requirements.json", seen=None):
    seen = set() if seen is None else seen
    if path in seen:
        return []
    seen.add(path)
    doc = json.loads(local(path))
    reqs = list(doc.get("requirements", []))
    parent = os.path.dirname(path)
    for child in doc.get("includes", []):
        reqs += merged_spec(os.path.join(parent, child), seen)
    return reqs

spec = merged_spec()
req = next((r for r in spec if r["id"] == rid), None)
report(req is not None, f"{rid} is in the catalog", "not drafted")
if req is None:
    sys.exit(1)

print(f"  {rid}: {req.get('title', '')}")

# ---- 1. a tagged scenario ---------------------------------------------
tag = f"@{rid}"
tagged = []
for path in glob.glob(os.path.join(harness, "tests/features/**/*.feature"), recursive=True):
    text = open(path).read()
    count = sum(1 for line in text.splitlines() if tag in line.split())
    if count:
        tagged.append((os.path.relpath(path, harness), count))
report(bool(tagged),
       f"a feature carries a scenario tagged {tag} "
       f"({', '.join(f'{p}: {n}' for p, n in tagged) or 'none'})",
       f"no scenario under tests/features/ is tagged {tag}")
feature_file = req.get("featureFile")
report(feature_file is None or any(p == feature_file for p, _ in tagged),
       f"the requirement's featureFile is where the tag lives ({feature_file or 'unset'})",
       f"featureFile names {feature_file} but the tag is in {[p for p, _ in tagged]}")

# ---- 2. no undefined steps ----------------------------------------------
missing = harness_json("steps", "missing")
report(missing is not None and missing.get("missing") == [],
       "spec steps missing reports 0",
       "spec is not on PATH" if missing is None else f"{len(missing.get('missing', []))} undefined: "
       + "; ".join(missing.get("missing", [])[:5]))

# ---- 3. the unit test ---------------------------------------------------
snake = re.sub(r"[^a-z0-9]+", "_", rid.lower()).strip("_")
test_path = f"tests/{snake}_test.rs"
exists = os.path.exists(os.path.join(harness, test_path))
report(exists, f"the unit test {test_path} exists", "not written")
if exists:
    text = local(test_path)
    placeholders = [l.strip() for l in text.splitlines() if "TODO: assert" in l or "todo!()" in l]
    report(not placeholders,
           f"{test_path} carries no generated placeholder "
           f"({len(re.findall(r'#\[test\]', text))} #[test])",
           f"{len(placeholders)} left: " + "; ".join(placeholders[:3]))
    build = run("cargo", "test", "--test", f"{snake}_test", "--no-run")
    report(build.returncode == 0, f"{test_path} compiles",
           (build.stderr.strip().splitlines() or ["no output"])[-1])

# ---- 4. production files ------------------------------------------------
production = req.get("productionFiles", [])
report(bool(production), f"{rid} records its productionFiles ({', '.join(production) or 'none'})",
       "nothing recorded - the attempt never wrote production code, or did not say where")
for path in production:
    present = os.path.exists(os.path.join(harness, path))
    report(present, f"{path} exists", "missing on disk")
    if present:
        # A file the base branch never had is new work by definition;
        # one it had must have moved, or nothing was implemented there.
        in_base = run("git", "cat-file", "-e", f"{base}:harness/{path}").returncode == 0
        moved = run("git", "diff", "--quiet", base, "--", path).returncode != 0
        report(not in_base or moved, f"{path} differs from {base}",
               "byte-identical to the base branch - nothing was implemented there")

# ---- 5. the bar and the status --------------------------------------------
state = harness_json("state")
report(state is not None and state.get("phase") == "GREEN",
       f"the recorded phase is GREEN (phase {state.get('phase') if state else 'unknown'}, "
       f"last run {state.get('lastRun', {}) if state else '?'})",
       "the bar is not green")
report(req.get("status") == "implemented",
       f"{rid} status is 'implemented' (is '{req.get('status')}')",
       "mark-implemented never ran, or was refused")

# ---- 6. the whole suite ---------------------------------------------------
suite = run("cargo", "test")
summary = [l for l in suite.stdout.splitlines() if l.startswith("test result") or "scenarios (" in l]
failed = [l for l in summary if "FAILED" in l or re.search(r"\d+ failed\)", l) and "0 failed" not in l]
report(suite.returncode == 0 and not failed, f"cargo test is green ({len(summary)} result lines)",
       "; ".join(failed[:3]) or (suite.stderr.strip().splitlines() or ["no output"])[-1])

# ---- 7. the judgments --------------------------------------------------
if report_path:
    try:
        deliver = json.load(open(report_path))
    except (OSError, json.JSONDecodeError) as e:
        deliver = None
        report(False, "the deliver report is readable", str(e))
    if deliver is not None:
        staged = deliver.get("judgments", [])
        counted = sum(len(s["judgments"]) for s in staged)
        # A configured decision model that was asked nothing is a fact
        # worth a line: a report with no judgments is byte-identical to
        # one from a project with no decision model at all.
        configured = run("spec", "config").stdout
        decision = [l for l in configured.splitlines() if l.startswith("decision.model\t")]
        named = bool(decision) and "(unset)" not in decision[0]
        report(counted > 0 or not named,
               f"the deliver report carries {counted} judgment(s) over {len(staged)} gated stage(s)",
               f"a decision model is configured ({decision[0].split(chr(9))[1] if decision else '?'}) "
               "and no gate recorded a judgment - every gated stage asked nothing, or none was reached")
        by_gate = {}
        for stage in staged:
            for j in stage["judgments"]:
                key = (stage["stage"], j["gate"], j["question"])
                row = by_gate.setdefault(key, {"HOLDS": 0, "FAILS": 0, "INCONCLUSIVE": 0, "actions": set(), "findings": []})
                row[j["verdict"]] = row.get(j["verdict"], 0) + 1
                row["actions"].add(j["action"])
                if j["verdict"] != "HOLDS":
                    row["findings"].append(f"{j['provenance']['input']}: {j['verdict']}")
        if by_gate:
            print("        stage            gate                        question                   holds fails unsure  actions")
            for (stage, gate, question), row in sorted(by_gate.items()):
                print(f"        {stage:<16} {gate:<27} {question:<26} {row['HOLDS']:>5} {row['FAILS']:>5} {row['INCONCLUSIVE']:>6}  {','.join(sorted(row['actions']))}")
            for (stage, gate, _), row in sorted(by_gate.items()):
                for finding in row["findings"]:
                    print(f"          {gate} at {stage}: {finding}")
        report(deliver.get("completed") is True and rid in deliver.get("delivered", []),
               f"the deliver report says {rid} was delivered",
               "; ".join(o.get("reason", "") for o in deliver.get("outstanding", [])) or "not in delivered")
else:
    print("  SKIP  no deliver report given - pass the JSON spec deliver printed as the third argument to table the judgments")

# ---- 8. the log ---------------------------------------------------------
logs = sorted(glob.glob(os.path.join(harness, ".spec/log/spec.log.*")))
if logs:
    text = "".join(open(p, errors="replace").read() for p in logs)
    approvals = [l for l in text.splitlines() if "command_run" in l]
    unjudged = [l for l in text.splitlines() if "could not be judged" in l]
    unasked = [l for l in text.splitlines() if "asked nothing" in l]
    print(f"  INFO  {len(logs)} log file(s): {len(approvals)} command_run line(s), "
          f"{len(unjudged)} 'could not be judged' warning(s), {len(unasked)} gate(s) that asked nothing")
    for line in unasked[:10]:
        print(f"          {line.strip()[:160]}")
    for line in approvals[:10]:
        print(f"          {line.strip()[:160]}")
    for line in unjudged[:5]:
        print(f"          {line.strip()[:160]}")
else:
    print("  INFO  no .spec/log/ files to scan")

print()
print("RESULT: " + ("PASS" if fail == 0 else "FAIL"))
sys.exit(fail)
PY
