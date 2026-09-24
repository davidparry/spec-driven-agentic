//! Getting a directory ready for the loop, for whichever command is
//! about to run it.
//!
//! An orchestrator cannot assume it was pointed at a project: the root
//! may hold no build markers, no spec, or no recorded memory. These are
//! the steps that settle that, and they are shared rather than owned by
//! one orchestrator so `deliver` does not have to depend on `greenfield`
//! to start a project.
//!
//! Unlike [`crate::wiring`] these may prompt: scaffolding needs a
//! language and a project name that no scan can supply.

use std::path::Path;

use crate::adapters::fs_project::FsProjectFiles;
use crate::adapters::fs_scaffold::FsScaffoldWriter;
use crate::application::init_service::InitService;
use crate::application::memory_service::LayoutAsk;
use crate::domain::language::{Language, detect_languages};
use crate::domain::memory::ProjectMemory;
use crate::domain::model::Spec;
use crate::ports::{PromptError, Prompter};
use crate::wiring::{DynLlm, project_memory_service};
use crate::workspace::SPEC_PATH;

/// Whether the root already holds marker files for a supported language,
/// so no scaffolding question needs asking.
pub fn project_detected(root: &Path) -> bool {
    !languages(root).is_empty()
}

/// Whether `root` holds a readable, non-empty requirements spec. False
/// is the greenfield signal: there is nothing to deliver from yet.
pub fn has_readable_spec(root: &Path) -> bool {
    std::fs::read_to_string(root.join(SPEC_PATH)).is_ok_and(|text| !text.trim().is_empty())
}

/// The project's language, scaffolding one first when the directory has
/// no marker files at all - the greenfield entry both orchestrators
/// start from.
pub fn ensure_project(root: &Path, prompter: &mut dyn Prompter) -> Result<Language, String> {
    if let Some(language) = languages(root).first().copied() {
        return Ok(language);
    }
    prompter.tell("No project detected - scaffolding a new one.");
    let language = prompt_language(prompter).map_err(|e| e.to_string())?;
    let name = prompter.ask("Project name:").map_err(|e| e.to_string())?;
    let report = InitService::new(FsScaffoldWriter::new(root.to_path_buf()))
        .init(language, &name)
        .map_err(|e| e.to_string())?;
    prompter.tell(&format!(
        "Scaffolded {} files for {} ({}).",
        report.created.len(),
        report.language,
        report.framework
    ));
    Ok(language)
}

/// An empty catalog at `requirements/requirements.json` when there is no
/// readable spec yet. A file that can be read is left exactly as it is.
pub fn ensure_spec(root: &Path, prompter: &mut dyn Prompter) -> Result<(), String> {
    if has_readable_spec(root) {
        return Ok(());
    }
    let path = root.join(SPEC_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{SPEC_PATH} is not writable - {error}"))?;
    }
    let spec = Spec {
        project: project_name(root),
        ..Spec::default()
    };
    let body = serde_json::to_string_pretty(&spec).expect("an empty spec always serializes");
    std::fs::write(&path, format!("{body}\n"))
        .map_err(|error| format!("{SPEC_PATH} is not writable - {error}"))?;
    prompter.tell(&format!(
        "Created {SPEC_PATH} — there was no readable spec yet."
    ));
    Ok(())
}

/// Ask for a supported language until the answer parses - the one
/// prompt loop shared by `spec init` and the greenfield scaffold step.
pub fn prompt_language(prompter: &mut dyn Prompter) -> Result<Language, PromptError> {
    loop {
        let answer = prompter
            .ask("Language for the new project (java, javascript, typescript, dotnet, rust):")?;
        match Language::parse(&answer) {
            Some(language) => return Ok(language),
            None => prompter.warn("Unrecognized language - pick one of the five listed."),
        }
    }
}

/// Scan the project and persist `.spec/memory.json`. Failures are logged
/// and yield empty memory so a read-only root still runs.
pub fn refresh_project_memory(root: &Path, chosen: Option<Language>) -> ProjectMemory {
    project_memory_service(root.to_path_buf())
        .refresh(chosen)
        .unwrap_or_else(|error| {
            tracing::debug!(error = %error.0, "project memory not refreshed");
            ProjectMemory::default()
        })
}

/// [`refresh_project_memory`] with the one layout question a scan cannot
/// answer: which module the harness works in when the tree holds several.
/// The model proposes, the developer confirms, and the answer is recorded,
/// so a session on an unambiguous project asks nothing.
pub fn settle_project_memory(
    root: &Path,
    llm: Option<&(String, DynLlm)>,
    attempts: u32,
    prompter: &mut dyn Prompter,
) -> ProjectMemory {
    let ask = llm.map(|(model, llm)| LayoutAsk {
        model,
        llm: llm.as_ref(),
        attempts,
    });
    project_memory_service(root.to_path_buf())
        .settle(None, ask, prompter)
        .unwrap_or_else(|error| {
            tracing::debug!(error = %error.0, "project memory not settled");
            ProjectMemory::default()
        })
}

