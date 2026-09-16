//! Live end-to-end happy path: spawn the built `bdd` binary and drive the
//! interactive `greenfield` loop over piped stdio until REQ-001 lands as
//! "implemented" in requirements/requirements.json.
//!
//! Piped stdin steers the harness onto the `ConsolePrompter` path (no TTY
//! needed), and every prompt is flushed before the read, so a small
//! expect-style loop can watch stdout, match the pending prompt, and
//! answer it - adaptively, because the live model decides how many
//! criteria, rewording rounds, and implementation attempts occur.
//!
//! Every run writes an artifacts directory (default
//! `harness/target/greenfield-e2e/<unix-seconds>/` - always the crate's own
//! target/, regardless of the working directory - override with
//! `BDD_E2E_ARTIFACTS`):
//!
//! - `transcript.log` - the full ANSI-stripped session
//! - `steps.jsonl` - one timestamped JSON event per prompt, answer, and
//!   milestone (scaffold, staged files, attempts, RED/GREEN bars)
//! - `summary.md` - the verdict; on failure it is a root-cause analysis
//!   (attempts used, error hotspots, files the model touched, the likely
//!   cause)
//! - `project/` - on failure, a copy of the generated project for
//!   inspection (build output excluded)
//!
//! The test is `#[ignore]`d: it needs Ollama serving at least one model,
//! a JDK, and Maven on PATH, and it runs for minutes. Run it with:
//!
//! ```sh
//! cargo test --test greenfield_e2e -- --ignored --nocapture
//! ```
//!
//! Knobs: `BDD_E2E_MODEL` picks the model (default: bdd's own discovery),
//! `BDD_E2E_TIMEOUT_SECS` bounds the whole run (default 3600 - a full
//! 30-attempt budget takes roughly two minutes per attempt),
//! `BDD_E2E_PROMPT_TIMEOUT_SECS` bounds silence between outputs
//! (default 300).
//!
//! The implementation budget is granted exactly once: the driver answers
//! the budget prompt with 30 attempts, and if the loop comes back asking
//! for more the whole budget burned RED - the test fails right there
//! instead of granting another round.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BDD: &str = env!("CARGO_BIN_EXE_bdd");

const PROJECT_NAME: &str = "String Calculator";
const DESCRIPTION: &str = "String calculator only intended for addition";
const DONE_MARKER: &str = "is implemented. Loop closed.";

/// How many hands-off implementation attempts the driver grants - once.
const ATTEMPT_BUDGET: &str = "30";

/// The RED-loop budget question. It appears once before the first
/// attempt and again only when a whole granted budget failed to go
/// green, so the driver treats a second appearance as the verdict.
fn is_budget_prompt(prompt: &str) -> bool {
    prompt.contains("Press Enter to let the model attempt")
}

/// The answer for the prompt currently pending at the end of the child's
/// output, or `None` when the tail is narration rather than a question.
/// Specific patterns come before generic ones; the manual-draft fallbacks
/// at the bottom only fire if the model produced no proposal to accept.
fn answer_for(prompt: &str) -> Option<&'static str> {
    if prompt.contains("Language for the new project") {
        return Some("java");
    }
    if prompt.ends_with("Project name:") {
        return Some(PROJECT_NAME);
    }
    if prompt.contains("Describe what to build in plain words") {
        return Some(DESCRIPTION);
    }
    if prompt.contains("Accept [Enter for all") {
        return Some("");
    }
    if prompt.contains("Which requirement first") {
        return Some("");
    }
    if prompt.contains("(Enter keeps it") {
        return Some(""); // accept every proposed title, story, criterion
    }
    if prompt.contains("Stage this requirement?") {
        return Some("y");
    }
    if prompt.contains("Commit the generated tests and step definitions?") {
        return Some("y");
    }
    if is_budget_prompt(prompt) {
        return Some(ATTEMPT_BUDGET); // attempt budget, as in the manual session
    }
    if prompt.contains("Start a refactor step") {
        return Some("n");
    }
    if prompt.contains("Which pending requirement next?") {
        return Some("n"); // one closed loop is the E2E scope
    }
    // Manual-draft fallbacks: the model returned no usable proposal, so
    // the wizard asks for bare wording. Supply a minimal clean spec so
    // the happy path still closes.
    if prompt.ends_with("title:") {
        return Some("Add two numbers");
    }
    if prompt.contains("story (As a ...") {
        return Some(
            "As a user, I want to add two numbers so that I get their sum \
             without doing the arithmetic myself.",
        );
    }
    if prompt.contains("criterion 1 (leave blank to finish") {
        return Some(
            "Given the inputs \"2\" and \"3\", when I submit them to the \
             calculator, then the result is \"5\"",
        );
    }
    if prompt.contains("criterion 2 (leave blank to finish") {
        return Some(
            "Given an empty string \"\" and \"5\", when I submit them to \
             the calculator, then the result is \"5\"",
        );
    }
    if prompt.contains("(leave blank to finish the criteria):") {
        return Some(""); // done adding criteria
    }
    None
}

