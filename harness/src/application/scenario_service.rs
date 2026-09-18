//! Typed feature and scenario mutations. Every edit is parsed back as
//! Gherkin before it is staged, so broken syntax can never reach the
//! staging area, let alone the working tree.

use serde::Serialize;

use crate::application::spec_service::ServiceError;
use crate::domain::feature::{self, FeatureDoc, ScenarioDoc};
use crate::ports::{ChangeStore, FeatureCatalog};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MutationReport {
    pub feature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    pub action: String,
    pub staged: bool,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct ScenarioService<C: ChangeStore, F: FeatureCatalog> {
    store: C,
    catalog: F,
}

impl<C: ChangeStore, F: FeatureCatalog> ScenarioService<C, F> {
    pub fn new(store: C, catalog: F) -> Self {
        Self { store, catalog }
    }

    pub fn create_feature(&self, path: &str, name: &str) -> Result<MutationReport, ServiceError> {
        if self.effective_doc(path)?.is_some() {
            return Err(ServiceError(format!(
                "{path} already exists - add scenarios to it with scenario add."
            )));
        }
        let doc = FeatureDoc {
            path: path.to_string(),
            name: name.to_string(),
            tags: Vec::new(),
            comments: Vec::new(),
            description: Vec::new(),
            scenarios: Vec::new(),
        };
        self.stage(&doc, &format!("create feature \"{name}\""))?;
        Ok(self.report(path, None, "create"))
    }

    pub fn add_scenario(
        &self,
        path: &str,
        req_id: &str,
        name: &str,
        steps: Vec<String>,
    ) -> Result<MutationReport, ServiceError> {
        check_steps(&steps)?;
        let mut doc = self.existing_doc(path)?;
        if doc.scenarios.iter().any(|s| s.name == name) {
            return Err(ServiceError(format!(
                "{path}: scenario \"{name}\" already exists - change it with scenario update."
            )));
        }
        doc.scenarios.push(ScenarioDoc {
            name: name.to_string(),
            tags: vec![requirement_tag(req_id)],
            steps,
        });
        self.stage(&doc, &format!("add scenario \"{name}\" for {req_id}"))?;
        Ok(self.report(path, Some(name), "add"))
    }

    pub fn update_scenario(
        &self,
        path: &str,
        name: &str,
        steps: Vec<String>,
        req_id: Option<&str>,
    ) -> Result<MutationReport, ServiceError> {
        check_steps(&steps)?;
        let mut doc = self.existing_doc(path)?;
        let scenario = doc
            .scenarios
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| no_such_scenario(path, name))?;
        if !steps.is_empty() {
            scenario.steps = steps;
        }
        if let Some(req_id) = req_id {
            scenario.tags = vec![requirement_tag(req_id)];
        }
        self.stage(&doc, &format!("update scenario \"{name}\""))?;
        Ok(self.report(path, Some(name), "update"))
    }

    pub fn delete_scenario(&self, path: &str, name: &str) -> Result<MutationReport, ServiceError> {
        let mut doc = self.existing_doc(path)?;
        let before = doc.scenarios.len();
        doc.scenarios.retain(|s| s.name != name);
        if doc.scenarios.len() == before {
            return Err(no_such_scenario(path, name));
        }
        self.stage(&doc, &format!("delete scenario \"{name}\""))?;
        Ok(self.report(path, Some(name), "delete"))
    }

    /// The feature as it would look after commit: staged content wins,
    /// then the working tree; `None` when the file exists nowhere.
    fn effective_doc(&self, path: &str) -> Result<Option<FeatureDoc>, ServiceError> {
        if let Some(content) = self.store.content(path)? {
            return feature::parse(path, &content)
                .map(Some)
                .map_err(ServiceError);
        }
        if !self.catalog.exists(path) {
            return Ok(None);
        }
        self.catalog
            .read(path)
            .map(Some)
            .map_err(ServiceError::from)
    }

    fn existing_doc(&self, path: &str) -> Result<FeatureDoc, ServiceError> {
        self.effective_doc(path)?.ok_or_else(|| {
            ServiceError(format!(
                "{path}: no such feature file. Create it first with feature create."
            ))
        })
    }

    /// Render, re-parse (the syntax gate), then stage.
    fn stage(&self, doc: &FeatureDoc, summary: &str) -> Result<(), ServiceError> {
        let text = feature::render(doc);
        feature::parse(&doc.path, &text).map_err(ServiceError)?;
        self.store.stage(&doc.path, &text, summary)?;
        Ok(())
    }

    fn report(&self, path: &str, scenario: Option<&str>, action: &str) -> MutationReport {
        MutationReport {
            feature: path.to_string(),
            scenario: scenario.map(String::from),
            action: action.to_string(),
            staged: true,
            next_step: "Review with changes show, run validate, then apply with \
                        changes commit."
                .into(),
        }
    }
}

