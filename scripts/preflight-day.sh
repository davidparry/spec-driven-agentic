#!/usr/bin/env bash
# Presenter preflight for "A Day in the Life".
# Run from the repo root, T-30 minutes:
#   scripts/preflight-day.sh
# An absolute path works from any directory; the script cds to the root itself.
# Exits non-zero if any check fails.
#
# The sibling `preflight.sh` grades the 60-minute kata workshop: Java, Maven,
# the smoke-test jar, and REQ-003 left pending at the repository root. None of
# that is in this talk, which runs entirely against the `harness/` crate from
# two git worktrees. What this one checks is what that one cannot know about:
# the two worktrees exist, each is warm, demo 2 has a seeded cache, and demo 1
# has a clean slate.
#
# Worktree paths default to the siblings the presenter notes create. Override:
#   DEMO1=../elsewhere DEMO2=../other scripts/preflight-day.sh
#
# The two suite runs are the only slow checks here. Re-running after fixing
# one thing should not rebuild both worktrees, so:
#   SKIP_SUITE=1 scripts/preflight-day.sh
# Run it at least once without that, though — a red bar in either worktree is
# the failure most likely to end up on stage.

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DEMO1="${DEMO1:-$ROOT/../talk-demo1}"
DEMO2="${DEMO2:-$ROOT/../talk-demo2}"
BASE_BRANCH="${BASE_BRANCH:-trunk}"

PASS=0
FAIL=0

ok()  { printf '  \033[32mPASS\033[0m  %s\n' "$1"; PASS=$((PASS + 1)); }
bad() { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; printf '        fix: %s\n' "$2"; FAIL=$((FAIL + 1)); }
note() { printf '  \033[33mINFO\033[0m  %s\n' "$1"; }

echo "A Day in the Life preflight — $ROOT"
echo

# ---- 1. the binary -------------------------------------------------------
if command -v spec >/dev/null 2>&1; then
    ok "spec on PATH ($(command -v spec) — $(spec --version 2>/dev/null | head -1))"
else
    bad "spec not on PATH" "cargo install --path harness, or download a release binary"
    echo
    echo "Result: $PASS passed, $FAIL failed."
    echo "NOT ready. Nothing else can be checked without the binary."
    exit 1
fi

if command -v cargo >/dev/null 2>&1; then
    ok "cargo $(cargo --version 2>/dev/null | awk '{print $2}')"
else
    bad "cargo not on PATH" "install the Rust toolchain from https://rustup.rs"
fi

# ---- 2. the MCP surface demo 1 depends on --------------------------------
# Asked of the binary rather than assumed: a host that lists no tools is the
# one failure the room cannot help with, and this at least says whether the
# server or the editor is the problem.
TOOLS="$(cd harness && spec mcp tools 2>/dev/null | grep -cE '^[a-z0-9_]+$')"
if [ "${TOOLS:-0}" -eq 27 ]; then
    ok "spec mcp serve offers 27 tools"
else
    bad "spec mcp tools listed ${TOOLS:-0} tools, expected 27" \
        "the slides and both talk documents state 27 — reconcile before going on"
fi

# ---- 3. both models, named by the config rather than by this script ------
# Reading them from `spec config` means a model pinned in harness/.spec/
# config.toml is the one checked, instead of a name that rots in here.
model_for() {
    (cd harness && spec config 2>/dev/null) | awk -F'\t' -v k="$1" '$1 == k {print $2}'
}

WRITER="$(model_for llm.model)"
JUDGE="$(model_for decision.model)"
MODE="$(model_for decision.mode)"

if command -v ollama >/dev/null 2>&1; then
    INSTALLED="$(ollama list 2>/dev/null)"
    for pair in "writing:$WRITER" "decision:$JUDGE"; do
        role="${pair%%:*}"
        name="${pair#*:}"
        if [ -z "$name" ] || [ "$name" = "(unset)" ]; then
            bad "no $role model configured" "set it in harness/.spec/config.toml"
        elif printf '%s' "$INSTALLED" | awk '{print $1}' | grep -qx "$name"; then
            ok "$role model $name is pulled"
        else
            bad "$role model $name is not pulled" "ollama pull $name"
        fi
    done
    if curl -sf -m 5 "$(model_for llm.endpoint)/api/tags" >/dev/null 2>&1; then
        ok "ollama is answering on $(model_for llm.endpoint)"
    else
        bad "ollama is not answering on $(model_for llm.endpoint)" "start it: ollama serve"
    fi
else
    bad "ollama not on PATH" "install Ollama — both demos run on a local model"
fi

# Only CRITERION_MEASURABLE ships at an `enforce` ceiling, so it is the one
# gate the plane's mode can arm — and the one demo 1 reaches, through
# refine_requirement. Armed, a false alarm answers `clean: false` and an agent
# told to reword until clean cannot get past it. The plane's own value does
# not matter here; this gate's resolved mode does.
MEASURABLE="$(model_for decision.gates.CRITERION_MEASURABLE.mode)"
if [ "$MEASURABLE" = "advisory" ] || [ "$MEASURABLE" = "off" ]; then
    ok "CRITERION_MEASURABLE is $MEASURABLE — reported, not blocking (plane is $MODE)"
elif [ "$MODE" = "enforce" ]; then
    bad "CRITERION_MEASURABLE is enforcing (plane $MODE, no override)" \
        "add [decision.gates.CRITERION_MEASURABLE] with mode = \"advisory\" to harness/.spec/config.toml — on this spec the question misreads about one criterion in five, and demo 1 cannot reword its way out of a false alarm"