/// Drop ANSI escape sequences (colors) so prompt matching sees the plain
/// words regardless of the console's highlighting.
fn strip_ansi(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            plain.push(c);
            continue;
        }
        // Skip a CSI sequence: ESC '[' parameters... final byte @..=~.
        if chars.next() == Some('[') {
            for follow in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&follow) {
                    break;
                }
            }
        }
    }
    plain
}

/// The pending prompt: whatever sits after the last line break, looking
/// only at output that arrived after the previous answer. Prompts are
/// flushed without a trailing newline, so consecutive questions share
/// one visual line - matching against the full line would re-match the
/// already-answered question's words.
fn pending_prompt(stripped: &str, answered_len: usize) -> &str {
    stripped[answered_len..]
        .rsplit(['\n', '\r'])
        .next()
        .unwrap_or("")
        .trim_end()
}

/// "Attempt N of M." - matched anywhere in a line, because the first
/// attempt announcement shares its line with the unterminated budget
/// prompt that preceded it.
fn attempt_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"Attempt \d+ of \d+\.").expect("a valid regex"))
}

/// Classify a completed transcript line as a loop milestone worth a
/// timestamped entry in the step log, or `None` for plain narration.
fn milestone_kind(line: &str) -> Option<&'static str> {
    if line.starts_with("Scaffolded ") {
        return Some("scaffold");
    }
    if line.contains("committed to the spec") {
        return Some("spec-committed");
    }
    if line.starts_with("Scenarios committed") {
        return Some("scenarios-committed");
    }
    if line.starts_with("Staged ") {
        return Some("staged");
    }
    if line.starts_with("Updated ") && line.ends_with("(llm).") {
        return Some("implement-update");
    }
    if attempt_re().is_match(line) {
        return Some("attempt");
    }
    if line.starts_with("RED:") {
        return Some("red-bar");
    }
    if line.starts_with("GREEN:") {
        return Some("green-bar");
    }
    if line.contains(DONE_MARKER) {
        return Some("loop-closed");
    }
    None
}

/// The per-run artifact collector: timestamped step events plus the
/// files written next to the transcript at the end of the run.
struct Reporter {
    dir: PathBuf,
    started: Instant,
    events: Vec<serde_json::Value>,
}

