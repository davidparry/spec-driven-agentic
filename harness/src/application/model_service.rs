//! LLM model resolution: flag > configuration > discovery, exactly the
//! order the plan documents. The provider (Ollama by default) is behind
//! the [`ModelCatalog`] port; the persisted choice behind [`ModelStore`].
//!
//! Resolution is also where the two model roles are kept apart.
//! Ollama lists a decision model next to the coding models, and the
//! session default used to be whichever one `/api/tags` returned first —
//! so pulling a decision model could silently become the model every
//! generative command used. Discovery now asks what a model can do
//! before offering it for work it cannot perform.

use crate::domain::RECOMMENDED_MODEL;
use crate::domain::decision::{COMPLETION_CAPABILITY, DECISION_CAPABILITY};
use crate::ports::{LlmError, ModelCatalog, ModelInfo, ModelStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    Flag,
    Config,
    OnlyInstalled,
    /// Several models are installed and none is configured: the first
    /// one serves as the session default. Nothing is persisted until
    /// the user explicitly picks a model with `spec model use`.
    FirstInstalled,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ModelResolution {
    /// A single model was determined without asking the user. Discovery
    /// sources (`OnlyInstalled`, `FirstInstalled`) are session-only and
    /// never written to configuration.
    Resolved { model: String, source: ModelSource },
    /// The provider is unreachable or has no models. Generation is
    /// unavailable; everything non-generative keeps working.
    Unavailable(String),
}

/// The typed model status a session announces at startup: what it will
/// use, or exactly what is missing (the provider itself, or a model).
#[derive(Debug, PartialEq, Eq)]
pub enum SessionModel {
    /// The model this session will use and where it came from.
    Ready { model: String, source: ModelSource },
    /// The provider answered but has no models pulled yet.
    NoModels,
    /// Models are installed, but every one of them is a decision model.
    /// A distinct state from [`Self::NoModels`] because the fix is
    /// different and "no models installed" would be a lie — this is the
    /// machine of someone who pulled `nimble` and nothing else.
    NoGenerativeModel { installed: Vec<String> },
    /// The provider is not reachable at all.
    ProviderDown(String),
}

/// Whether a model may be handed to generative work.
///
/// The question is asked in the negative on purpose: only a model the
/// provider *positively* reports as decision-capable and not
/// completion-capable is withheld. A provider that cannot answer, or a
/// model it has no capability list for, keeps the behaviour it has
/// always had — a capability probe that fails is not evidence to start
/// excluding models on.
fn generative_candidate<C: ModelCatalog + ?Sized>(catalog: &C, model: &str) -> bool {
    match catalog.capabilities(model) {
        None => true,
        Some(capabilities) => {
            let decides = capabilities.iter().any(|c| c == DECISION_CAPABILITY);
            let completes = capabilities.iter().any(|c| c == COMPLETION_CAPABILITY);
            !decides || completes
        }
    }
}

pub struct ModelService<C: ModelCatalog, S: ModelStore> {
    catalog: C,
    store: S,
}

impl<C: ModelCatalog, S: ModelStore> ModelService<C, S> {
    pub fn new(catalog: C, store: S) -> Self {
        Self { catalog, store }
    }

    /// The one resolution order - flag > configuration > discovery -
    /// as the typed status a session announces at startup. Discovery
    /// never persists anything.
    pub fn session_model(&self, flag: Option<&str>) -> SessionModel {
        if let Some(model) = flag {
            return SessionModel::Ready {
                model: model.to_string(),
                source: ModelSource::Flag,
            };
        }
        if let Some(model) = self.store.configured() {
            return SessionModel::Ready {
                model,
                source: ModelSource::Config,
            };
        }
        match self.catalog.models() {
            Err(e) => SessionModel::ProviderDown(e.0),
            Ok(models) if models.is_empty() => SessionModel::NoModels,
            Ok(models) => {
                // Only the discovery path pays for capability probes. A
                // flag or a configured name returned above without one,
                // so the common case costs exactly what it used to.
                let mut usable: Vec<String> = models
                    .iter()
                    .filter(|model| generative_candidate(&self.catalog, &model.name))
                    .map(|model| model.name.clone())
                    .collect();
                if usable.is_empty() {
                    return SessionModel::NoGenerativeModel {
                        installed: models.into_iter().map(|model| model.name).collect(),
                    };
                }
                let source = if usable.len() == 1 {
                    ModelSource::OnlyInstalled
                } else {
                    ModelSource::FirstInstalled
                };
                SessionModel::Ready {
                    model: usable.remove(0),
                    source,
                }
            }
        }
    }

    /// The installed models that can answer decisions, in the order the
    /// provider lists them.
    ///
    /// Capability-driven, so a decision model released tomorrow is
    /// offered the day it is pulled and no model name is compiled into
    /// the harness.
    pub fn decision_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(self
            .catalog
            .models()?
            .into_iter()
            .filter(|model| {
                self.catalog
                    .capabilities(&model.name)
                    .is_some_and(|capabilities| {
                        capabilities.iter().any(|c| c == DECISION_CAPABILITY)
                    })
            })
            .map(|model| model.name)
            .collect())
    }

    pub fn resolve(&self, flag: Option<&str>) -> ModelResolution {
        match self.session_model(flag) {
            SessionModel::Ready { model, source } => ModelResolution::Resolved { model, source },
            SessionModel::NoModels => ModelResolution::Unavailable(format!(
                "llm_unavailable: no models installed - pull one first \
                 (e.g. `ollama pull {RECOMMENDED_MODEL}`)"
            )),
            SessionModel::NoGenerativeModel { installed } => ModelResolution::Unavailable(format!(
                "llm_unavailable: {} answers decisions, not chat - pull a coding model \
                 too (e.g. `ollama pull {RECOMMENDED_MODEL}`); keep the decision model \
                 for `spec judge`",
                installed.join(", ")
            )),
            SessionModel::ProviderDown(e) => ModelResolution::Unavailable(format!(
                "llm_unavailable: cannot reach the model provider - {e}"
            )),
        }
    }

    pub fn list(&self) -> Result<Vec<ModelInfo>, LlmError> {
        self.catalog.models()
    }

    /// Persist a model choice. When the provider is reachable the name
    /// must be one of the installed models; when it is not reachable the
    /// choice is persisted anyway (validated on next use).
    ///
    /// A decision model is refused outright. `llm.model` is the model
    /// every generative command uses, and pointing it at a model that
    /// cannot hold a conversation breaks drafting, implementing, and
    /// refactoring in one line of configuration — with no error, because
    /// a decision model answers `/api/chat` with prose rather than
    /// refusing. The decision role has its own key.
    pub fn choose(&self, model: &str) -> Result<(), LlmError> {
        if let Ok(models) = self.catalog.models()
            && !models.iter().any(|m| m.name == model)
        {
            let available: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
            return Err(LlmError(format!(
                "model '{model}' is not installed - available: {}",
                available.join(", ")
            )));
        }
        if !generative_candidate(&self.catalog, model) {
            return Err(LlmError(format!(
                "'{model}' is a decision model and cannot do generative work - \
                 set it as the decision model instead: spec judge use {model}"
            )));
        }
        self.store.persist(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Reports no capabilities at all, like a provider too old to be
    /// asked. Every pre-existing test below uses it, so they double as
    /// the proof that an unknown capability changes nothing.
    struct FakeCatalog(Result<Vec<ModelInfo>, LlmError>);

    impl ModelCatalog for FakeCatalog {
        fn models(&self) -> Result<Vec<ModelInfo>, LlmError> {
            self.0.clone()
        }
    }

    /// A provider that answers capability questions, for the models it
    /// has been told about.
    struct CapableCatalog {
        models: Vec<ModelInfo>,
        capabilities: std::collections::HashMap<String, Vec<String>>,
    }

    impl CapableCatalog {
        /// `(name, capabilities)` pairs; an empty slice means the
        /// provider has no capability list for that model.
        fn new(entries: &[(&str, &[&str])]) -> Self {
            Self {
                models: entries.iter().map(|(name, _)| model(name)).collect(),
                capabilities: entries
                    .iter()
                    .filter(|(_, caps)| !caps.is_empty())
                    .map(|(name, caps)| {
                        (
                            (*name).to_string(),
                            caps.iter().map(|c| (*c).to_string()).collect(),
                        )
                    })
                    .collect(),
            }
        }
    }

    impl ModelCatalog for CapableCatalog {
        fn models(&self) -> Result<Vec<ModelInfo>, LlmError> {
            Ok(self.models.clone())
        }

        fn capabilities(&self, model: &str) -> Option<Vec<String>> {
            self.capabilities.get(model).cloned()
        }
    }

    #[derive(Default)]
    struct FakeStore {
        configured: Option<String>,
        persisted: RefCell<Option<String>>,
    }

    impl ModelStore for FakeStore {
        fn configured(&self) -> Option<String> {
            self.configured.clone()
        }
        fn persist(&self, model: &str) -> Result<(), LlmError> {
            *self.persisted.borrow_mut() = Some(model.to_string());
            Ok(())
        }
    }

    fn model(name: &str) -> ModelInfo {
        ModelInfo {
            name: name.into(),
            size_bytes: None,
            modified_at: None,
        }
    }

    #[test]
    fn the_flag_wins_over_everything() {
        let service = ModelService::new(
            FakeCatalog(Ok(vec![model("a"), model("b")])),
            FakeStore {
                configured: Some("configured".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            service.resolve(Some("flagged")),
            ModelResolution::Resolved {
                model: "flagged".into(),
                source: ModelSource::Flag
            }
        );
    }

    #[test]
    fn the_configured_model_wins_over_discovery() {
        let service = ModelService::new(
            FakeCatalog(Ok(vec![model("a"), model("b")])),
            FakeStore {
                configured: Some("configured".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            service.resolve(None),
            ModelResolution::Resolved {
                model: "configured".into(),
                source: ModelSource::Config
            }
        );
    }

    #[test]
    fn a_single_installed_model_is_used_automatically() {
        let service = ModelService::new(FakeCatalog(Ok(vec![model("only")])), FakeStore::default());
        assert_eq!(
            service.resolve(None),
            ModelResolution::Resolved {
                model: "only".into(),
                source: ModelSource::OnlyInstalled
            }
        );
        assert_eq!(*service.store.persisted.borrow(), None);
    }

    #[test]
    fn several_installed_models_fall_back_to_the_first_as_the_session_default() {
        let service = ModelService::new(
            FakeCatalog(Ok(vec![model("a"), model("b")])),
            FakeStore::default(),
        );
        assert_eq!(
            service.resolve(None),
            ModelResolution::Resolved {
                model: "a".into(),
                source: ModelSource::FirstInstalled
            }
        );
        assert_eq!(
            *service.store.persisted.borrow(),
            None,
            "discovery must never persist a choice"
        );
    }

    #[test]
    fn no_installed_models_is_unavailable_without_installing_anything() {
        let service = ModelService::new(FakeCatalog(Ok(vec![])), FakeStore::default());
        let resolution = service.resolve(None);
        assert!(
            matches!(&resolution, ModelResolution::Unavailable(m)
                if m.starts_with("llm_unavailable: no models installed")
                    && m.contains(crate::domain::RECOMMENDED_MODEL)),
            "got {resolution:?}"
        );
    }

    #[test]
    fn an_unreachable_provider_is_unavailable() {
        let service = ModelService::new(
            FakeCatalog(Err(LlmError("connection refused".into()))),
            FakeStore::default(),
        );
        let resolution = service.resolve(None);
        assert!(
            matches!(&resolution, ModelResolution::Unavailable(m)
                if m.contains("connection refused")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn the_session_status_is_no_models_when_the_catalog_is_empty() {
        let service = ModelService::new(FakeCatalog(Ok(vec![])), FakeStore::default());
        assert_eq!(service.session_model(None), SessionModel::NoModels);
    }

    #[test]
    fn the_session_status_carries_the_provider_error_when_it_is_down() {
        let service = ModelService::new(
            FakeCatalog(Err(LlmError("connection refused".into()))),
            FakeStore::default(),
        );
        assert_eq!(
            service.session_model(None),
            SessionModel::ProviderDown("connection refused".into())
        );
    }

    #[test]
    fn list_returns_whatever_the_catalog_reports() {
        let service = ModelService::new(FakeCatalog(Ok(vec![model("a")])), FakeStore::default());
        assert_eq!(service.list().unwrap(), vec![model("a")]);
    }

    #[test]
    fn choosing_an_installed_model_persists_it() {
        let store = FakeStore::default();
        let service = ModelService::new(FakeCatalog(Ok(vec![model("a")])), store);
        service.choose("a").unwrap();
        assert_eq!(*service.store.persisted.borrow(), Some("a".to_string()));
    }

    #[test]
    fn choosing_a_model_that_is_not_installed_is_rejected_with_the_alternatives() {
        let service = ModelService::new(
            FakeCatalog(Ok(vec![model("a"), model("b")])),
            FakeStore::default(),
        );
        assert_eq!(
            service.choose("zzz").unwrap_err(),
            LlmError("model 'zzz' is not installed - available: a, b".into())
        );
    }

    #[test]
    fn choosing_while_the_provider_is_down_persists_for_later_validation() {
        let store = FakeStore::default();
        let service = ModelService::new(FakeCatalog(Err(LlmError("down".into()))), store);
        service.choose("a").unwrap();
        assert_eq!(*service.store.persisted.borrow(), Some("a".to_string()));
    }

    /// The bug this guard exists for: `/api/tags` lists a pulled
    /// decision model alongside the coding models, and whichever came
    /// back first became the session default for every generative
    /// command.
    #[test]
    fn a_decision_model_listed_first_is_never_the_session_default() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("nimble:latest", &["decision"]),
                ("coder:latest", &["completion", "tools"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.resolve(None),
            ModelResolution::Resolved {
                model: "coder:latest".into(),
                source: ModelSource::OnlyInstalled,
            },
            "the decision model is not a generative candidate at all"
        );
    }

    #[test]
    fn a_model_that_both_decides_and_completes_stays_a_generative_candidate() {
        let service = ModelService::new(
            CapableCatalog::new(&[("hybrid:latest", &["decision", "completion"])]),
            FakeStore::default(),
        );
        assert_eq!(
            service.resolve(None),
            ModelResolution::Resolved {
                model: "hybrid:latest".into(),
                source: ModelSource::OnlyInstalled,
            }
        );
    }

    /// A capability probe that cannot answer is not grounds to start
    /// withholding models; the previous behaviour stands.
    #[test]
    fn a_model_with_no_capability_list_keeps_the_behaviour_it_always_had() {
        let service = ModelService::new(
            CapableCatalog::new(&[("mystery:latest", &[]), ("coder:latest", &["completion"])]),
            FakeStore::default(),
        );
        assert_eq!(
            service.resolve(None),
            ModelResolution::Resolved {
                model: "mystery:latest".into(),
                source: ModelSource::FirstInstalled,
            }
        );
    }

    /// The machine of someone whose first pull was a decision model.
    /// "No models installed" would be false and would send them to pull
    /// the model they already have.
    #[test]
    fn only_a_decision_model_installed_is_its_own_state_not_an_empty_provider() {
        let service = ModelService::new(
            CapableCatalog::new(&[("nimble:latest", &["decision"])]),
            FakeStore::default(),
        );
        assert_eq!(
            service.session_model(None),
            SessionModel::NoGenerativeModel {
                installed: vec!["nimble:latest".into()]
            }
        );
        let resolution = service.resolve(None);
        assert!(
            matches!(&resolution, ModelResolution::Unavailable(m)
                if m.contains("answers decisions, not chat")
                    && m.contains("spec judge")
                    && m.contains(crate::domain::RECOMMENDED_MODEL)),
            "got {resolution:?}"
        );
    }

    /// A configured or flagged name is used as given. Resolution must
    /// not start probing capabilities on the hot path, and must not
    /// second-guess an explicit instruction.
    #[test]
    fn a_configured_or_flagged_name_is_honoured_without_a_capability_probe() {
        let service = ModelService::new(
            CapableCatalog::new(&[("nimble:latest", &["decision"])]),
            FakeStore {
                configured: Some("nimble:latest".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            service.resolve(None),
            ModelResolution::Resolved {
                model: "nimble:latest".into(),
                source: ModelSource::Config,
            }
        );
        assert_eq!(
            service.resolve(Some("nimble:latest")),
            ModelResolution::Resolved {
                model: "nimble:latest".into(),
                source: ModelSource::Flag,
            }
        );
    }

    #[test]
    fn only_decision_capable_models_are_offered_for_the_decision_role() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("coder:latest", &["completion", "tools"]),
                ("nimble:latest", &["decision"]),
                ("mystery:latest", &[]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(service.decision_models().unwrap(), vec!["nimble:latest"]);
    }

    #[test]
    fn listing_decision_models_carries_a_provider_failure_up() {
        let service = ModelService::new(
            FakeCatalog(Err(LlmError("down".into()))),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_models().unwrap_err(),
            LlmError("down".into())
        );
    }

    /// `spec model use nimble` is a one-line way to break every
    /// generative command, and it would fail silently: a decision model
    /// answers `/api/chat` with prose instead of refusing.
    #[test]
    fn a_decision_model_is_refused_as_the_generative_model_and_points_at_the_other_key() {
        let service = ModelService::new(
            CapableCatalog::new(&[("nimble:latest", &["decision"])]),
            FakeStore::default(),
        );
        let error = service.choose("nimble:latest").unwrap_err();
        assert_eq!(
            error,
            LlmError(
                "'nimble:latest' is a decision model and cannot do generative work - \
                 set it as the decision model instead: spec judge use nimble:latest"
                    .into()
            )
        );
        assert_eq!(
            *service.store.persisted.borrow(),
            None,
            "nothing is written when the choice is refused"
        );
    }

    #[test]
    fn a_completion_model_is_still_choosable() {
        let service = ModelService::new(
            CapableCatalog::new(&[("coder:latest", &["completion"])]),
            FakeStore::default(),
        );
        service.choose("coder:latest").unwrap();
        assert_eq!(
            *service.store.persisted.borrow(),
            Some("coder:latest".to_string())
        );
    }
}