fn languages(root: &Path) -> Vec<Language> {
    detect_languages(&FsProjectFiles::new(root.to_path_buf()))
}

/// The directory's own name, as the default project name for a spec
/// created from nothing.
fn project_name(root: &Path) -> String {
    root.canonicalize()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "project".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_is_detected_from_its_marker_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!project_detected(dir.path()));
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        assert!(project_detected(dir.path()));
    }

    #[test]
    fn a_spec_is_readable_only_when_it_holds_something() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_readable_spec(dir.path()));
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(dir.path().join(SPEC_PATH), "   \n").unwrap();
        assert!(!has_readable_spec(dir.path()), "blank is not readable");
        std::fs::write(dir.path().join(SPEC_PATH), r#"{"project":"Kata"}"#).unwrap();
        assert!(has_readable_spec(dir.path()));
    }

    /// The name is the directory's, so a spec created from nothing is not
    /// called "project" when the folder already says what this is.
    #[test]
    fn a_created_spec_is_named_after_its_directory() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("string-calculator");
        std::fs::create_dir_all(&root).unwrap();
        let mut prompter = Silent::default();
        ensure_spec(&root, &mut prompter).unwrap();
        let written = std::fs::read_to_string(root.join(SPEC_PATH)).unwrap();
        assert!(written.contains("string-calculator"), "{written}");
        assert!(prompter.told.iter().any(|line| line.contains(SPEC_PATH)));
    }

    /// A spec already on disk is never rewritten - the catalog is the
    /// author's, and a bootstrap step must not touch it.
    #[test]
    fn an_existing_spec_is_left_exactly_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        let original = r#"{"project":"Mine","requirements":[]}"#;
        std::fs::write(dir.path().join(SPEC_PATH), original).unwrap();
        ensure_spec(dir.path(), &mut Silent::default()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(SPEC_PATH)).unwrap(),
            original
        );
    }

    #[test]
    fn a_detected_project_is_not_scaffolded_and_asks_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"k\"\n").unwrap();
        let language = ensure_project(dir.path(), &mut Silent::default()).unwrap();
        assert_eq!(language, Language::Rust);
    }

    /// The language question re-asks rather than accepting a guess, which
    /// is also why an unattended run must not reach it.
    #[test]
    fn the_language_question_repeats_until_the_answer_parses() {
        let mut prompter = Silent {
            answers: ["klingon", "", "rust"]
                .iter()
                .map(|a| a.to_string())
                .collect(),
            ..Silent::default()
        };
        assert_eq!(prompt_language(&mut prompter).unwrap(), Language::Rust);
        assert_eq!(
            prompter
                .told
                .iter()
                .filter(|line| line.contains("Unrecognized language"))
                .count(),
            2
        );
    }

    #[test]
    fn a_scan_of_an_unreadable_root_yields_empty_memory_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            refresh_project_memory(dir.path(), None),
            ProjectMemory::default()
        );
    }

    /// Settling records what the scan found and says so; a memory file
    /// that cannot be written leaves the run going on empty memory.
    #[test]
    fn settling_memory_records_the_language_and_a_broken_file_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        let mut prompter = Silent::default();
        let memory = settle_project_memory(dir.path(), None, 1, &mut prompter);
        assert_eq!(memory.language, "Java");
        assert_eq!(prompter.told, vec!["Reading the project - working ..."]);

        let broken = dir.path().join(".spec/memory.json");
        std::fs::remove_file(&broken).unwrap();
        std::fs::create_dir_all(&broken).unwrap();
        assert!(refresh_project_memory(dir.path(), None).is_empty());
        assert!(settle_project_memory(dir.path(), None, 1, &mut prompter).is_empty());
    }

    #[derive(Default)]
    struct Silent {
        answers: std::collections::VecDeque<String>,
        told: Vec<String>,
    }

    impl Prompter for Silent {
        fn tell(&mut self, message: &str) {
            self.told.push(message.to_string());
        }
        fn ask(&mut self, question: &str) -> Result<String, PromptError> {
            self.told.push(question.to_string());
            self.answers
                .pop_front()
                .ok_or_else(|| PromptError("script exhausted".into()))
        }
        fn confirm(&mut self, _question: &str) -> Result<bool, PromptError> {
            Ok(false)
        }
    }
}