impl Reporter {
    fn new() -> Self {
        let dir = std::env::var("BDD_E2E_ARTIFACTS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let seconds = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("the clock is past 1970")
                    .as_secs();
                // Pinned to the crate's own target/ (harness/target), not the
                // current working directory - gitignored and swept by
                // cargo clean wherever the test is launched from.
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/greenfield-e2e")
                    .join(seconds.to_string())
            });
        std::fs::create_dir_all(&dir).expect("the artifacts directory is creatable");
        Self {
            dir,
            started: Instant::now(),
            events: Vec::new(),
        }
    }

    fn record(&mut self, kind: &str, text: &str) {
        self.events.push(serde_json::json!({
            "elapsedSecs": (self.started.elapsed().as_millis() as f64) / 1000.0,
            "kind": kind,
            "text": text,
        }));
    }

    /// Persist the transcript and the step log - written for every run,
    /// pass or fail, so the evidence never depends on the verdict.
    fn write_run_artifacts(&self, transcript: &str) {
        std::fs::write(self.dir.join("transcript.log"), transcript)
            .expect("the transcript is writable");
        let steps: String = self
            .events
            .iter()
            .map(|event| format!("{event}\n"))
            .collect();
        std::fs::write(self.dir.join("steps.jsonl"), steps).expect("the step log is writable");
    }

    fn write_summary(&self, body: &str) {
        std::fs::write(self.dir.join("summary.md"), body).expect("the summary is writable");
    }
}

/// The automated root-cause analysis for a failed run: what the loop
/// managed to do, where the errors clustered, which files the model
/// touched, and the most likely cause the evidence supports.
fn analyze(transcript: &str, reason: &str) -> String {
    let file_name = regex::Regex::new(r"([A-Za-z0-9_]+\.java)").expect("a valid regex");

    let last_attempt = attempt_re()
        .find_iter(transcript)
        .last()
        .map(|found| found.as_str());
    let last_bar = transcript
        .lines()
        .rfind(|line| line.starts_with("RED:") || line.starts_with("GREEN:"));

    let mut error_files: BTreeMap<String, usize> = BTreeMap::new();
    for line in transcript.lines().filter(|l| l.contains("[ERROR]")) {
        for capture in file_name.captures_iter(line) {
            *error_files.entry(capture[1].to_string()).or_default() += 1;
        }
    }
    let updated_files: BTreeSet<String> = transcript
        .lines()
        .filter(|line| {
            (line.starts_with("Updated ") || line.starts_with("Staged ")) && line.contains("(llm).")
        })
        .filter_map(|line| file_name.captures(line).map(|c| c[1].to_string()))
        .collect();
    let never_touched: Vec<&String> = error_files
        .keys()
        .filter(|file| !updated_files.contains(*file))
        .collect();

    let mut rca = String::from("# Greenfield E2E failure - root-cause analysis\n\n");
    rca.push_str(&format!("**Failure:** {reason}\n\n"));
    rca.push_str(&format!(
        "**Last attempt observed:** {}\n\n",
        last_attempt.unwrap_or("none - the run never reached the RED loop")
    ));
    rca.push_str(&format!(
        "**Last test bar:** {}\n\n",
        last_bar.unwrap_or("none - the tests never ran")
    ));
    if !error_files.is_empty() {
        rca.push_str("## Error hotspots (file: error-line mentions)\n\n");
        for (file, count) in &error_files {
            rca.push_str(&format!("- {file}: {count}\n"));
        }
        rca.push('\n');
    }
    if !updated_files.is_empty() {
        rca.push_str("## Files the model updated\n\n");
        for file in &updated_files {
            rca.push_str(&format!("- {file}\n"));
        }
        rca.push('\n');
    }
    rca.push_str("## Likely root cause\n\n");
    if !never_touched.is_empty() {
        let stuck: Vec<String> = never_touched.iter().map(|s| s.to_string()).collect();
        rca.push_str(&format!(
            "Compile errors persisted in {} but no implementation attempt \
             ever rewrote {} - the implement loop kept regenerating {} \
             instead. The model never received or never acted on the real \
             failing file, so extra attempts could not converge.\n",
            stuck.join(", "),
            if stuck.len() == 1 {
                "that file"
            } else {
                "those files"
            },
            updated_files.iter().cloned().collect::<Vec<_>>().join(", "),
        ));
    } else if last_bar.is_some_and(|bar| bar.starts_with("RED:")) {
        rca.push_str(
            "The loop stayed RED with the model editing the failing files - \
             the attempts changed the right targets but never satisfied the \
             tests within the budget. Inspect the project next to this \
             file and the transcript's final attempts for the residual \
             failures.\n",
        );
    } else {
        rca.push_str(
            "The run ended outside the RED loop - read the failure reason \
             above and the transcript tail for the step that stopped it.\n",
        );
    }
    rca
}

