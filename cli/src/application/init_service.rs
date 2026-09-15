//! `bdd init`: lay down the per-language scaffold (build file, Cucumber
//! runner, empty spec, CLI configuration). Existing files are never
//! touched, so init is safe to re-run and safe on half-scaffolded
//! directories.

use serde::Serialize;

use crate::application::spec_service::ServiceError;
use crate::domain::language::Language;
use crate::domain::scaffold::scaffold;
use crate::ports::ScaffoldWriter;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct InitReport {
    pub language: String,
    pub framework: String,
    pub created: Vec<String>,
    pub skipped: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct InitService<W: ScaffoldWriter> {
    writer: W,
}

impl<W: ScaffoldWriter> InitService<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn init(&self, language: Language, project_name: &str) -> Result<InitReport, ServiceError> {
        tracing::info!(language = %language.display(), project = project_name, "scaffolding");
        let mut created = Vec::new();
        let mut skipped = Vec::new();
        for file in scaffold(language, project_name) {
            if self.writer.write_new(&file.path, &file.content)? {
                created.push(file.path);
            } else {
                skipped.push(file.path);
            }
        }
        tracing::debug!(created = ?created, skipped = ?skipped, "scaffold outcome");
        Ok(InitReport {
            language: language.display().to_string(),
            framework: language.bdd_framework().to_string(),
            created,
            skipped,
            next_step: "Draft the first requirement with bdd spec draft (or run bdd greenfield \
                        for the whole loop)."
                .into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::ScaffoldError;
    use std::cell::RefCell;
    use std::collections::HashSet;

    #[derive(Default)]
    struct FakeWriter {
        existing: HashSet<String>,
        written: RefCell<Vec<String>>,
        fail_with: Option<String>,
    }

    impl ScaffoldWriter for FakeWriter {
        fn write_new(&self, path: &str, _content: &str) -> Result<bool, ScaffoldError> {
            if let Some(message) = &self.fail_with {
                return Err(ScaffoldError(message.clone()));
            }
            if self.existing.contains(path) {
                return Ok(false);
            }
            self.written.borrow_mut().push(path.to_string());
            Ok(true)
        }
    }

    #[test]
    fn a_fresh_directory_gets_every_scaffold_file() {
        let service = InitService::new(FakeWriter::default());
        let report = service.init(Language::Rust, "String Calculator").unwrap();
        assert_eq!(report.language, "Rust");
        assert_eq!(report.framework, "cucumber-rs");
        assert!(report.created.contains(&"Cargo.toml".to_string()));
        assert!(
            report
                .created
                .contains(&"requirements/requirements.json".to_string())
        );
        assert!(report.skipped.is_empty());
        assert!(report.next_step.contains("bdd spec draft"));
    }

    #[test]
    fn existing_files_are_reported_as_skipped_not_overwritten() {
        let writer = FakeWriter {
            existing: ["Cargo.toml".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let report = InitService::new(writer)
            .init(Language::Rust, "Calc")
            .unwrap();
        assert_eq!(report.skipped, vec!["Cargo.toml"]);
        assert!(!report.created.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn writer_failures_become_service_errors() {
        let writer = FakeWriter {
            fail_with: Some("disk full".into()),
            ..Default::default()
        };
        assert_eq!(
            InitService::new(writer)
                .init(Language::Java, "Calc")
                .unwrap_err(),
            ServiceError("disk full".into())
        );
    }
}
