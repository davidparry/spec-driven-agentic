//! Guards the facts that more than one file states.
//!
//! Nothing else in the suite reads `talks/`, `student-follow-docs/`, or
//! the site. That gap is not theoretical: the error message quoted in
//! `spec-binary-follow-along.md` went stale in a commit that ran every
//! other test green, and a slide told the room to run `spec scenario
//! add` without the flags it requires. Both would have failed here.
//!
//! The rule for what belongs in this file: a fact with exactly one
//! source of truth, restated somewhere a human reads. The source wins,
//! and a failure names the file, the line, and what the source says.
//!
//! Facts left unguarded on purpose, because they are observations
//! rather than contracts: timings, the probabilities quoted in
//! transcripts, slide counts, and the test counts in kata walkthroughs,
//! which change legitimately as a student adds scenarios.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const SPEC: &str = env!("CARGO_BIN_EXE_spec");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the harness crate sits one level under the repository root")
        .to_path_buf()
}

/// Every file a reader is pointed at, with the generated copies left out.
///
/// `docs/manual/` is mdbook's output of `manual/src/` and `_site/` is
/// the built site, so guarding those would report one drift twice and
/// tempt someone to edit the copy instead of the source. `prompts/` is
/// addressed to a model, not a reader, and its prose mentions commands
/// in sentences rather than instructing anyone to run them.
fn published_files() -> Vec<(String, String)> {
    let root = repo_root();
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];

    while let Some(dir) = stack.pop() {
        let listing = match std::fs::read_dir(&dir) {
            Ok(listing) => listing,
            Err(_) => continue,
        };
        for entry in listing.flatten() {
            let path = entry.path();
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let skip = relative.starts_with(".git")
                || relative.starts_with("target")
                || relative.starts_with("docs/manual")
                || relative.starts_with("_site")
                || relative.starts_with("harness/prompts")
                || relative.starts_with("node_modules")
                || relative == "notes";
            if skip {
                continue;
            }

            if path.is_dir() {
                stack.push(path);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("md" | "html")
            ) && let Ok(text) = std::fs::read_to_string(&path)
            {
                found.push((relative, text));
            }
        }
    }
    found.sort();
    found
}

/// A line that is prose about a command rather than a command to run.
///
/// Arrows are how several pages draw the loop (`spec -> validate ->
/// refine`), an ellipsis is the author saying "and the rest", and a
/// page that labels its own example as not real means it.
fn is_prose_not_a_command(line: &str) -> bool {
    ["->", "-->", "→", "—", "…", "...", "does not exist"]
        .iter()
        .any(|marker| line.contains(marker))
}

#[test]
fn every_documented_version_floor_is_the_version_this_crate_ships() {
    let shipped = env!("CARGO_PKG_VERSION");
    let mut wrong = Vec::new();

    for (file, text) in published_files() {
        // Floors wrap across lines in two files, so the needle is
        // hunted in text with its runs of whitespace flattened, and
        // the line is recovered afterwards for the message.
        for (number, line) in text.lines().enumerate() {
            let flattened = line.split_whitespace().collect::<Vec<_>>().join(" ");
            // A wrapped floor is caught from its first line, whose
            // window reaches the "newer" on the next one. Triggering on
            // that continuation too would read the sentence after it.
            let claims_a_floor = flattened.contains(" or newer") || flattened.ends_with(" or");
            if !claims_a_floor {
                continue;
            }
            // A floor names a version on the same line or the next one.
            let window = format!("{flattened} {}", text.lines().nth(number + 1).unwrap_or(""))
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");

            // Pages state floors for Ollama, Java, and Maven too. Only
            // `spec`'s own floor is this crate's to be right about.
            let lowercase = window.to_lowercase();
            let belongs_to_something_else = ["ollama", "java", "maven", "mvn", "node", "cargo"]
                .iter()
                .any(|other| lowercase.contains(other));
            if belongs_to_something_else && !lowercase.contains("spec --version") {
                continue;
            }

            // The floor is the version the phrase "or newer" is about,
            // so take the one just before it. Pages also discuss older
            // releases by name in the same breath ("0.5.5 was the first
            // release that..."), and those are history, not a floor.
            let Some(before) = window.split(" or newer").next() else {
                continue;
            };
            if let Some(floor) = versions_in(before).last()
                && floor != shipped
            {
                wrong.push(format!(
                    "{file}:{} claims floor {floor}, but this crate ships {shipped}\n    {}",
                    number + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "a reader on the version these pages name would not get what they describe:\n{}",
        wrong.join("\n")
    );
}

/// Every `N.N.N` in a string.
fn versions_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == '.') {
                index += 1;
            }
            let candidate: String = bytes[start..index].iter().collect();
            if candidate.split('.').count() == 3
                && candidate
                    .split('.')
                    .all(|p| !p.is_empty() && p.parse::<u32>().is_ok())
            {
                found.push(candidate);
            }
        } else {
            index += 1;
        }
    }
    found
}