fn env_secs(name: &str, default: u64) -> Duration {
    let secs = std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default);
    Duration::from_secs(secs)
}

/// Fail fast, with instructions, when a prerequisite is missing - a
/// clearer story than a hung or half-run loop.
fn preflight() {
    let maven = Command::new("mvn")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    assert!(
        maven.is_ok_and(|status| status.success()),
        "preflight: `mvn -version` failed - install Maven and a JDK before \
         running the greenfield E2E test"
    );

    // The binary's own discovery path doubles as the Ollama probe: it
    // hits the configured endpoint's /api/tags exactly like greenfield.
    let models = Command::new(BDD)
        .args(["model", "list"])
        .output()
        .expect("preflight: the bdd binary should spawn");
    let listing = String::from_utf8_lossy(&models.stdout);
    assert!(
        models.status.success() && !listing.trim().is_empty(),
        "preflight: `bdd model list` found no models - is Ollama running \
         on localhost:11434 with at least one model pulled? stdout: {listing} \
         stderr: {}",
        String::from_utf8_lossy(&models.stderr)
    );
}

/// Kills the child if the test panics mid-drive - otherwise the bdd
/// process outlives the test and keeps generating against the model,
/// mutating the project snapshot the failure left behind.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn spawn_greenfield(root: &Path) -> Child {
    let mut command = Command::new(BDD);
    command.arg("--root").arg(root);
    if let Ok(model) = std::env::var("BDD_E2E_MODEL") {
        command.args(["--model", &model]);
    }
    command
        .arg("greenfield")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the bdd binary should spawn")
}

