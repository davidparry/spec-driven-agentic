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

use crate::domain::decision::{COMPLETION_CAPABILITY, DECISION_CAPABILITY};
use crate::domain::{RECOMMENDED_DECISION_MODEL, RECOMMENDED_MODEL};
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

/// Nothing was named and the provider could not be asked which of its
/// models could answer, so discovery had nothing to go on. Judgments
/// are off for want of a reachable provider rather than a decision.
const NO_PROVIDER_TO_DISCOVER_FROM: &str = "No decision model configured and none could be discovered - judgments are off \
     this session. Name one to settle it:\n    spec judge use <model-name>";

/// The typed decision status a session announces at startup, beside
/// [`SessionModel`]: which model will answer judgments, or why none
/// will.
#[derive(Debug, PartialEq, Eq)]
pub enum SessionDecision {
    /// The model this session will put judgments to, and where the name
    /// came from.
    Ready { model: String, source: ModelSource },
    /// Nothing will answer judgments this session, and what to do about
    /// it. Not an error: judgments are advice, so every command still
    /// returns the deterministic answer it always did.
    Off { remedy: String },
}

impl SessionDecision {
    /// The model judgments will be put to, or `None` when none will be
    /// - shaped to hand straight to [`ModelService::decision_readiness`].
    pub fn model(&self) -> Option<&str> {
        match self {
            Self::Ready { model, .. } => Some(model),
            Self::Off { .. } => None,
        }
    }
}

/// Whether this machine can answer a decision, and what to do when it
/// cannot. Every variant but [`Self::Ready`] names one next command.
#[derive(Debug, PartialEq, Eq)]
pub enum DecisionReadiness {
    /// A decision-capable model is installed and chosen.
    Ready,
    /// The provider answered and reports no decision-capable model.
    NoneInstalled,
    /// Decision models are installed; this project picked none.
    NoneConfigured { installed: Vec<String> },
    /// The chosen model is not one the provider can decide with -
    /// never pulled, or pulled and not decision-capable.
    ConfiguredMissing {
        model: String,
        installed: Vec<String>,
    },
    /// The provider could not be asked, so nothing is known.
    ///
    /// Deliberately not a refusal, on the same reasoning as
    /// [`generative_candidate`]: a capability probe that fails is not
    /// evidence a model is absent. The decision call that follows
    /// reports the real failure through its own typed error, which
    /// names the endpoint rather than blaming the model.
    Unknown,
}

impl DecisionReadiness {
    /// What to do about it, or `None` when there is nothing to do.
    ///
    /// The wording lives with the state rather than at the command that
    /// prints it, so every surface reaching a state says the same
    /// steps, and so a test can read them. Each remedy names a command
    /// that can be run as written.
    pub fn remedy(&self) -> Option<String> {
        self.steps("to continue")
    }

    /// The same steps worded for a session announcing its state rather
    /// than a command refusing to run. Startup blocks on none of this -
    /// a session with nothing to ask still returns every deterministic
    /// answer - and "to continue" would describe a halt that is not
    /// happening.
    pub fn announcement(&self) -> Option<String> {
        self.steps("to turn judgments on")
    }

    /// `purpose` closes the sentence that names the situation, so the
    /// two surfaces differ in exactly the clause that differs between
    /// them and nowhere else.
    fn steps(&self, purpose: &str) -> Option<String> {
        match self {
            // Nothing to fix, and nothing known to be broken.
            Self::Ready | Self::Unknown => None,
            Self::NoneInstalled => Some(format!(
                "No decision model is installed - install one {purpose}:\n    \
                 ollama pull {RECOMMENDED_DECISION_MODEL}\n    \
                 spec judge use {RECOMMENDED_DECISION_MODEL}"
            )),
            Self::NoneConfigured { installed } => Some(format!(
                "No decision model configured - choose one {purpose}:\n    \
                 spec judge use {}\ninstalled: {}",
                installed[0],
                installed.join(", ")
            )),
            Self::ConfiguredMissing { model, installed } => Some(format!(
                "Decision model '{model}' cannot answer decisions here - install it \
                 {purpose}:\n    ollama pull {model}\nor choose one already installed:\n    \
                 spec judge use {}\ninstalled: {}",
                installed[0],
                installed.join(", ")
            )),
        }
    }
}