fn requirement_tag(req_id: &str) -> String {
    format!("@{}", req_id.trim_start_matches('@'))
}

/// The Gherkin parser silently treats keyword-less lines as description,
/// so guard steps explicitly instead of relying on the re-parse gate.
fn check_steps(steps: &[String]) -> Result<(), ServiceError> {
    const KEYWORDS: [&str; 5] = ["Given ", "When ", "Then ", "And ", "But "];
    for step in steps {
        if !KEYWORDS.iter().any(|k| step.starts_with(k)) {
            return Err(ServiceError(format!(
                "step \"{step}\" must start with Given, When, Then, And, or But"
            )));
        }
    }
    Ok(())
}

fn no_such_scenario(path: &str, name: &str) -> ServiceError {
    ServiceError(format!(
        "{path}: no scenario named \"{name}\". Call feature show to see its scenarios."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{InMemoryChangeStore, InMemoryFeatureCatalog};

    const PATH: &str = "features/calc.feature";
    const EXISTING: &str = "Feature: Calc\n\n  @REQ-001\n  Scenario: Empty string\n    Given a calculator\n    When add is called with \"\"\n    Then the result is 0\n";

    fn service_with_file() -> ScenarioService<InMemoryChangeStore, InMemoryFeatureCatalog> {
        let mut catalog = InMemoryFeatureCatalog::default();
        catalog.files.insert(PATH.into(), EXISTING.into());
        ScenarioService::new(InMemoryChangeStore::default(), catalog)
    }

    fn staged(
        service: &ScenarioService<InMemoryChangeStore, InMemoryFeatureCatalog>,
    ) -> FeatureDoc {
        let content = service.store.content(PATH).unwrap().expect("staged");
        feature::parse(PATH, &content).unwrap()
    }

    #[test]
    fn create_feature_stages_a_bare_feature_file() {
        let service = ScenarioService::new(
            InMemoryChangeStore::default(),
            InMemoryFeatureCatalog::default(),
        );
        let report = service.create_feature(PATH, "Calc").unwrap();
        assert_eq!(report.action, "create");
        assert_eq!(report.scenario, None);
        assert!(report.staged);
        assert_eq!(
            service.store.content(PATH).unwrap().as_deref(),
            Some("Feature: Calc\n")
        );
    }

    #[test]
    fn create_feature_refuses_an_existing_path() {
        let service = service_with_file();
        let error = service.create_feature(PATH, "Calc").unwrap_err();
        assert_eq!(
            error.0,
            "features/calc.feature already exists - add scenarios to it with scenario add."
        );
    }

    #[test]
    fn add_scenario_appends_a_tagged_scenario() {
        let service = service_with_file();
        let report = service
            .add_scenario(
                PATH,
                "REQ-002",
                "Single number",
                vec![
                    "Given a calculator".into(),
                    "When add is called with \"7\"".into(),
                    "Then the result is 7".into(),
                ],
            )
            .unwrap();
        assert_eq!(report.scenario.as_deref(), Some("Single number"));
        let doc = staged(&service);
        assert_eq!(doc.scenarios.len(), 2);
        assert_eq!(doc.scenarios[1].tags, vec!["@REQ-002"]);
        assert_eq!(
            service.store.summaries()[0],
            "add scenario \"Single number\" for REQ-002"
        );
    }

    #[test]
    fn a_leading_at_sign_on_the_requirement_id_is_tolerated() {
        let service = service_with_file();
        service
            .add_scenario(PATH, "@REQ-002", "S", vec!["Given a".into()])
            .unwrap();
        assert_eq!(staged(&service).scenarios[1].tags, vec!["@REQ-002"]);
    }

    #[test]
    fn add_scenario_refuses_a_duplicate_name() {
        let service = service_with_file();
        let error = service
            .add_scenario(PATH, "REQ-001", "Empty string", vec!["Given a".into()])
            .unwrap_err();
        assert_eq!(
            error.0,
            "features/calc.feature: scenario \"Empty string\" already exists - \
             change it with scenario update."
        );
    }

    #[test]
    fn add_scenario_to_a_missing_feature_names_the_recovery_command() {
        let service = ScenarioService::new(
            InMemoryChangeStore::default(),
            InMemoryFeatureCatalog::default(),
        );
        let error = service
            .add_scenario(PATH, "REQ-001", "S", vec!["Given a".into()])
            .unwrap_err();
        assert_eq!(
            error.0,
            "features/calc.feature: no such feature file. Create it first with \
             feature create."
        );
    }

    #[test]
    fn a_step_without_a_gherkin_keyword_is_refused_before_staging() {
        let service = service_with_file();
        let error = service
            .add_scenario(PATH, "REQ-002", "S", vec!["the result is 0".into()])
            .unwrap_err();
        assert_eq!(
            error.0,
            "step \"the result is 0\" must start with Given, When, Then, And, or But"
        );
        assert_eq!(service.store.content(PATH).unwrap(), None);
        let update = service
            .update_scenario(PATH, "Empty string", vec!["nope".into()], None)
            .unwrap_err();
        assert!(update.0.starts_with("step \"nope\" must start with"));
    }

    #[test]
    fn update_scenario_replaces_steps_and_optionally_the_tag() {
        let service = service_with_file();
        service
            .update_scenario(
                PATH,
                "Empty string",
                vec!["Given a calculator".into(), "Then the result is 0".into()],
                Some("REQ-009"),
            )
            .unwrap();
        let doc = staged(&service);
        assert_eq!(doc.scenarios[0].steps.len(), 2);
        assert_eq!(doc.scenarios[0].tags, vec!["@REQ-009"]);
    }

    #[test]
    fn update_scenario_with_no_new_steps_keeps_the_old_ones() {
        let service = service_with_file();
        service
            .update_scenario(PATH, "Empty string", vec![], Some("REQ-009"))
            .unwrap();
        let doc = staged(&service);
        assert_eq!(doc.scenarios[0].steps.len(), 3);
        assert_eq!(doc.scenarios[0].tags, vec!["@REQ-009"]);
    }

    #[test]
    fn update_of_an_unknown_scenario_names_the_recovery_command() {
        let service = service_with_file();
        let error = service
            .update_scenario(PATH, "Nope", vec![], None)
            .unwrap_err();
        assert_eq!(
            error.0,
            "features/calc.feature: no scenario named \"Nope\". Call feature show \
             to see its scenarios."
        );
    }

    #[test]
    fn delete_scenario_removes_it() {
        let service = service_with_file();
        let report = service.delete_scenario(PATH, "Empty string").unwrap();
        assert_eq!(report.action, "delete");
        assert!(staged(&service).scenarios.is_empty());
    }

    #[test]
    fn delete_of_an_unknown_scenario_names_the_recovery_command() {
        let service = service_with_file();
        let error = service.delete_scenario(PATH, "Nope").unwrap_err();
        assert!(error.0.contains("no scenario named \"Nope\""));
    }

    #[test]
    fn mutations_build_on_staged_content_not_the_working_tree() {
        let service = service_with_file();
        service
            .add_scenario(PATH, "REQ-002", "Second", vec!["Given a".into()])
            .unwrap();
        service
            .add_scenario(PATH, "REQ-003", "Third", vec!["Given a".into()])
            .unwrap();
        assert_eq!(staged(&service).scenarios.len(), 3);
    }

    #[test]
    fn corrupt_staged_content_is_a_structured_error() {
        let service = service_with_file();
        service.store.stage(PATH, "not gherkin", "oops").unwrap();
        let error = service
            .add_scenario(PATH, "REQ-002", "S", vec!["Given a".into()])
            .unwrap_err();
        assert!(
            error
                .0
                .starts_with("features/calc.feature: not valid Gherkin -"),
            "got: {}",
            error.0
        );
    }
}
