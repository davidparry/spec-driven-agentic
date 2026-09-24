//! LLM model resolution: flag > configuration > discovery, exactly the
//! order the plan documents. The provider (Ollama by default) is behind
//! the [`ModelCatalog`] port; the persisted choice behind [`ModelStore`].

use crate::domain::RECOMMENDED_MODEL;
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
    /// The provider is not reachable at all.
    ProviderDown(String),
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
            Ok(mut models) => {
                let source = if models.len() == 1 {
                    ModelSource::OnlyInstalled
                } else {
                    ModelSource::FirstInstalled
                };
                SessionModel::Ready {
                    model: models.remove(0).name,
                    source,
                }
            }
        }
    }

    pub fn resolve(&self, flag: Option<&str>) -> ModelResolution {
        match self.session_model(flag) {
            SessionModel::Ready { model, source } => ModelResolution::Resolved { model, source },
            SessionModel::NoModels => ModelResolution::Unavailable(format!(
                "llm_unavailable: no models installed - pull one first \
                 (e.g. `ollama pull {RECOMMENDED_MODEL}`)"
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
        self.store.persist(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeCatalog(Result<Vec<ModelInfo>, LlmError>);

    impl ModelCatalog for FakeCatalog {
        fn models(&self) -> Result<Vec<ModelInfo>, LlmError> {
            self.0.clone()
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
}