/// `nimble` and `nimble:latest` name one model.
///
/// The provider lists the tagged form; a person configures whichever
/// they typed, and `spec judge use nimble` is a reasonable thing to
/// type. Comparing the strings alone would report a model as missing
/// while it sat in the list directly above the error.
fn same_model(configured: &str, installed: &str) -> bool {
    let tagged = |long: &str, short: &str| {
        long.strip_prefix(short)
            .is_some_and(|rest| rest.starts_with(':'))
    };
    configured == installed || tagged(installed, configured) || tagged(configured, installed)
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

    /// Whether this machine can answer a decision right now.
    ///
    /// Only `spec judge` asks. The automatic judgments inside `refine`
    /// deliberately do not: a project with no decision model configured
    /// behaves exactly as it did before the decision plane existed, and
    /// turning that into an error would make an opt-in feature
    /// mandatory. A human who typed `spec judge` has asked for one
    /// judgment, so an actionable refusal beats a late failure.
    pub fn decision_readiness(&self, configured: Option<&str>) -> DecisionReadiness {
        let Ok(installed) = self.decision_models() else {
            return DecisionReadiness::Unknown;
        };
        if installed.is_empty() {
            return DecisionReadiness::NoneInstalled;
        }
        match configured {
            None => DecisionReadiness::NoneConfigured { installed },
            Some(model) if installed.iter().any(|name| same_model(model, name)) => {
                DecisionReadiness::Ready
            }
            Some(model) => DecisionReadiness::ConfiguredMissing {
                model: model.to_string(),
                installed,
            },
        }
    }

    /// The model judgments are put to: the configured name, or the
    /// first decision-capable model installed when none is configured.
    ///
    /// A named model is honoured as given, without a capability probe,
    /// exactly as [`Self::session_model`] honours a configured
    /// generative name. An explicit instruction is not second-guessed,
    /// and a model that turns out not to be there reports the real
    /// failure against the endpoint - far better than judgments quietly
    /// doing nothing on a project that asked for them by name.
    ///
    /// [`Self::session_decision`] is the surface that does probe, so
    /// the problem is still said out loud at startup.
    pub fn decision_model(&self, configured: Option<&str>) -> Option<String> {
        match configured {
            Some(model) => Some(model.to_string()),
            None => self.decision_models().ok()?.into_iter().next(),
        }
    }

    /// The decision role this session will use, beside
    /// [`Self::session_model`] for the generative one: the configured
    /// name, or - when none is configured - the first model the
    /// provider reports as decision-capable.
    ///
    /// `source` describes where `configured` was read from, which only
    /// the caller knows: the `[decision]` block and the
    /// `--decision-model` flag are resolved together before this is
    /// asked. Discovery supplies its own source and ignores it.
    ///
    /// Discovery is the same bargain [`Self::session_model`] strikes
    /// for the generative role: borrow a model for the session, persist
    /// nothing, and leave `spec judge use` as the only thing that makes
    /// a choice stick. It is what makes judgments work on a machine
    /// that has pulled a decision model without a second configuration
    /// step. No model name is compiled in - the provider is asked which
    /// of its models can decide, so one released tomorrow is used the
    /// day it is pulled.
    ///
    /// A named model that [`Self::decision_readiness`] has nothing to
    /// say against is used, including when the provider could not be
    /// asked at all - on the same reasoning that readiness refuses
    /// nothing on a [`DecisionReadiness::Unknown`]: a capability probe
    /// that fails is not evidence a model is absent, and the first
    /// judgment reports the real failure against the endpoint.
    pub fn session_decision(
        &self,
        configured: Option<&str>,
        source: ModelSource,
    ) -> SessionDecision {
        match self.decision_readiness(configured) {
            DecisionReadiness::Ready => SessionDecision::Ready {
                model: configured
                    .expect("readiness is only Ready for a model that was named")
                    .to_string(),
                source,
            },
            // Nothing named, and the provider has models that can
            // answer. Borrow the first, for this session only.
            DecisionReadiness::NoneConfigured { mut installed } => SessionDecision::Ready {
                source: if installed.len() == 1 {
                    ModelSource::OnlyInstalled
                } else {
                    ModelSource::FirstInstalled
                },
                model: installed.remove(0),
            },
            DecisionReadiness::Unknown => match configured {
                Some(model) => SessionDecision::Ready {
                    model: model.to_string(),
                    source,
                },
                None => SessionDecision::Off {
                    remedy: NO_PROVIDER_TO_DISCOVER_FROM.to_string(),
                },
            },
            off => SessionDecision::Off {
                remedy: off
                    .announcement()
                    .expect("every remaining state names steps"),
            },
        }
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
    fn a_machine_with_a_decision_model_installed_and_chosen_is_ready() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("coder:latest", &["completion"]),
                ("nimble:latest", &["decision"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_readiness(Some("nimble:latest")),
            DecisionReadiness::Ready
        );
    }

    /// The whole point of the preflight: a machine that has pulled a
    /// coding model and nothing else is told what to install, not told
    /// to pick from an empty list.
    #[test]
    fn no_decision_capable_model_installed_is_its_own_state() {
        let service = ModelService::new(
            CapableCatalog::new(&[("coder:latest", &["completion", "tools"])]),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_readiness(None),
            DecisionReadiness::NoneInstalled
        );
        assert_eq!(
            service.decision_readiness(Some("nimble:latest")),
            DecisionReadiness::NoneInstalled,
            "nothing can answer, whatever was configured"
        );
    }

    #[test]
    fn decision_models_installed_but_none_chosen_names_the_ones_to_choose_from() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("nimble:latest", &["decision"]),
                ("other:latest", &["decision"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_readiness(None),
            DecisionReadiness::NoneConfigured {
                installed: vec!["nimble:latest".into(), "other:latest".into()]
            }
        );
    }

    #[test]
    fn a_chosen_model_that_cannot_decide_names_itself_and_the_alternatives() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("nimble:latest", &["decision"]),
                ("coder:latest", &["completion"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_readiness(Some("coder:latest")),
            DecisionReadiness::ConfiguredMissing {
                model: "coder:latest".into(),
                installed: vec!["nimble:latest".into()]
            }
        );
    }

    /// `spec judge use nimble` is a reasonable thing to type, and the
    /// provider lists `nimble:latest`. Comparing the strings alone
    /// would report the model missing while it sat in the list printed
    /// directly below the error.
    #[test]
    fn an_untagged_name_matches_the_tag_the_provider_lists() {
        let service = ModelService::new(
            CapableCatalog::new(&[("nimble:latest", &["decision"])]),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_readiness(Some("nimble")),
            DecisionReadiness::Ready
        );
        assert_eq!(
            service.decision_readiness(Some("nimble-mini")),
            DecisionReadiness::ConfiguredMissing {
                model: "nimble-mini".into(),
                installed: vec!["nimble:latest".into()]
            },
            "a prefix is not a tag"
        );
    }

    /// A probe that could not run is not evidence a model is absent, so
    /// the preflight stands aside and lets the decision call report the
    /// real failure against the endpoint.
    #[test]
    fn an_unreachable_provider_refuses_nothing() {
        let service = ModelService::new(
            FakeCatalog(Err(LlmError("down".into()))),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_readiness(Some("nimble:latest")),
            DecisionReadiness::Unknown
        );
        assert_eq!(service.decision_readiness(None), DecisionReadiness::Unknown);
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

    /// The line a session prints beside the inference one. The source
    /// is the caller's to supply: by the time this is asked, the flag
    /// and the `[decision]` block have already been resolved together.
    #[test]
    fn a_configured_decision_model_is_announced_with_where_it_came_from() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("coder:latest", &["completion"]),
                ("nimble:latest", &["decision"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.session_decision(Some("nimble:latest"), ModelSource::Config),
            SessionDecision::Ready {
                model: "nimble:latest".into(),
                source: ModelSource::Config,
            }
        );
        assert_eq!(
            service.session_decision(Some("nimble:latest"), ModelSource::Flag),
            SessionDecision::Ready {
                model: "nimble:latest".into(),
                source: ModelSource::Flag,
            }
        );
    }

    /// Judgments are on by default: a machine that pulled a decision
    /// model gets one without a second configuration step, borrowed for
    /// the session and never written down.
    #[test]
    fn with_nothing_configured_the_only_installed_decision_model_is_borrowed() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("coder:latest", &["completion", "tools"]),
                ("nimble:latest", &["decision"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.session_decision(None, ModelSource::Config),
            SessionDecision::Ready {
                model: "nimble:latest".into(),
                source: ModelSource::OnlyInstalled,
            },
            "the one model that can decide, whatever else is installed"
        );
        assert_eq!(
            *service.store.persisted.borrow(),
            None,
            "discovery must never persist a choice"
        );
    }

    /// Provider order, and the source says it was a borrow rather than
    /// the only candidate - the same distinction the generative role
    /// draws, so `spec judge current` can say which happened.
    #[test]
    fn with_several_decision_models_the_first_is_the_session_default() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("nimble:latest", &["decision"]),
                ("other:latest", &["decision"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.session_decision(None, ModelSource::Config),
            SessionDecision::Ready {
                model: "nimble:latest".into(),
                source: ModelSource::FirstInstalled,
            }
        );
    }

    /// Nothing installed that can decide is the one state discovery
    /// cannot rescue, so the session says what to pull.
    #[test]
    fn with_no_decision_capable_model_installed_the_session_says_what_to_pull() {
        let service = ModelService::new(
            CapableCatalog::new(&[("coder:latest", &["completion", "tools"])]),
            FakeStore::default(),
        );
        let SessionDecision::Off { remedy } = service.session_decision(None, ModelSource::Config)
        else {
            panic!("nothing installed can decide, so nothing can be borrowed");
        };
        assert!(remedy.contains("ollama pull"), "{remedy}");
    }

    /// The service honours a named model without a probe; the
    /// announcement probes and says so. Both are deliberate, and a test
    /// that reads only one of them would miss the pairing.
    #[test]
    fn a_named_decision_model_is_used_as_given_even_when_the_announcement_objects() {
        let service = ModelService::new(
            CapableCatalog::new(&[("nimble:latest", &["decision"])]),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_model(Some("never-pulled")),
            Some("never-pulled".to_string()),
            "an explicit instruction is not second-guessed"
        );
        assert!(
            matches!(
                service.session_decision(Some("never-pulled"), ModelSource::Config),
                SessionDecision::Off { .. }
            ),
            "and the startup line still says it cannot answer"
        );
    }

    #[test]
    fn with_nothing_named_the_service_takes_the_discovered_model() {
        let service = ModelService::new(
            CapableCatalog::new(&[
                ("coder:latest", &["completion"]),
                ("nimble:latest", &["decision"]),
            ]),
            FakeStore::default(),
        );
        assert_eq!(
            service.decision_model(None),
            Some("nimble:latest".to_string())
        );
    }

    /// The gap this announcement closes: a decision model named in
    /// configuration but never pulled used to go unnoticed until the
    /// first judgment was asked for.
    #[test]
    fn a_configured_decision_model_that_cannot_answer_is_off_at_startup() {
        let service = ModelService::new(
            CapableCatalog::new(&[("nimble:latest", &["decision"])]),
            FakeStore::default(),
        );
        let SessionDecision::Off { remedy } =
            service.session_decision(Some("never-pulled"), ModelSource::Config)
        else {
            panic!("a model the provider cannot decide with is not ready");
        };
        assert!(remedy.contains("ollama pull never-pulled"), "{remedy}");
    }

    /// Announcing must not become the one place that turns an
    /// unreachable provider into a verdict on a model. Whatever is
    /// configured is announced, and the first judgment reports the real
    /// failure against the endpoint.
    #[test]
    fn an_unreachable_provider_still_announces_the_configured_decision_model() {
        let service = ModelService::new(
            FakeCatalog(Err(LlmError("connection refused".into()))),
            FakeStore::default(),
        );
        assert_eq!(
            service.session_decision(Some("nimble:latest"), ModelSource::Config),
            SessionDecision::Ready {
                model: "nimble:latest".into(),
                source: ModelSource::Config,
            }
        );
        let SessionDecision::Off { remedy } = service.session_decision(None, ModelSource::Config)
        else {
            panic!("discovery cannot borrow a model from a provider that will not answer");
        };
        assert!(
            remedy.contains("spec judge use"),
            "nothing to list, so the advice is to name one: {remedy}"
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