else
    ok "CRITERION_MEASURABLE cannot block (plane is $MODE)"
fi

# ---- 4. the two worktrees ------------------------------------------------
check_worktree() {
    label="$1"
    path="$2"
    echo
    echo "  --- $label — $path"

    if [ ! -d "$path/harness" ]; then
        bad "$label worktree missing" \
            "git worktree add $path -b ${label}-\$(date +%Y%m%d)"
        return
    fi
    ok "$label worktree present"

    branch="$(git -C "$path" branch --show-current 2>/dev/null)"
    if [ "$branch" = "$BASE_BRANCH" ]; then
        bad "$label is on $BASE_BRANCH" \
            "git -C $path switch -c ${label}-\$(date +%Y%m%d) — never demo on the base branch"
    else
        ok "$label is on '$branch'"
    fi

    # Tracked, so a worktree brings it. Checked anyway because it pins
    # RUST_TEST_THREADS, and without it the runner adapter tests
    # intermittently fail to spawn on macOS — which reaches the stage as a
    # red bar that has nothing to do with the demo.
    if [ -f "$path/harness/.cargo/config.toml" ]; then
        ok "$label has harness/.cargo/config.toml"
    else
        bad "$label is missing harness/.cargo/config.toml" \
            "git -C $path restore harness/.cargo/config.toml"
    fi

    # A cold target/ turns the first run_tests of either demo into a full
    # build of the crate and its dependencies, before a single test runs.
    if [ -d "$path/harness/target/debug" ]; then
        ok "$label has a warm target/"
    else
        bad "$label target/ is cold" "(cd $path/harness && cargo test)"
    fi

    valid="$( (cd "$path/harness" && spec validate --json 2>/dev/null) | grep -c '"valid": *true')"
    if [ "${valid:-0}" -ge 1 ]; then
        ok "$label catalog is valid"
    else
        bad "$label catalog is not valid" "(cd $path/harness && spec validate) and read the issues"
    fi

    if [ -n "${SKIP_SUITE:-}" ]; then
        note "$label suite not run (SKIP_SUITE set)"
        return
    fi
    log="/tmp/preflight-day-$label.log"
    if (cd "$path/harness" && cargo test --no-fail-fast >"$log" 2>&1); then
        ok "$label suite is green"
    else
        bad "$label suite is red" \
            "see $log — a pre-existing red bar stops demo 1, which treats unrelated failures as foreign and refuses to implement against them"
    fi
}

check_worktree demo1 "$DEMO1"

# Demo 1 starts from nothing: no requirement from a rehearsal, and no bar left
# behind by one. Both files are gitignored, so `git restore` does not do it.
if [ -d "$DEMO1/harness" ]; then
    if (cd "$DEMO1/harness" && spec list 2>/dev/null) | grep -q 'HARNESS-025'; then
        bad "demo1 already holds HARNESS-025 (rehearsal leftover)" \
            "cd $DEMO1 && git restore . && git clean -fd harness/tests"
    else
        ok "demo1 has no HARNESS-025 yet"
    fi
    if [ -f "$DEMO1/harness/.spec/state.json" ]; then
        bad "demo1 has a leftover .spec/state.json" \
            "rm -f $DEMO1/harness/.spec/state.json — or the demo opens on a bar it did not earn"
    else
        ok "demo1 has no leftover TDD state"
    fi
fi

check_worktree demo2 "$DEMO2"

# Demo 2 no longer has demo 1's segment to run underneath, so it has to replay
# a rehearsal. An empty cache means 20 to 57 minutes, live.
if [ -d "$DEMO2/harness" ]; then
    CACHED="$(find "$DEMO2/harness/.spec/cache" -maxdepth 1 -name '*.json' 2>/dev/null | wc -l | tr -d ' ')"
    if [ "${CACHED:-0}" -gt 0 ]; then
        ok "demo2 cache holds $CACHED entry/entries"
        AGE="$(find "$DEMO2/harness/.spec/cache" -maxdepth 1 -name '*.json' -mtime +1 2>/dev/null | wc -l | tr -d ' ')"
        if [ "${AGE:-0}" -gt 0 ]; then
            note "$AGE cached entry/entries are older than the 86400s TTL and will not be reused"
        fi
    else
        bad "demo2 cache is empty — the run will be cold, at 20 to 57 minutes" \
            "rehearse it: (cd $DEMO2/harness && spec deliver \"explain one spec command by name - what it does\"), then git restore . && rm -f harness/.spec/state.json, keeping .spec/cache/"
    fi
    if [ -f "$DEMO2/harness/.spec/state.json" ]; then
        bad "demo2 has a leftover .spec/state.json" \
            "rm -f $DEMO2/harness/.spec/state.json — keep .spec/cache/, delete only the state"
    else
        ok "demo2 has no leftover TDD state"
    fi
fi

echo
echo "Result: $PASS passed, $FAIL failed."
if [ "$FAIL" -gt 0 ]; then
    echo "NOT ready. Fix the failures above and re-run."
    exit 1
fi
cat <<MANUAL
Ready. Three manual steps left, none of which a script can see:
  1. Open the editor on $DEMO1/harness — not the repository root, or it
     finds no skill and points the server at the wrong catalog.
  2. Confirm the editor lists the spec-driven-server tools before you walk on.
  3. Open the slides once to cache the reveal.js CDN assets.
Afterwards, grade the run with:
  scripts/verify-harness-morning.sh check HARNESS-025 [deliver-report.json]
MANUAL