/// Drive the child to completion: read stdout, answer each prompt, log
/// every step, and read until the child exits (declining the offer to
/// continue with further pending requirements). Returns the
/// ANSI-stripped transcript and the drive verdict - `Err` carries the
/// reason instead of panicking so the caller can persist artifacts
/// first.
fn drive(child: &mut Child, reporter: &mut Reporter) -> (String, Result<(), String>) {
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let mut stdout = child.stdout.take().expect("stdout is piped");

    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut buffer = [0u8; 4096];
        while let Ok(count) = stdout.read(&mut buffer) {
            if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    let overall = env_secs("BDD_E2E_TIMEOUT_SECS", 3600);
    let silence = env_secs("BDD_E2E_PROMPT_TIMEOUT_SECS", 300);
    let started = Instant::now();
    let mut last_output = Instant::now();
    let mut stripped = String::new();
    let mut answered_len = 0usize;
    let mut scanned_len = 0usize;
    let mut budget_granted = false;

    // The loop-closed marker is no longer the stop signal: after it, the
    // run may still ask whether to continue with the next pending
    // requirement (answered "n" above), so the driver reads until the
    // child exits on its own.
    let verdict = loop {
        if started.elapsed() >= overall {
            break Err(format!(
                "the greenfield run exceeded {overall:?} (BDD_E2E_TIMEOUT_SECS)"
            ));
        }
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(chunk) => {
                let text = String::from_utf8_lossy(&chunk);
                print!("{text}"); // stream live progress for --nocapture
                let _ = std::io::stdout().flush();
                stripped.push_str(&strip_ansi(&text));
                last_output = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {
                if last_output.elapsed() >= silence {
                    break Err(format!(
                        "no output for {silence:?} (BDD_E2E_PROMPT_TIMEOUT_SECS) - \
                         the run looks hung"
                    ));
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break Ok(()), // child exited
        }
        // Log every milestone in the lines completed since the last scan.
        if let Some(newline) = stripped[scanned_len..].rfind('\n') {
            let complete = scanned_len + newline + 1;
            for line in stripped[scanned_len..complete].lines() {
                if let Some(kind) = milestone_kind(line.trim_end()) {
                    reporter.record(kind, line.trim_end());
                }
            }
            scanned_len = complete;
        }
        // Only the output that arrived since the last answer can hold a
        // new question - never re-answer a prompt that is still the tail.
        if stripped.len() <= answered_len {
            continue;
        }
        let prompt = pending_prompt(&stripped, answered_len);
        if prompt.contains("Implement the production code now") {
            break Err(
                "greenfield ran without a resolved model (templates only) - \
                 the happy path needs Ollama"
                    .into(),
            );
        }
        if is_budget_prompt(prompt) {
            if budget_granted {
                break Err(format!(
                    "still RED after the full {ATTEMPT_BUDGET}-attempt implementation \
                     budget - the loop asked for more attempts and the test grants \
                     exactly one budget"
                ));
            }
            budget_granted = true;
        }
        if let Some(answer) = answer_for(prompt) {
            reporter.record("prompt", prompt);
            reporter.record("answer", answer);
            println!("{answer}"); // echo the scripted answer into the log
            if writeln!(stdin, "{answer}")
                .and_then(|_| stdin.flush())
                .is_err()
            {
                break Err("the child stopped accepting answers (stdin closed)".into());
            }
            answered_len = stripped.len();
        }
    };
    (stripped, verdict)
}

fn transcript_tail(transcript: &str) -> String {
    let lines: Vec<&str> = transcript.lines().collect();
    let from = lines.len().saturating_sub(40);
    lines[from..].join("\n")
}

fn wait_for_exit(child: &mut Child) -> Result<std::process::ExitStatus, String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().expect("the child is waitable") {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err("the child did not exit within 60s of closing the loop".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The final checks, as reasons instead of panics so a failure still
/// lands in the artifacts before the test dies.
fn verify(root: &Path, transcript: &str) -> Result<(), String> {
    if !transcript.contains("GREEN:") {
        return Err("the run never reached a green bar".into());
    }
    if !transcript.contains(DONE_MARKER) {
        return Err("the loop never closed".into());
    }
    let spec_path = root.join("requirements/requirements.json");
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|e| format!("requirements.json is unreadable after the loop - {e}"))?;
    let spec: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("requirements.json is not JSON - {e}"))?;
    let requirements = spec["requirements"]
        .as_array()
        .ok_or("the spec holds no requirements array")?;
    let first = requirements
        .first()
        .ok_or_else(|| format!("the spec has no requirements: {spec}"))?;
    if first["id"] != "REQ-001" {
        return Err(format!("the first requirement is not REQ-001: {first}"));
    }
    if first["status"] != "implemented" {
        return Err(format!(
            "REQ-001 should be implemented, but the spec says: {first}"
        ));
    }
    let feature = first["featureFile"]
        .as_str()
        .ok_or("REQ-001 carries no feature file")?;
    if !root.join(feature).is_file() {
        return Err(format!(
            "the tagged feature file {feature} does not exist in the project root"
        ));
    }
    Ok(())
}

#[test]
#[ignore = "live E2E: needs Ollama with a model, a JDK, and Maven (cargo test --test greenfield_e2e -- --ignored --nocapture)"]
fn greenfield_happy_path_marks_the_requirement_implemented() {
    preflight();
    let mut reporter = Reporter::new();
    // The generated project is built and tested inside the crate's own
    // target/ folder, next to the run's other artifacts - never in the
    // system temp dir - so it can always be reopened and rebuilt.
    let root = reporter.dir.join("project");
    std::fs::create_dir_all(&root).expect("the project directory is creatable");
    let root = root.as_path();

    let mut child = ChildGuard(spawn_greenfield(root));
    let (transcript, drive_verdict) = drive(&mut child.0, &mut reporter);

    let verdict = drive_verdict
        .and_then(|_| {
            wait_for_exit(&mut child.0).and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("bdd greenfield exited with {status}"))
                }
            })
        })
        .and_then(|_| verify(root, &transcript));

    reporter.write_run_artifacts(&transcript);
    match verdict {
        Ok(()) => {
            reporter.write_summary(&format!(
                "# Greenfield E2E - PASSED\n\nREQ-001 reached \"implemented\" \
                 in requirements.json. Steps and transcript sit next to this \
                 file.\n\nArtifacts: {}\n",
                reporter.dir.display()
            ));
            println!("\nE2E artifacts: {}", reporter.dir.display());
        }
        Err(reason) => {
            // Stop the child so the project snapshot on disk is stable.
            let _ = child.0.kill();
            let _ = child.0.wait();
            reporter.write_summary(&analyze(&transcript, &reason));
            panic!(
                "{reason}\n\nRCA, step log, transcript, and the project: {}\n\n\
                 Transcript tail:\n{}",
                reporter.dir.display(),
                transcript_tail(&transcript)
            );
        }
    }
}