#[test]
fn every_documented_tool_count_is_the_number_the_server_serves() {
    let served = tools_the_server_lists();
    assert_eq!(
        served, 25,
        "the server's tool count moved. Every page that states it needs the new \
         number before this assertion is changed to match"
    );

    let mut wrong = Vec::new();
    for (file, text) in published_files() {
        // The changelog's job is to say what was true at the time, so
        // "23 tools" in a 0.5.x entry is a record, not a stale claim.
        if file == "CHANGELOG.md" {
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            for claimed in counts_of_tools_in(line) {
                // The talk adds the 26th tool on stage, so a page may
                // correctly say either the number served or one more.
                if claimed != served && claimed != served + 1 {
                    wrong.push(format!(
                        "{file}:{} says {claimed} tools; the server lists {served} \
                         (or {} mid-demo)\n    {}",
                        number + 1,
                        served + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "a tool count no longer matches the server:\n{}",
        wrong.join("\n")
    );
}

/// How many tools `spec mcp tools` lists, asked of the binary itself.
fn tools_the_server_lists() -> usize {
    let output = Command::new(SPEC)
        .args(["mcp", "tools"])
        .current_dir(repo_root())
        .output()
        .expect("`spec mcp tools` runs");
    assert!(
        output.status.success(),
        "`spec mcp tools` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && line
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
        .count()
}

/// Counts written as "N tools", ignoring "7 tools" style counts of
/// something else by requiring the number to be plausible for a server.
fn counts_of_tools_in(line: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let words: Vec<&str> = line.split_whitespace().collect();
    for pair in words.windows(2) {
        let noun = pair[1].trim_matches(|c: char| !c.is_ascii_alphabetic());
        if noun != "tools" {
            continue;
        }
        let number = pair[0].trim_matches(|c: char| !c.is_ascii_digit());
        if let Ok(count) = number.parse::<usize>()
            && count >= 20
        {
            found.push(count);
        }
    }
    found
}

#[test]
fn every_spec_command_a_reader_is_told_to_run_exists() {
    let mut occurrences = Vec::new();
    for (file, text) in published_files() {
        for (number, line) in command_lines(&file, &text) {
            if let Some(path) = subcommand_path(&line) {
                occurrences.push((file.clone(), number, line, path));
            }
        }
    }

    let distinct: std::collections::BTreeSet<_> =
        occurrences.iter().map(|(_, _, _, path)| path).collect();
    assert!(
        distinct.len() > 30,
        "only {} distinct commands found - the scan is broken, not the docs",
        distinct.len()
    );

    // The binary is the authority on both questions, so ask it once per
    // command and reuse the answer.
    let mut help: BTreeMap<Vec<String>, Option<String>> = BTreeMap::new();
    let mut wrong = Vec::new();

    for (file, line_number, line, path) in &occurrences {
        let answer = help.entry(path.clone()).or_insert_with(|| {
            let output = Command::new(SPEC)
                .args(path)
                .arg("--help")
                .current_dir(repo_root())
                .output()
                .expect("`spec --help` runs");
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
        });

        let Some(help_text) = answer else {
            wrong.push(format!(
                "{file}:{line_number} runs `spec {}`, which the binary does not accept\n    {line}",
                path.join(" ")
            ));
            continue;
        };

        // Existing is not enough. The slide that sent a room to
        // `spec scenario add --feature ... --req ...` named a real
        // command and still produced a usage error, because `--name`
        // is required and was not there.
        for flag in required_flags(help_text) {
            if !line.contains(&flag) {
                wrong.push(format!(
                    "{file}:{line_number} runs `spec {}` without the required `{flag}`, \
                     so it exits with a usage error\n    {line}",
                    path.join(" ")
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "a reader following these pages would hit a usage error:\n{}",
        wrong.join("\n")
    );
}

/// The flags clap prints bare in its `Usage:` line, which are the ones
/// it will refuse to run without. Optional flags appear as `[OPTIONS]`
/// or in brackets, so anything left starting with `--` is required.
fn required_flags(help: &str) -> Vec<String> {
    let Some(usage) = help
        .lines()
        .find(|line| line.trim_start().starts_with("Usage:"))
    else {
        return Vec::new();
    };
    usage
        .split_whitespace()
        .filter(|word| word.starts_with("--"))
        .map(|word| word.to_string())
        .collect()
}

/// The `spec ...` lines a reader would actually type, which means the
/// ones inside a code block.
///
/// Prose wraps, and a sentence can begin with the word "spec" on a line
/// of its own — "spec by contract, says so in its `nextStep`..." is not
/// an instruction to run `spec by`. Only fenced blocks in markdown and
/// `<pre>` blocks in HTML carry commands.
fn command_lines(file: &str, text: &str) -> Vec<(usize, String)> {
    let html = file.ends_with(".html");
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut inside = false;

    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim();

        if html {
            if trimmed.contains("<pre") {
                inside = true;
            }
            if trimmed.contains("</pre>") {
                inside = false;
                continue;
            }
        } else if trimmed.starts_with("```") {
            inside = !inside;
            continue;
        }
        if !inside {
            continue;
        }

        // Only HTML gets its tags stripped. Doing it to markdown turns
        // the REPL's `spec> exit` prompt into a `spec exit` command.
        let line = if html {
            strip_tags(trimmed)
        } else {
            trimmed.to_string()
        };
        let line = line.trim().trim_start_matches("$ ").trim();
        if !line.starts_with("spec ") || is_prose_not_a_command(line) {
            continue;
        }

        // A command wrapped over several lines with `\` is still one
        // command, and its required flags are mostly on the later ones.
        let mut whole = line.trim_end_matches('\\').trim_end().to_string();
        let mut cursor = index;
        while lines[cursor].trim_end().ends_with('\\') && cursor + 1 < lines.len() {
            cursor += 1;
            let next = lines[cursor].trim();
            let next = if html {
                strip_tags(next)
            } else {
                next.to_string()
            };
            whole.push(' ');
            whole.push_str(next.trim().trim_end_matches('\\').trim_end());
        }

        found.push((index + 1, whole));
    }
    found
}

/// HTML without its tags or the entities the slides use.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth = 0usize;
    for character in line.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(character),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
}

/// The subcommand words of a `spec ...` line: what is left after the
/// global flags and before the first argument or flag.
fn subcommand_path(line: &str) -> Option<Vec<String>> {
    let line = line.split('#').next().unwrap_or(line);
    let mut words = line.split_whitespace().skip(1).peekable();
    let mut path = Vec::new();

    while let Some(word) = words.next() {
        if word == "--root" || word == "--config" {
            words.next();
            continue;
        }
        // Cheat-sheet blocks put two commands on one line, in columns.
        if word == "spec" {
            break;
        }
        if word.starts_with('-') {
            break;
        }
        // Subcommands are lowercase words; anything else is an
        // argument, like a requirement id or a file path.
        if !word.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            break;
        }
        path.push(word.to_string());
        if path.len() == 3 {
            break;
        }
    }

    (!path.is_empty()).then_some(path)
}

#[test]
fn the_decision_band_the_pages_quote_is_the_one_the_code_applies() {
    let applied = spec_harness::domain::decision::DEFAULT_MIN_CONFIDENCE;
    assert_eq!(
        applied, 0.80,
        "the decision band moved. Every page quoting 0.80 needs the new number, \
         and the figures measured against the old band no longer describe it"
    );

    // Stated as a band with both edges, so the complement matters too.
    let complement = format!("{:.2}", 1.0 - applied);
    let band = format!("{applied:.2}");

    let quotes = published_files()
        .into_iter()
        .filter(|(file, _)| file.starts_with("manual/src/commands/") || file.starts_with("talks/"))
        .filter(|(_, text)| text.contains("decision band") || text.contains("DECISION"))
        .count();
    assert!(
        quotes > 0,
        "no page explains the decision band any more; it is the one number \
         a reader needs to read a judgment"
    );

    let mut wrong = Vec::new();
    for (file, text) in published_files() {
        for (number, line) in text.lines().enumerate() {
            if line.contains("decision band") && !line.contains(&band) {
                // The sentence may carry the number on the next line.
                let next = text.lines().nth(number + 1).unwrap_or("");
                if !next.contains(&band) {
                    wrong.push(format!(
                        "{file}:{} describes the decision band without saying {band}\n    {}",
                        number + 1,
                        line.trim()
                    ));
                }
            }
            // The lower edge may be written as the number or left as
            // the expression it comes from, which cannot go stale.
            let edge_is_derived = line.contains("1 - threshold")
                || line.contains("1 - min_confidence")
                || line.contains("1 -&nbsp;threshold");
            if line.contains("at or below") && !line.contains(&complement) && !edge_is_derived {
                let next = text.lines().nth(number + 1).unwrap_or("");
                if !next.contains(&complement) && !next.contains("1 - threshold") {
                    wrong.push(format!(
                        "{file}:{} gives the band's lower edge as something other than \
                         {complement}\n    {}",
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "the published decision band no longer matches DEFAULT_MIN_CONFIDENCE:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn the_measured_accuracy_the_pages_quote_matches_the_labelled_set() {
    let labelled = std::fs::read_to_string(repo_root().join("harness/tests/decision_live.rs"))
        .expect("the live evaluation is readable");

    let cases = labelled.matches("label: Label::").count();

    // Prose says this result a dozen ways - "zero misses", "0 misses",
    // "one false alarm" - and a guard that regexes prose would cry wolf
    // until someone deleted it. So the figure is pinned where it is
    // written once: the constant every surface interpolates. Pages are
    // held to that constant by quoting it, not by matching here.
    let canonical = spec_harness::domain::decision::DECISION_PLANE_HELP;
    for figure in ["0 misses", "1 false alarm", "3 left unsure", "32-criterion"] {
        assert!(
            canonical.contains(figure),
            "DECISION_PLANE_HELP stopped reporting `{figure}`. It is the one \
             place the measured result is written; `spec refine --help`, \
             `spec judge criterion --help`, and the manual all read from it."
        );
    }

    // The result is only meaningful against the set it was measured on.
    assert_eq!(
        cases, 32,
        "the labelled set is now {cases} cases, so `0 misses, 1 false alarm, \
         3 left unsure` in DECISION_PLANE_HELP describes a run that no longer \
         exists. Re-run `cargo test --test decision_live -- --ignored` and \
         publish what it actually produces."
    );
}