#[cfg(test)]
mod driver_unit {
    use super::*;

    #[test]
    fn ansi_colors_are_stripped_for_matching() {
        assert_eq!(
            strip_ansi("REQ-001 title \x1b[32m[Add]\x1b[0m (Enter keeps it): "),
            "REQ-001 title [Add] (Enter keeps it): "
        );
    }

    #[test]
    fn the_pending_prompt_is_the_unterminated_tail() {
        assert_eq!(
            pending_prompt("Scaffolded 5 files.\nProject name: ", 0),
            "Project name:"
        );
        assert_eq!(pending_prompt("REQ-001 committed to the spec.\n", 0), "");
    }

    #[test]
    fn consecutive_prompts_on_one_line_only_expose_the_newest_question() {
        // Prompts are not newline-terminated, so the language question
        // and the name question share a line. Matching must see only
        // the part that arrived after the language answer.
        let language =
            "Language for the new project (java, javascript, typescript, dotnet, rust): ";
        let both = format!("{language}Project name: ");
        assert_eq!(pending_prompt(&both, language.len()), "Project name:");
        assert_eq!(
            answer_for(pending_prompt(&both, language.len())),
            Some(PROJECT_NAME)
        );
    }

    #[test]
    fn every_happy_path_prompt_has_an_answer() {
        let script = [
            (
                "Language for the new project (java, javascript, typescript, dotnet, rust):",
                "java",
            ),
            ("Project name:", PROJECT_NAME),
            (
                "Describe what to build in plain words (one or several requirements). \
                 Enter drafts manually instead:",
                DESCRIPTION,
            ),
            ("Accept [Enter for all, or comma-separated numbers]:", ""),
            (
                "Which requirement first to review and refine? [1-2, Enter for 1]:",
                "",
            ),
            ("REQ-001 title [Add two numbers] (Enter keeps it):", ""),
            (
                "REQ-001 criterion 1 [Given \"2\" and \"3\", then \"5\"] \
                 (Enter keeps it, '-' drops it):",
                "",
            ),
            (
                "REQ-001 criterion 4 (leave blank to finish the criteria):",
                "",
            ),
            (
                "The wording reads clean. Stage this requirement? [y/N]",
                "y",
            ),
            (
                "Commit the generated tests and step definitions? [y/N]",
                "y",
            ),
            (
                "Press Enter to let the model attempt the implementation and rerun \
                 the tests, enter a number to attempt up to that many times without \
                 asking again, or type stop to pause here:",
                "30",
            ),
            (
                "Green bar. Start a refactor step before closing the loop? [y/N]",
                "n",
            ),
            (
                "Which pending requirement next? [1-1, Enter for 1] (n stops):",
                "n",
            ),
        ];
        for (prompt, expected) in script {
            assert_eq!(answer_for(prompt), Some(expected), "prompt: {prompt}");
        }
    }

    #[test]
    fn the_budget_prompt_is_recognized_and_answered_with_the_full_budget() {
        let prompt = "Press Enter to let the model attempt the implementation and rerun \
                      the tests, enter a number to attempt up to that many times without \
                      asking again, or type stop to pause here:";
        assert!(is_budget_prompt(prompt));
        assert_eq!(answer_for(prompt), Some(ATTEMPT_BUDGET));
        // Narration around the RED loop must not look like the budget
        // question - a false positive would fail the run instantly.
        for narration in [
            "Attempt 30 of 30.",
            "RED: 0 tests, 0 failures, 1 errors.",
            "Generating an implementation attempt - working ...",
        ] {
            assert!(!is_budget_prompt(narration), "narration: {narration}");
        }
    }

    #[test]
    fn narration_lines_are_not_answered() {
        for narration in [
            "",
            "Running the tests - working ...",
            "GREEN: 8 tests, 0 failures, 0 errors.",
            "REQ-001 committed to the spec.",
            "Saving status - working ...",
        ] {
            assert_eq!(answer_for(narration), None, "narration: {narration}");
        }
    }

    #[test]
    fn milestones_are_classified_and_narration_is_not() {
        let cases = [
            ("Scaffolded 5 files for Java (Cucumber-JVM).", "scaffold"),
            ("REQ-001 committed to the spec.", "spec-committed"),
            (
                "Scenarios committed to features/add-two-numbers.feature.",
                "scenarios-committed",
            ),
            (
                "Staged src/test/java/steps/GeneratedSteps.java (llm).",
                "staged",
            ),
            (
                "Updated src/main/java/StringCalculator.java (llm).",
                "implement-update",
            ),
            ("Attempt 11 of 30.", "attempt"),
            ("RED: 0 tests, 0 failures, 1 errors.", "red-bar"),
            ("GREEN: 8 tests, 0 failures, 0 errors.", "green-bar"),
            ("REQ-001 is implemented. Loop closed.", "loop-closed"),
        ];
        for (line, expected) in cases {
            assert_eq!(milestone_kind(line), Some(expected), "line: {line}");
        }
        // The first attempt announcement shares its line with the
        // unterminated budget prompt - it must still classify.
        assert_eq!(
            milestone_kind("asking again, or type stop to pause here: Attempt 1 of 30."),
            Some("attempt")
        );
        assert_eq!(milestone_kind("Findings to address:"), None);
        assert_eq!(
            milestone_kind("Project language: Java (Cucumber-JVM)."),
            None
        );
    }

    #[test]
    fn the_rca_names_error_files_the_model_never_rewrote() {
        let transcript = "\
or type stop to pause here: Attempt 1 of 30.
Attempt 4 of 30.
Updated src/main/java/StringCalculator.java (llm).
RED: 0 tests, 0 failures, 1 errors.
[ERROR] /tmp/x/src/test/java/steps/GeneratedSteps.java:[8,8] class CalculatorSteps is public
[ERROR] /tmp/x/src/test/java/steps/GeneratedSteps.java:[9,13] cannot find symbol
";
        let rca = analyze(transcript, "timed out");
        assert!(rca.contains("**Failure:** timed out"), "rca: {rca}");
        assert!(rca.contains("Attempt 4 of 30."), "rca: {rca}");
        assert!(rca.contains("GeneratedSteps.java: 2"), "rca: {rca}");
        assert!(
            rca.contains("no implementation attempt ever rewrote that file"),
            "rca: {rca}"
        );
        assert!(rca.contains("StringCalculator.java"), "rca: {rca}");
    }

    #[test]
    fn the_rca_blames_the_budget_when_the_model_edited_the_failing_files() {
        let transcript = "\
Attempt 30 of 30.
Updated src/test/java/steps/GeneratedSteps.java (llm).
[ERROR] /tmp/x/src/test/java/steps/GeneratedSteps.java:[8,8] boom
RED: 4 tests, 1 failures, 0 errors.
";
        let rca = analyze(transcript, "timed out");
        assert!(
            rca.contains("the attempts changed the right targets"),
            "rca: {rca}"
        );
    }
}
