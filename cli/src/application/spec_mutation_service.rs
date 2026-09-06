//! Spec mutations: interactive drafting (the human words the spec, the
//! validate → refine findings drive rewording until clean) and the
//! GREEN-gated `mark-implemented`. Every mutation lands in the staging
//! area, never directly in the working tree.

use serde::Serialize;

use crate::application::assets::{feature_tagged, load_effective_catalog};
use crate::application::spec_service::ServiceError;
use crate::application::{DEFAULT_LLM_ATTEMPTS, LlmReplyError, generate_valid};
use crate::domain::model::{Requirement, Spec, SpecCatalog};
use crate::domain::proposal::{
    ProposedRequirement, parse_proposals_checked, parse_rewording_checked, proposal_prompt,
    rewording_prompt,
};
use crate::domain::refiner::{RequirementRefiner, finding_signature, suggestion_for};
use crate::domain::spec_validator::SpecValidator;
use crate::domain::tdd::TddPhase;
use crate::ports::{
    ChangeStore, FeatureCatalog, FeatureFiles, LlmGenerator, Prompter, SpecRepository, StateStore,
};

/// A resolved model to call: name + generator.
type Model<'a> = (&'a str, &'a dyn LlmGenerator);

/// A resolved model available to assist drafting, when there is one.
type ModelAid<'a> = Option<Model<'a>>;

/// Where a drafted requirement lands: the resolved catalog and the
/// document inside it that receives the wording.
struct DraftTarget {
    catalog: SpecCatalog,
    file: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DraftReport {
    pub id: String,
    pub title: String,
    pub staged: bool,
    /// Wording findings that were still open when the requirement was
    /// staged, empty when the review came back clean.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// How the developer wants to continue once rewording stops making
/// progress.
enum StalledChoice {
    /// Ask the model for another rewording pass.
    Reword,
    /// Take over by hand for the remaining passes.
    Manual,
    /// The wording stands; stage it with its open findings.
    Accept,
}

/// The findings a wording earns, split by who owns them. `structural`
/// issues come from [`SpecValidator`] and must be fixed before staging -
/// a spec that fails them is not usable. `advisory` findings come from
/// [`RequirementRefiner`]: wording quality the developer may accept
/// as-is.
#[derive(Debug, Default)]
struct Findings {
    structural: Vec<String>,
    advisory: Vec<String>,
}

impl Findings {
    fn is_empty(&self) -> bool {
        self.structural.is_empty() && self.advisory.is_empty()
    }

    /// Every finding, structural ones first - they gate staging.
    fn all(&self) -> Vec<String> {
        let mut all = self.structural.clone();
        all.extend(self.advisory.iter().cloned());
        all
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SetFeatureReport {
    pub id: String,
    #[serde(rename = "featureFile")]
    pub feature_file: String,
    pub staged: bool,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ListedRequirement {
    pub id: String,
    pub title: String,
    pub status: String,
    /// The spec file declaring this requirement, relative to the
    /// project root.
    pub file: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub staged: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct IncludeReport {
    /// The included spec file, relative to the project root.
    pub file: String,
    /// The catalog document that lists the include.
    pub parent: String,
    /// Whether a fresh (empty) spec file was staged for the include.
    pub created: bool,
    pub staged: bool,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MarkReport {
    pub id: String,
    pub status: String,
    pub staged: bool,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct SpecMutationService<
    R: SpecRepository,
    F: FeatureFiles,
    G: FeatureCatalog,
    C: ChangeStore,
    S: StateStore,
> {
    repository: R,
    feature_files: F,
    catalog: G,
    store: C,
    state: S,
    spec_path: String,
    llm_attempts: u32,
    max_reword_passes: u32,
}

/// Rewording passes before the wizard stops looping on its own and asks
/// the developer how to continue.
pub const DEFAULT_MAX_REWORD_PASSES: u32 = 3;

impl<R: SpecRepository, F: FeatureFiles, G: FeatureCatalog, C: ChangeStore, S: StateStore>
    SpecMutationService<R, F, G, C, S>
{
    pub fn new(
        repository: R,
        feature_files: F,
        catalog: G,
        store: C,
        state: S,
        spec_path: String,
    ) -> Self {
        Self {
            repository,
            feature_files,
            catalog,
            store,
            state,
            spec_path,
            llm_attempts: DEFAULT_LLM_ATTEMPTS,
            max_reword_passes: DEFAULT_MAX_REWORD_PASSES,
        }
    }

    /// How many times a model reply is tried when validation fails.
    pub fn with_llm_attempts(mut self, attempts: u32) -> Self {
        self.llm_attempts = attempts.max(1);
        self
    }

    /// How many rewording passes the wizard takes before asking the
    /// developer whether to keep going, take over, or accept the wording.
    pub fn with_max_reword_passes(mut self, passes: u32) -> Self {
        self.max_reword_passes = passes.max(1);
        self
    }

    /// Draft a new requirement interactively. The human words the spec;
    /// the loop reruns validate + refine on every wording until there
    /// are no findings, then asks for approval before staging. On
    /// rewording passes every prompt carries the prior answer, which
    /// Enter keeps as-is.
    pub fn draft(&self, prompter: &mut dyn Prompter) -> Result<DraftReport, ServiceError> {
        self.draft_in(prompter, None)
    }

    /// [`Self::draft`] into a chosen catalog file instead of the root
    /// document. The file must already be part of the include tree.
    pub fn draft_in(
        &self,
        prompter: &mut dyn Prompter,
        file: Option<&str>,
    ) -> Result<DraftReport, ServiceError> {
        let catalog = self.effective_catalog()?;
        let file = self.target_file(&catalog, file)?;
        let id = next_id(&catalog.merged());
        self.manual_draft(prompter, DraftTarget { catalog, file }, id, None)
    }

    /// Draft with the model's help: the human describes what to build
    /// in plain words, the model splits the description into complete
    /// requirement proposals (title, story, criteria). The human
    /// accepts all of them, or a comma-separated subset; accepted
    /// proposals are stored in the spec as pending requirements under
    /// sequential ids. The human then picks which stored one the
    /// wizard walks through first, with every field pre-filled from
    /// the proposal - Enter accepts, typing replaces. Any model
    /// failure falls back to manual drafting.
    pub fn draft_assisted(
        &self,
        prompter: &mut dyn Prompter,
        model: &str,
        llm: &dyn LlmGenerator,
    ) -> Result<DraftReport, ServiceError> {
        self.draft_assisted_in(prompter, model, llm, None)
    }

    /// [`Self::draft_assisted`] into a chosen catalog file instead of
    /// the root document.
    pub fn draft_assisted_in(
        &self,
        prompter: &mut dyn Prompter,
        model: &str,
        llm: &dyn LlmGenerator,
        file: Option<&str>,
    ) -> Result<DraftReport, ServiceError> {
        let mut catalog = self.effective_catalog()?;
        let target = self.target_file(&catalog, file)?;
        let mut merged = catalog.merged();
        let id = next_id(&merged);
        let description = self.ask(
            prompter,
            "Describe what to build in plain words (one or several requirements). \
             Enter drafts manually instead:",
        )?;
        if description.is_empty() {
            return self.manual_draft(
                prompter,
                DraftTarget {
                    catalog,
                    file: target,
                },
                id,
                Some((model, llm)),
            );
        }
        let work = prompter.working(&format!(
            "Splitting the description into requirements with {model} - working"
        ));
        let prompt = proposal_prompt(&description);
        let outcome = generate_valid(
            llm,
            model,
            &prompt,
            self.llm_attempts,
            parse_proposals_checked,
            |attempt, of, reason| {
                prompter.warn(&format!(
                    "The model reply was invalid ({reason}) - asking again ({attempt} of {of})"
                ));
            },
        );
        drop(work);
        let proposals = match outcome {
            Ok(proposals) => proposals,
            Err(LlmReplyError::Call(error)) => {
                prompter.warn(&format!("The model call failed ({}).", error.0));
                Vec::new()
            }
            Err(LlmReplyError::Invalid { reason }) => {
                prompter.warn(&format!(
                    "The model did not return a valid requirement list ({reason})."
                ));
                Vec::new()
            }
        };
        if proposals.is_empty() {
            prompter.warn("The description gave no complete requirement - drafting manually.");
            return self.manual_draft(
                prompter,
                DraftTarget {
                    catalog,
                    file: target,
                },
                id,
                Some((model, llm)),
            );
        }
        if proposals.len() > 1 {
            prompter.tell(
                "Accept all these requirements to refine, or enter comma-separated \
                 numbers of the ones to accept.",
            );
        }
        prompter.tell(&format!(
            "The description holds {} requirement(s):",
            proposals.len()
        ));
        for (index, proposal) in proposals.iter().enumerate() {
            prompter.tell(&format!("  {}. {}", index + 1, proposal.title));
        }
        let accepted = self.accept_proposals(prompter, &proposals)?;
        let mut stored = Vec::with_capacity(accepted.len());
        for ProposedRequirement {
            title,
            story,
            acceptance_criteria,
        } in accepted
        {
            let requirement = Requirement {
                id: next_id(&merged),
                title,
                status: "pending".into(),
                story,
                acceptance_criteria,
                feature_file: None,
            };
            merged.requirements.push(requirement.clone());
            catalog
                .file_mut(&target)
                .expect("the target is part of the catalog")
                .requirements
                .push(requirement.clone());
            stored.push(requirement);
        }
        let ids: Vec<&str> = stored.iter().map(|r| r.id.as_str()).collect();
        self.stage_file(
            &catalog,
            &target,
            &format!("draft {} from the description", ids.join(", ")),
        )?;
        // Land in the working-tree spec file now, not after the refine
        // wizard. The human already accepted these rows; they should be
        // visible in requirements.json while the first one is reviewed.
        self.store.commit().map_err(|e| ServiceError(e.0))?;
        prompter.tell(&format!(
            "Accepted requirements are now stored in {} as pending:",
            self.project_path(&target)
        ));
        for requirement in &stored {
            prompter.tell(&format!("  {} {}", requirement.id, requirement.title));
        }
        let chosen = self.pick_proposal(prompter, stored.len())?;
        let proposal = stored.remove(chosen);
        let chosen_id = proposal.id.clone();
        prompter.tell(&format!(
            "Walking through {chosen_id}. Each prompt shows the proposal - Enter \
             accepts it, or type your own wording."
        ));
        self.draft_loop(
            prompter,
            DraftTarget {
                catalog,
                file: target,
            },
            chosen_id,
            Some(proposal),
            Some((model, llm)),
            true,
        )
    }

    /// Which of the listed proposals to keep. Enter means all of them;
    /// otherwise a comma-separated list of 1-based numbers. A single
    /// proposal is accepted without asking.
    fn accept_proposals(
        &self,
        prompter: &mut dyn Prompter,
        proposals: &[ProposedRequirement],
    ) -> Result<Vec<ProposedRequirement>, ServiceError> {
        if proposals.len() == 1 {
            return Ok(proposals.to_vec());
        }
        loop {
            let answer = self.ask(
                prompter,
                "Accept [Enter for all, or comma-separated numbers]:",
            )?;
            match parse_accept_selection(&answer, proposals.len()) {
                Ok(indices) => {
                    return Ok(indices.into_iter().map(|i| proposals[i].clone()).collect());
                }
                Err(warning) => prompter.warn(&warning),
            }
        }
    }

    /// Which stored requirement to review first. Enter means the first;
    /// anything else must be a number from the accepted list.
    fn pick_proposal(
        &self,
        prompter: &mut dyn Prompter,
        count: usize,
    ) -> Result<usize, ServiceError> {
        if count == 1 {
            return Ok(0);
        }
        loop {
            let answer = self.ask(
                prompter,
                &format!("Which requirement first to review and refine? [1-{count}, Enter for 1]:"),
            )?;
            if answer.is_empty() {
                return Ok(0);
            }
            match answer.parse::<usize>() {
                Ok(pick) if (1..=count).contains(&pick) => return Ok(pick - 1),
                _ => prompter.warn(&format!("Pick a number between 1 and {count}.")),
            }
        }
    }

    fn manual_draft(
        &self,
        prompter: &mut dyn Prompter,
        target: DraftTarget,
        id: String,
        llm: ModelAid,
    ) -> Result<DraftReport, ServiceError> {
        prompter.tell(&format!(
            "Drafting {id}. You word the spec; validate and refine findings drive \
             rewording until the wording is clean."
        ));
        self.draft_loop(prompter, target, id, None, llm, false)
    }

    /// Reword an existing requirement interactively (refine findings on
    /// a backlog item). Stages a replacement of that row.
    pub fn reword(
        &self,
        prompter: &mut dyn Prompter,
        id: &str,
    ) -> Result<DraftReport, ServiceError> {
        self.reword_with(prompter, id, None)
    }

    pub fn reword_assisted(
        &self,
        prompter: &mut dyn Prompter,
        id: &str,
        model: &str,
        llm: &dyn LlmGenerator,
    ) -> Result<DraftReport, ServiceError> {
        self.reword_with(prompter, id, Some((model, llm)))
    }

    fn reword_with(
        &self,
        prompter: &mut dyn Prompter,
        id: &str,
        llm: ModelAid,
    ) -> Result<DraftReport, ServiceError> {
        let catalog = self.effective_catalog()?;
        let existing = catalog
            .merged()
            .requirements
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| {
                ServiceError(format!(
                    "No requirement with id '{id}'. Call spec list to see valid ids."
                ))
            })?;
        let target = catalog
            .source_of(id)
            .expect("the id was found in the merged spec")
            .to_string();
        prompter.tell(&format!(
            "Rewording {id}. You word the spec; validate and refine findings drive \
             rewording until the wording is clean."
        ));
        self.draft_loop(
            prompter,
            DraftTarget {
                catalog,
                file: target,
            },
            id.to_string(),
            Some(existing),
            llm,
            true,
        )
    }

    /// Non-interactive draft from flags. Structural validate must pass;
    /// refine findings are reported in `nextStep` rather than blocking.
    pub fn draft_direct(
        &self,
        title: &str,
        story: &str,
        criteria: Vec<String>,
    ) -> Result<DraftReport, ServiceError> {
        self.draft_direct_in(title, story, criteria, None)
    }

    /// [`Self::draft_direct`] into a chosen catalog file instead of the
    /// root document.
    pub fn draft_direct_in(
        &self,
        title: &str,
        story: &str,
        criteria: Vec<String>,
        file: Option<&str>,
    ) -> Result<DraftReport, ServiceError> {
        let mut catalog = self.effective_catalog()?;
        let target = self.target_file(&catalog, file)?;
        let id = next_id(&catalog.merged());
        let candidate = Requirement {
            id: id.clone(),
            title: title.to_string(),
            status: "pending".into(),
            story: story.to_string(),
            acceptance_criteria: criteria,
            feature_file: None,
        };
        self.stage_direct(&mut catalog, &target, candidate, false)
    }

    /// Non-interactive reword of an existing requirement from flags.
    pub fn reword_direct(
        &self,
        id: &str,
        title: Option<String>,
        story: Option<String>,
        criteria: Vec<String>,
    ) -> Result<DraftReport, ServiceError> {
        let mut catalog = self.effective_catalog()?;
        let mut candidate = catalog
            .merged()
            .requirements
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| {
                ServiceError(format!(
                    "No requirement with id '{id}'. Call spec list to see valid ids."
                ))
            })?;
        if let Some(title) = title {
            candidate.title = title;
        }
        if let Some(story) = story {
            candidate.story = story;
        }
        if !criteria.is_empty() {
            candidate.acceptance_criteria = criteria;
        }
        let target = catalog
            .source_of(id)
            .expect("the id was found in the merged spec")
            .to_string();
        self.stage_direct(&mut catalog, &target, candidate, true)
    }

    fn stage_direct(
        &self,
        catalog: &mut SpecCatalog,
        target: &str,
        candidate: Requirement,
        replace: bool,
    ) -> Result<DraftReport, ServiceError> {
        let id = candidate.id.clone();
        let title = candidate.title.clone();
        let merged = catalog.merged();
        let findings = self.findings_for(&merged, &candidate).all();
        let structural: Vec<String> = findings
            .iter()
            .filter(|issue| {
                issue.contains("must be phrased")
                    || issue.contains("title is missing")
                    || issue.contains("user story is missing")
                    || issue.contains("at least one acceptance criterion")
                    || issue.contains("id must look like")
                    || issue.contains("duplicate id")
            })
            .cloned()
            .collect();
        if !structural.is_empty() {
            return Err(ServiceError(structural.join(" ")));
        }
        let warning = duplicate_warning(&merged, &candidate);
        let doc = catalog
            .file_mut(target)
            .expect("the target is part of the catalog");
        if replace {
            if let Some(slot) = doc.requirements.iter_mut().find(|r| r.id == id) {
                *slot = candidate;
            }
            self.stage_file(catalog, target, &format!("reword {id}"))?;
        } else {
            doc.requirements.push(candidate);
            self.stage_file(catalog, target, &format!("draft {id}: {title}"))?;
        }
        let mut next_step = format!(
            "Review with bdd changes show and apply with bdd changes commit, then add \
             the @{id} scenario with bdd scenario add."
        );
        let refine: Vec<String> = findings
            .into_iter()
            .filter(|issue| {
                !issue.contains("must be phrased") && !issue.contains("title is missing")
            })
            .collect();
        if !refine.is_empty() {
            next_step = format!(
                "Staged {id} with refine findings. Run bdd spec reword {id} to address \
                 them, then bdd changes commit."
            );
        }
        if let Some(warning) = warning {
            next_step = format!("{warning} {next_step}");
        }
        Ok(DraftReport {
            id,
            title,
            staged: true,
            findings: refine,
            next_step,
        })
    }

    /// Point a requirement at the feature file that was actually written.
    /// No-op (and unstaged) when the path is already recorded.
    pub fn set_feature(&self, id: &str, path: &str) -> Result<SetFeatureReport, ServiceError> {
        let mut catalog = self.effective_catalog()?;
        let target = catalog.source_of(id).map(str::to_string).ok_or_else(|| {
            ServiceError(format!(
                "No requirement with id '{id}'. Call spec list to see valid ids."
            ))
        })?;
        let requirement = catalog
            .file_mut(&target)
            .expect("source_of found the file")
            .requirements
            .iter_mut()
            .find(|r| r.id == id)
            .expect("source_of found the requirement");
        if requirement.feature_file.as_deref() == Some(path) {
            return Ok(SetFeatureReport {
                id: id.to_string(),
                feature_file: path.to_string(),
                staged: false,
                next_step: format!("{id} already names {path}."),
            });
        }
        requirement.feature_file = Some(path.to_string());
        self.stage_file(
            &catalog,
            &target,
            &format!("set {id} featureFile to {path}"),
        )?;
        Ok(SetFeatureReport {
            id: id.to_string(),
            feature_file: path.to_string(),
            staged: true,
            next_step: "Review with bdd changes show, then apply with bdd changes commit."
                .to_string(),
        })
    }

    /// Every requirement on the effective (staged-wins) spec, each
    /// naming the catalog file it lives in. New ids that exist only in
    /// staging are labelled `staged`.
    pub fn list_requirements(&self) -> Result<Vec<ListedRequirement>, ServiceError> {
        let disk_ids: std::collections::HashSet<String> = self
            .repository
            .load()
            .map(|spec| spec.requirements.into_iter().map(|r| r.id).collect())
            .unwrap_or_default();
        let catalog = self.effective_catalog()?;
        let disk_ids = &disk_ids;
        Ok(catalog
            .files()
            .iter()
            .flat_map(|file| {
                let path = self.project_path(&file.path);
                file.spec
                    .requirements
                    .iter()
                    .map(move |r| ListedRequirement {
                        staged: !disk_ids.contains(&r.id),
                        id: r.id.clone(),
                        title: r.title.clone(),
                        status: r.status.clone(),
                        file: path.clone(),
                    })
            })
            .collect())
    }

    /// Add an include to the catalog: stages the parent document with
    /// the new entry and, when the included file does not exist anywhere
    /// yet, an empty spec skeleton for it.
    pub fn include_add(
        &self,
        path: &str,
        from: Option<&str>,
    ) -> Result<IncludeReport, ServiceError> {
        let mut catalog = self.effective_catalog()?;
        let child = crate::domain::model::resolve_include("", &self.catalog_relative(path))
            .ok_or_else(|| {
                ServiceError(format!(
                    "{path} escapes the spec directory - keep every spec file under it."
                ))
            })?;
        if !child.ends_with(".json") {
            return Err(ServiceError(format!(
                "{path} is not a .json file - includes name requirement spec files."
            )));
        }
        let parent = match from {
            None => catalog.root().path.clone(),
            Some(from) => {
                let parent = self.catalog_relative(from);
                catalog.file(&parent).ok_or_else(|| {
                    ServiceError(format!(
                        "{from} is not part of the spec catalog. Run bdd spec list to \
                         see where requirements live, or include it first."
                    ))
                })?;
                parent
            }
        };
        if catalog.file(&child).is_some() {
            return Ok(IncludeReport {
                file: self.project_path(&child),
                parent: self.project_path(&parent),
                created: false,
                staged: false,
                next_step: format!(
                    "{} is already part of the spec catalog.",
                    self.project_path(&child)
                ),
            });
        }
        let child_project = self.project_path(&child);
        let created = self
            .store
            .content(&child_project)
            .map_err(|e| ServiceError(e.0))?
            .is_none()
            && self.repository.read_raw(&child).is_err();
        if created {
            self.store
                .stage(
                    &child_project,
                    "{\n  \"requirements\": []\n}\n",
                    &format!("create the spec file {child_project}"),
                )
                .map_err(|e| ServiceError(e.0))?;
        }
        // The include entry is written relative to the parent document.
        let entry = relative_to(parent_dir(&parent), &child);
        catalog
            .file_mut(&parent)
            .expect("checked above")
            .includes
            .push(entry.clone());
        self.stage_file(
            &catalog,
            &parent,
            &format!("include {entry} in {}", self.project_path(&parent)),
        )?;
        Ok(IncludeReport {
            file: child_project,
            parent: self.project_path(&parent),
            created,
            staged: true,
            next_step: "Review with bdd changes show, apply with bdd changes commit, \
                        then draft into it with bdd spec draft --file."
                .to_string(),
        })
    }

    /// The wizard loop shared by manual and assisted drafting. `prior`
    /// pre-fills every prompt (a model proposal or the previous pass's
    /// answers); validate + refine findings drive rewording until clean.
    /// A pass that changes nothing, or `max_reword_passes` passes without
    /// a clean read, hands the decision back to the developer rather than
    /// looping forever.
    fn draft_loop(
        &self,
        prompter: &mut dyn Prompter,
        target: DraftTarget,
        id: String,
        mut prior: Option<Requirement>,
        mut llm: ModelAid,
        replace: bool,
    ) -> Result<DraftReport, ServiceError> {
        let DraftTarget { mut catalog, file } = target;
        let merged = catalog.merged();
        // Every rejected wording and its findings, oldest first: the
        // model's rewording brief carries them so it never circles back
        // to a wording the review already rejected.
        let mut tries: Vec<(Requirement, Vec<String>)> = Vec::new();
        // Passes since the developer last said how to continue, and what
        // the previous pass earned: an unchanged list means the rewording
        // is going nowhere.
        let mut passes = 0;
        let mut previous: Option<Vec<String>> = None;
        let (requirement, title, unresolved) = loop {
            let title = self.ask_field(
                prompter,
                &id,
                "title",
                prior.as_ref().map(|p| p.title.as_str()),
            )?;
            let story = self.ask_field(
                prompter,
                &id,
                "story (As a ..., I want ..., so that ...)",
                prior.as_ref().map(|p| p.story.as_str()),
            )?;
            let prior_criteria = prior
                .as_ref()
                .map(|p| p.acceptance_criteria.as_slice())
                .unwrap_or(&[]);
            let criteria = self.ask_criteria(prompter, &id, prior_criteria)?;
            let candidate = Requirement {
                id: id.clone(),
                title: title.clone(),
                status: "pending".into(),
                story,
                acceptance_criteria: criteria,
                feature_file: prior.as_ref().and_then(|p| p.feature_file.clone()),
            };
            let findings = self.findings_for(&merged, &candidate);
            if findings.is_empty() {
                break (candidate, title, Vec::new());
            }
            let listed = findings.all();
            prompter.tell("Findings to address:");
            for finding in &listed {
                prompter.tell(&format!("  - {finding}"));
                if let Some(suggestion) = suggestion_for(finding) {
                    prompter.tell(&format!("    try: {suggestion}"));
                }
            }
            passes += 1;
            let stalled = previous.as_ref() == Some(&listed);
            previous = Some(listed.clone());
            // Only wording findings are the developer's to wave through -
            // a structurally invalid requirement would not validate, so
            // those keep the loop honest however long it takes.
            if findings.structural.is_empty() && (stalled || passes >= self.max_reword_passes) {
                match self.stalled_choice(prompter, passes, stalled, findings.advisory.len())? {
                    StalledChoice::Accept => break (candidate, title, findings.advisory),
                    StalledChoice::Manual => {
                        llm = None;
                        passes = 0;
                    }
                    StalledChoice::Reword => passes = 0,
                }
            }
            // With a model, the findings become its brief: the next pass's
            // prompts carry its reworded proposal instead of the raw prior.
            let reworded = llm.and_then(|aid| {
                self.rewording(prompter, aid, &merged, &candidate, &listed, &tries)
            });
            tries.push((candidate.clone(), listed));
            prior = Some(match reworded {
                Some(requirement) => requirement,
                None => {
                    prompter.tell(
                        "Reword the requirement to address each finding. Press Enter \
                         on a prompt to keep the prior answer as-is.",
                    );
                    candidate
                }
            });
        };
        let question = if unresolved.is_empty() {
            "The wording reads clean. Stage this requirement?".to_string()
        } else {
            format!(
                "{} wording finding(s) stay open. Stage this requirement anyway?",
                unresolved.len()
            )
        };
        if !self.confirm(prompter, &question)? {
            return Ok(DraftReport {
                id,
                title,
                staged: false,
                findings: unresolved,
                next_step: "Nothing was staged. Run spec draft again when the wording \
                            is ready."
                    .into(),
            });
        }
        let doc = catalog
            .file_mut(&file)
            .expect("the target is part of the catalog");
        if replace {
            if let Some(slot) = doc.requirements.iter_mut().find(|r| r.id == id) {
                *slot = requirement;
            }
            self.stage_file(&catalog, &file, &format!("reword {id}"))?;
        } else {
            doc.requirements.push(requirement);
            self.stage_file(&catalog, &file, &format!("draft {id}: {title}"))?;
        }
        let next_step = if unresolved.is_empty() {
            format!(
                "Review with changes show and apply with changes commit, then add \
                 the @{id} scenario with scenario add."
            )
        } else {
            format!(
                "Staged {id} with {} wording finding(s) you accepted. Run spec reword \
                 {id} to revisit them, or review with changes show and apply with \
                 changes commit, then add the @{id} scenario with scenario add.",
                unresolved.len()
            )
        };
        Ok(DraftReport {
            id: id.clone(),
            title,
            staged: true,
            findings: unresolved,
            next_step,
        })
    }

    /// The way out when rewording stops making progress. Wording
    /// findings are advice, not structure, so whether they stand is the
    /// developer's decision - the wizard asks instead of looping.
    fn stalled_choice(
        &self,
        prompter: &mut dyn Prompter,
        passes: u32,
        stalled: bool,
        open: usize,
    ) -> Result<StalledChoice, ServiceError> {
        let reason = if stalled {
            format!("{passes} pass(es) produced the same {open} finding(s)")
        } else {
            format!("{passes} pass(es) left {open} finding(s) open")
        };
        prompter.tell(&format!("The wording review is not converging - {reason}."));
        loop {
            let answer = self.ask(
                prompter,
                "Choose [r]eword again, [m]anual rewording without the model, \
                 [a]ccept as-is and stage [r/m/a, Enter for r]:",
            )?;
            match answer.to_lowercase().as_str() {
                "" | "r" => return Ok(StalledChoice::Reword),
                "m" => return Ok(StalledChoice::Manual),
                "a" => return Ok(StalledChoice::Accept),
                _ => prompter.warn("Answer r, m, or a."),
            }
        }
    }

    /// Ask the model to reword the draft, one call per finding: each
    /// call addresses exactly one finding and is briefed with the draft
    /// the previous call produced, so the fixes accumulate. An unusable
    /// reply skips that finding; a model error ends the chain (a dead
    /// model would fail every remaining call too). `None` when no call
    /// landed a fix - the developer rewords by hand, exactly as without
    /// a model.
    fn rewording(
        &self,
        prompter: &mut dyn Prompter,
        aid: Model<'_>,
        merged: &Spec,
        candidate: &Requirement,
        findings: &[String],
        history: &[(Requirement, Vec<String>)],
    ) -> Option<Requirement> {
        let (model, llm) = aid;
        let mut current = candidate.clone();
        let mut applied = false;
        let total = findings.len();
        for (index, finding) in findings.iter().enumerate() {
            let work = prompter.working(&format!(
                "Asking {model} to address finding {n} of {total} - working",
                n = index + 1
            ));
            let prompt = rewording_prompt(&current, finding, history);
            // The review itself is the gate: a reply only counts when it
            // leaves fewer findings of the targeted kind than the draft
            // it was given. Anything else goes back to the model with
            // the reason, so the brief stays deterministic.
            let targeted = finding_signature(finding);
            let before = self.matching_findings(merged, &current, targeted);
            let check = |reply: &str| {
                let proposal = parse_rewording_checked(reply)?;
                let reworded = reworded_from(candidate, proposal.clone());
                if reworded.acceptance_criteria.len() < current.acceptance_criteria.len() {
                    return Err(format!(
                        "the rewording dropped acceptance criteria ({} became {}) - keep \
                         every criterion and address only the finding",
                        current.acceptance_criteria.len(),
                        reworded.acceptance_criteria.len()
                    ));
                }
                // An earlier call in this chain may already have cleared
                // this finding, and then there is nothing left to move.
                if before > 0 {
                    if reworded.title == current.title
                        && reworded.story == current.story
                        && reworded.acceptance_criteria == current.acceptance_criteria
                    {
                        return Err("the rewording is identical to the draft - change the \
                                    wording the finding names"
                            .to_string());
                    }
                    let after = self.matching_findings(merged, &reworded, targeted);
                    if after >= before {
                        return Err(format!(
                            "the rewording did not clear the finding - {after} of the same \
                             kind remain (the draft had {before}): {finding}"
                        ));
                    }
                }
                Ok(proposal)
            };
            let outcome = generate_valid(
                llm,
                model,
                &prompt,
                self.llm_attempts,
                check,
                |attempt, of, reason| {
                    prompter.warn(&format!(
                        "The model's rewording was invalid ({reason}) - asking again ({attempt} of {of})"
                    ));
                },
            );
            drop(work);
            let proposal = match outcome {
                Ok(proposal) => proposal,
                Err(LlmReplyError::Call(error)) => {
                    prompter.warn(&format!("The model call failed ({}).", error.0));
                    break;
                }
                Err(LlmReplyError::Invalid { .. }) => {
                    prompter.warn(&format!(
                        "The model's rewording for finding {} was unusable - it stays \
                         yours to fix.",
                        index + 1
                    ));
                    continue;
                }
            };
            current = reworded_from(candidate, proposal);
            applied = true;
        }
        if !applied {
            return None;
        }
        prompter.tell(
            "The model reworded the draft. Each prompt shows its proposal - Enter \
             accepts it, or type your own wording.",
        );
        Some(current)
    }

    /// Flip a requirement to implemented and record its featureFile from
    /// the `@REQ-ID`-tagged feature - the validator demands both, so the
    /// staged spec validates. Refused off GREEN (the status change is the
    /// last step of a passing loop, never a promise) and refused without
    /// a tagged scenario (an implemented requirement needs its executable
    /// scenario). Re-running on an already-implemented requirement
    /// backfills a missing featureFile.
    pub fn mark_implemented(&self, id: &str) -> Result<MarkReport, ServiceError> {
        let snapshot = self.state.load().map_err(|e| ServiceError(e.0))?;
        if snapshot.phase() != TddPhase::Green {
            return Err(ServiceError(format!(
                "Requirements are only marked implemented on GREEN (current phase: \
                 {}). Run the tests and make them pass first.",
                snapshot.phase()
            )));
        }
        let mut catalog = self.effective_catalog()?;
        let target = catalog.source_of(id).map(str::to_string).ok_or_else(|| {
            ServiceError(format!(
                "No requirement with id '{id}'. Call list_requirements to see valid ids."
            ))
        })?;
        let feature = feature_tagged(&self.catalog, &format!("@{id}"))?.ok_or_else(|| {
            ServiceError(format!(
                "No scenario is tagged @{id} - implemented requirements need an \
                 executable scenario. Add one with bdd scenario add, apply it with \
                 bdd changes commit, then mark {id} implemented."
            ))
        })?;
        let requirement = catalog
            .file_mut(&target)
            .expect("source_of found the file")
            .requirements
            .iter_mut()
            .find(|r| r.id == id)
            .expect("source_of found the requirement");
        requirement.status = "implemented".into();
        requirement.feature_file = Some(feature);
        self.stage_file(&catalog, &target, &format!("mark {id} implemented"))?;
        Ok(MarkReport {
            id: id.to_string(),
            status: "implemented".into(),
            staged: true,
            next_step: format!(
                "Review with changes show, run bdd validate (it checks the @{id} \
                 scenario exists), then bdd changes commit."
            ),
        })
    }

    /// The spec tree as it would look after commit: staged content wins
    /// over the working tree file by file, so consecutive drafts stack
    /// and staged includes resolve.
    fn effective_catalog(&self) -> Result<SpecCatalog, ServiceError> {
        load_effective_catalog(&self.repository, &self.store, &self.spec_path)
    }

    /// The catalog document new requirements land in: the root by
    /// default, or the `--file` target, which must already be part of
    /// the include tree.
    fn target_file(
        &self,
        catalog: &SpecCatalog,
        file: Option<&str>,
    ) -> Result<String, ServiceError> {
        let Some(file) = file else {
            return Ok(catalog.root().path.clone());
        };
        let relative = self.catalog_relative(file);
        if catalog.file(&relative).is_some() {
            Ok(relative)
        } else {
            Err(ServiceError(format!(
                "{file} is not part of the spec catalog. Include it with bdd spec \
                 include add {file}, apply with bdd changes commit, then draft into it."
            )))
        }
    }

    /// Accepts a spec file given either as a project path
    /// ("requirements/core/math.json") or as a catalog path relative to
    /// the root document ("core/math.json").
    fn catalog_relative(&self, file: &str) -> String {
        match self.spec_path.rfind('/') {
            Some(cut) => {
                let prefix = format!("{}/", &self.spec_path[..cut]);
                file.strip_prefix(&prefix).unwrap_or(file).to_string()
            }
            None => file.to_string(),
        }
    }

    /// The project-root-relative path of a catalog document
    /// ("core/math.json" -> "requirements/core/math.json").
    fn project_path(&self, catalog_path: &str) -> String {
        match self.spec_path.rfind('/') {
            Some(cut) => format!("{}/{catalog_path}", &self.spec_path[..cut]),
            None => catalog_path.to_string(),
        }
    }

    /// How many findings of one signature a wording earns - the measure
    /// of whether a rewording moved the finding it was asked to fix.
    fn matching_findings(&self, spec: &Spec, candidate: &Requirement, signature: &str) -> usize {
        self.findings_for(spec, candidate)
            .all()
            .iter()
            .filter(|finding| finding_signature(finding) == signature)
            .count()
    }

    fn findings_for(&self, spec: &Spec, candidate: &Requirement) -> Findings {
        let mut with_candidate = spec.clone();
        if let Some(existing) = with_candidate
            .requirements
            .iter_mut()
            .find(|r| r.id == candidate.id)
        {
            *existing = candidate.clone();
        } else {
            with_candidate.requirements.push(candidate.clone());
        }
        let prefix = format!("{}:", candidate.id);
        Findings {
            structural: SpecValidator::new(&self.feature_files)
                .validate(&with_candidate)
                .into_iter()
                .filter(|issue| issue.starts_with(&prefix))
                .collect(),
            advisory: RequirementRefiner.review(candidate),
        }
    }

    /// Stage one catalog document at its project-relative path — the
    /// file the mutation touched, never the whole merged spec.
    fn stage_file(
        &self,
        catalog: &SpecCatalog,
        target: &str,
        summary: &str,
    ) -> Result<(), ServiceError> {
        let doc = &catalog
            .file(target)
            .expect("the target is part of the catalog")
            .spec;
        let json = serde_json::to_string_pretty(doc).expect("spec is always serializable");
        self.store
            .stage(&self.project_path(target), &json, summary)
            .map_err(|e| ServiceError(e.0))?;
        Ok(())
    }

    fn ask(&self, prompter: &mut dyn Prompter, question: &str) -> Result<String, ServiceError> {
        prompter.ask(question).map_err(|e| ServiceError(e.0))
    }

    /// One named field of the requirement. Rewording passes show the prior
    /// answer, and Enter keeps it unchanged.
    fn ask_field(
        &self,
        prompter: &mut dyn Prompter,
        id: &str,
        label: &str,
        prior: Option<&str>,
    ) -> Result<String, ServiceError> {
        match prior {
            None => self.ask(prompter, &format!("{id} {label}:")),
            Some(prior) => {
                let answer = self.ask(
                    prompter,
                    &format!("{id} {label} [{prior}] (Enter keeps it):"),
                )?;
                Ok(if answer.is_empty() {
                    prior.to_string()
                } else {
                    answer
                })
            }
        }
    }

    /// The acceptance criteria list. Prior criteria are offered one by one
    /// (Enter keeps, '-' drops); after them, each blank answer ends the
    /// list — the prompts say so.
    fn ask_criteria(
        &self,
        prompter: &mut dyn Prompter,
        id: &str,
        prior: &[String],
    ) -> Result<Vec<String>, ServiceError> {
        prompter.tell("Acceptance criteria (Given/When/Then). A blank criterion ends the list:");
        let mut criteria = Vec::new();
        for prior_criterion in prior {
            let answer = self.ask(
                prompter,
                &format!(
                    "{id} criterion {} [{prior_criterion}] (Enter keeps it, '-' drops it):",
                    criteria.len() + 1
                ),
            )?;
            match answer.as_str() {
                "" => criteria.push(prior_criterion.clone()),
                "-" => {}
                _ => criteria.push(answer),
            }
        }
        loop {
            let question = format!(
                "{id} criterion {} (leave blank to finish the criteria):",
                criteria.len() + 1
            );
            let criterion = self.ask(prompter, &question)?;
            if criterion.is_empty() {
                break;
            }
            criteria.push(criterion);
        }
        Ok(criteria)
    }

    fn confirm(&self, prompter: &mut dyn Prompter, question: &str) -> Result<bool, ServiceError> {
        prompter.confirm(question).map_err(|e| ServiceError(e.0))
    }
}

/// The requirement a rewording proposal stands for: the model owns the
/// wording, the draft keeps its identity, status, and feature file.
fn reworded_from(candidate: &Requirement, proposal: ProposedRequirement) -> Requirement {
    Requirement {
        id: candidate.id.clone(),
        title: proposal.title,
        status: candidate.status.clone(),
        story: proposal.story,
        acceptance_criteria: proposal.acceptance_criteria,
        feature_file: candidate.feature_file.clone(),
    }
}

/// Enter (or whitespace) keeps every listed proposal. Otherwise a
/// comma-separated list of 1-based numbers from the list; duplicates
/// are dropped and the surviving indices stay in list order.
fn parse_accept_selection(answer: &str, count: usize) -> Result<Vec<usize>, String> {
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        return Ok((0..count).collect());
    }
    let mut picks = Vec::new();
    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.parse::<usize>() {
            Ok(n) if (1..=count).contains(&n) => {
                let idx = n - 1;
                if !picks.contains(&idx) {
                    picks.push(idx);
                }
            }
            _ => {
                return Err(format!(
                    "Pick numbers between 1 and {count}, separated by commas."
                ));
            }
        }
    }
    if picks.is_empty() {
        return Err(format!(
            "Pick numbers between 1 and {count}, separated by commas."
        ));
    }
    picks.sort_unstable();
    Ok(picks)
}

/// The directory of a catalog path ("core/math.json" -> "core").
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(cut) => &path[..cut],
        None => "",
    }
}

/// `target` (relative to the root document's directory) expressed
/// relative to `dir` — the form an include entry is written in.
fn relative_to(dir: &str, target: &str) -> String {
    if dir.is_empty() {
        return target.to_string();
    }
    let dir_parts: Vec<&str> = dir.split('/').collect();
    let target_parts: Vec<&str> = target.split('/').collect();
    let common = dir_parts
        .iter()
        .zip(target_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; dir_parts.len() - common];
    parts.extend(&target_parts[common..]);
    parts.join("/")
}

/// The next free REQ-### id, counting past staged drafts too.
fn next_id(spec: &Spec) -> String {
    let max = spec
        .requirements
        .iter()
        .filter_map(|r| {
            r.id.strip_prefix("REQ-")
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);
    format!("REQ-{:03}", max + 1)
}

fn duplicate_warning(spec: &Spec, candidate: &Requirement) -> Option<String> {
    spec.requirements
        .iter()
        .find(|existing| {
            existing.id != candidate.id
                && (existing.title.eq_ignore_ascii_case(&candidate.title)
                    || existing
                        .acceptance_criteria
                        .iter()
                        .any(|e| candidate.acceptance_criteria.iter().any(|c| c == e)))
        })
        .map(|existing| {
            format!(
                "Warning: {id} looks similar to {} ({}). ",
                existing.id,
                existing.title,
                id = candidate.id
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tdd::TddSnapshot;
    use crate::ports::{PromptError, SpecError};
    use crate::test_support::{
        FakeFeatureFiles, FixedStateStore, InMemoryChangeStore, InMemoryFeatureCatalog,
        InMemorySpecRepository, calculator_catalog,
    };
    use std::collections::VecDeque;

    #[derive(Default)]
    struct ScriptedPrompter {
        answers: VecDeque<String>,
        transcript: Vec<String>,
    }

    impl ScriptedPrompter {
        fn answering(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|a| a.to_string()).collect(),
                transcript: Vec::new(),
            }
        }
    }

    impl Prompter for ScriptedPrompter {
        fn tell(&mut self, message: &str) {
            self.transcript.push(message.to_string());
        }

        fn ask(&mut self, question: &str) -> Result<String, PromptError> {
            self.transcript.push(question.to_string());
            self.answers
                .pop_front()
                .ok_or_else(|| PromptError("input is not readable - script exhausted".into()))
        }

        fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
            Ok(self.ask(question)?.eq_ignore_ascii_case("y"))
        }
    }

    const SPEC_PATH: &str = "requirements/requirements.json";

    fn requirement(id: &str) -> Requirement {
        Requirement {
            id: id.into(),
            title: "A title".into(),
            status: "pending".into(),
            story: "As a user, I want things so that value.".into(),
            acceptance_criteria: vec![
                "Given an empty string \"\", when add is called, then the result is 0".into(),
            ],
            feature_file: None,
        }
    }

    fn spec() -> Spec {
        Spec {
            project: "Kata".into(),
            requirements: vec![requirement("REQ-001"), requirement("REQ-007")],
            ..Spec::default()
        }
    }

    fn green() -> FixedStateStore {
        FixedStateStore::holding(TddSnapshot::at(TddPhase::Green))
    }

    fn service(
        spec: Result<Spec, SpecError>,
        state: FixedStateStore,
    ) -> SpecMutationService<
        InMemorySpecRepository,
        FakeFeatureFiles,
        InMemoryFeatureCatalog,
        InMemoryChangeStore,
        FixedStateStore,
    > {
        // The catalog carries the @REQ-001 tag in features/calc.feature;
        // REQ-007 has no tagged scenario anywhere.
        SpecMutationService::new(
            InMemorySpecRepository(spec),
            FakeFeatureFiles::default(),
            calculator_catalog(),
            InMemoryChangeStore::default(),
            state,
            SPEC_PATH.into(),
        )
    }

    const CLEAN_STORY: &str = "As a user, I want comma sums so that totals come from one input.";
    const CLEAN_CRITERION: &str =
        "Given the input \"1,2\", when add is called, then the result is 3";
    // The refiner's coverage rule wants at least one edge case.
    const EDGE_CRITERION: &str =
        "Given an empty string \"\", when add is called, then the result is 0";

    #[test]
    fn a_clean_draft_is_staged_with_the_next_free_id() {
        let service = service(Ok(spec()), green());
        let mut prompter = ScriptedPrompter::answering(&[
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service.draft(&mut prompter).unwrap();
        assert_eq!(report.id, "REQ-008");
        assert!(report.staged);
        assert!(report.next_step.contains("scenario add"));
        let staged = service.store.content(SPEC_PATH).unwrap().unwrap();
        let staged_spec: Spec = serde_json::from_str(&staged).unwrap();
        assert_eq!(staged_spec.requirements.len(), 3);
        assert_eq!(staged_spec.requirements[2].id, "REQ-008");
        assert_eq!(staged_spec.requirements[2].status, "pending");
        assert_eq!(service.store.summaries()[0], "draft REQ-008: Comma sums");
    }

    #[test]
    fn findings_are_told_and_the_human_rewords_until_clean() {
        let service = service(Ok(spec()), green());
        let mut prompter = ScriptedPrompter::answering(&[
            // first pass: vague story, criterion without Given/When/Then
            "Comma sums",
            "the calculator should handle commas quickly",
            "the result is 3",
            "",
            // second pass: clean
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service.draft(&mut prompter).unwrap();
        assert!(report.staged);
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l == "Findings to address:"),
            "transcript: {:#?}",
            prompter.transcript
        );
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l.contains("must be phrased Given/When/Then")),
            "transcript: {:#?}",
            prompter.transcript
        );
        // Every finding with an obvious fix carries a "try:" suggestion.
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l.contains("try: rephrase as: Given <starting state>")),
            "transcript: {:#?}",
            prompter.transcript
        );
    }

    #[test]
    fn rewording_prompts_carry_the_id_and_the_prior_answers() {
        let service = service(Ok(spec()), green());
        let mut prompter = ScriptedPrompter::answering(&[
            "Comma sums",
            "the calculator should handle commas quickly",
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "",
            CLEAN_STORY,
            "",
            "",
            "",
            "y",
        ]);
        let report = service.draft(&mut prompter).unwrap();
        assert!(report.staged, "report: {report:?}");
        let asked = |question: &str| {
            assert!(
                prompter.transcript.iter().any(|l| l == question),
                "missing {question:?} in transcript: {:#?}",
                prompter.transcript
            );
        };
        asked("REQ-008 title:");
        asked("REQ-008 title [Comma sums] (Enter keeps it):");
        asked(&format!(
            "REQ-008 criterion 1 [{CLEAN_CRITERION}] (Enter keeps it, '-' drops it):"
        ));
        asked("REQ-008 criterion 3 (leave blank to finish the criteria):");
        // Enter kept the prior title, story was replaced, both criteria kept.
        let staged: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        let drafted = &staged.requirements[2];
        assert_eq!(drafted.title, "Comma sums");
        assert_eq!(drafted.story, CLEAN_STORY);
        assert_eq!(
            drafted.acceptance_criteria,
            vec![CLEAN_CRITERION.to_string(), EDGE_CRITERION.to_string()]
        );
    }

    #[test]
    fn a_dash_drops_a_prior_criterion_on_the_rewording_pass() {
        let service = service(Ok(spec()), green());
        let mut prompter = ScriptedPrompter::answering(&[
            // first pass: one malformed criterion plus a clean edge case
            "Comma sums",
            CLEAN_STORY,
            "the result is 3",
            EDGE_CRITERION,
            "",
            // second pass: keep title and story, drop the malformed
            // criterion, keep the edge case, add a clean happy path
            "",
            "",
            "-",
            "",
            CLEAN_CRITERION,
            "",
            "y",
        ]);
        let report = service.draft(&mut prompter).unwrap();
        assert!(report.staged, "report: {report:?}");
        let staged: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert_eq!(
            staged.requirements[2].acceptance_criteria,
            vec![EDGE_CRITERION.to_string(), CLEAN_CRITERION.to_string()]
        );
    }

    /// [`LlmGenerator`] with one scripted outcome.
    struct FakeLlm(Result<String, String>);

    impl LlmGenerator for FakeLlm {
        fn generate(
            &self,
            _model: &str,
            _system: &str,
            _user: &str,
        ) -> Result<String, crate::ports::LlmError> {
            self.0.clone().map_err(crate::ports::LlmError)
        }
    }

    const PROPOSALS: &str = r#"[
        {"title": "Comma separated numbers are summed",
         "story": "As a user, I want comma sums so that totals come from one input.",
         "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
        {"title": "Empty string returns zero",
         "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
         "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}
    ]"#;

    #[test]
    fn a_blank_description_drafts_manually_without_calling_the_model() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Err("must not be called".into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "",
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert!(
            !prompter.transcript.iter().any(|l| l.contains("working")),
            "transcript: {:#?}",
            prompter.transcript
        );
    }

    const REWORDING: &str = r#"{
        "title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": [
            "Given the input \"1,2\", when add is called, then the result is 3",
            "Given an empty string \"\", when add is called, then the result is 0"
        ]
    }"#;

    #[test]
    fn findings_send_the_draft_to_the_model_whose_rewording_seeds_the_next_pass() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(REWORDING.into()));
        // Blank description -> manual first pass with only a happy path,
        // then every rewording prompt accepts the model's proposal.
        let mut prompter = ScriptedPrompter::answering(&[
            "",
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            "",
            "",
            "",
            "",
            "",
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert_eq!(report.title, "Comma separated numbers are summed");
        let told = |line: &str| {
            assert!(
                prompter.transcript.iter().any(|l| l == line),
                "missing {line:?} in transcript: {:#?}",
                prompter.transcript
            );
        };
        told("Asking test-model to address finding 1 of 1 - working ...");
        told(
            "The model reworded the draft. Each prompt shows its proposal - Enter \
             accepts it, or type your own wording.",
        );
        // The rewording prompt carries the model's title, not the raw prior.
        told("REQ-008 title [Comma separated numbers are summed] (Enter keeps it):");
        let staged: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        let drafted = staged.requirements.last().unwrap();
        assert_eq!(drafted.acceptance_criteria.len(), 2);
        assert_eq!(drafted.acceptance_criteria[1], EDGE_CRITERION);
    }

    /// [`LlmGenerator`] recording every call's system and user prompt
    /// (joined with a newline), replying the same thing.
    struct RecordingLlm {
        reply: String,
        prompts: std::cell::RefCell<Vec<String>>,
    }

    impl LlmGenerator for RecordingLlm {
        fn generate(
            &self,
            _model: &str,
            system: &str,
            user: &str,
        ) -> Result<String, crate::ports::LlmError> {
            self.prompts.borrow_mut().push(format!("{system}\n{user}"));
            Ok(self.reply.clone())
        }
    }

    /// A rewording that answers the coverage finding - it adds an edge
    /// case - but words the new criterion vaguely, so the next pass has
    /// a finding of its own and a second rewording pass runs.
    const PARTIAL_REWORDING: &str = r#"{
        "title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": [
            "Given the input \"1,2\", when add is called, then the result is 3",
            "Given an empty string, when add is called, then it works"
        ]
    }"#;

    #[test]
    fn a_second_rewording_pass_recounts_the_wording_the_review_already_rejected() {
        // One attempt per call: the second pass's reply repeats the
        // wording it was briefed with, which the review rejects.
        let service = service(Ok(spec()), green()).with_llm_attempts(1);
        let llm = RecordingLlm {
            reply: PARTIAL_REWORDING.into(),
            prompts: std::cell::RefCell::new(Vec::new()),
        };
        let mut prompter = ScriptedPrompter::answering(&[
            "",              // blank description -> manual first pass
            "Comma sums",    // pass 1: title
            CLEAN_STORY,     // pass 1: story
            CLEAN_CRITERION, // pass 1: only the happy path -> findings
            "",
            "", // pass 2 accepts the model's reworded proposal
            "",
            "",
            "",
            "",
            "",
            "",
            "",             // pass 3: keep title, story, criterion 1
            EDGE_CRITERION, // pass 3: the human words the edge case
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        let prompts = llm.prompts.borrow();
        assert_eq!(prompts.len(), 2, "two rewording passes ran");
        assert!(
            !prompts[0].contains("Earlier wordings"),
            "the first pass has no history"
        );
        assert!(prompts[1].contains("Earlier wordings of this draft"));
        assert!(
            prompts[1].contains("title: Comma sums"),
            "prompt: {}",
            prompts[1]
        );
        assert!(prompts[1].contains("Wording 1 findings:"));
    }

    #[test]
    fn each_finding_is_its_own_model_call_briefed_with_the_previous_fix() {
        let service = service(Ok(spec()), green());
        let llm = RecordingLlm {
            reply: REWORDING.into(),
            prompts: std::cell::RefCell::new(Vec::new()),
        };
        // Pass 1: a story without an actor plus only a happy path -> two
        // findings, so the rewording chain makes two calls.
        let mut prompter = ScriptedPrompter::answering(&[
            "",           // blank description -> manual first pass
            "Comma sums", // pass 1: title
            "The user gets comma sums so that totals come from one input",
            CLEAN_CRITERION,
            "",
            "", // pass 2 accepts the reworded proposal field by field
            "",
            "",
            "",
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        let prompts = llm.prompts.borrow();
        assert_eq!(prompts.len(), 2, "one call per finding");
        assert!(
            prompts[0].contains("story: The user gets comma sums"),
            "the first call is briefed with the raw draft: {}",
            prompts[0]
        );
        assert!(
            prompts[0].contains("- story: missing the actor"),
            "prompt: {}",
            prompts[0]
        );
        assert!(
            prompts[1].contains("story: As a user, I want comma sums"),
            "the second call is briefed with the first call's fix: {}",
            prompts[1]
        );
        assert!(
            prompts[1].contains("- criteria: only happy paths"),
            "prompt: {}",
            prompts[1]
        );
        let told = |line: &str| {
            assert!(
                prompter.transcript.iter().any(|l| l == line),
                "missing {line:?} in transcript: {:#?}",
                prompter.transcript
            );
        };
        told("Asking test-model to address finding 1 of 2 - working ...");
        told("Asking test-model to address finding 2 of 2 - working ...");
    }

    /// [`LlmGenerator`] that answers the first call and fails the rest.
    struct FlakyLlm {
        reply: String,
        calls: std::cell::RefCell<usize>,
    }

    impl LlmGenerator for FlakyLlm {
        fn generate(
            &self,
            _model: &str,
            _system: &str,
            _user: &str,
        ) -> Result<String, crate::ports::LlmError> {
            let mut calls = self.calls.borrow_mut();
            *calls += 1;
            if *calls == 1 {
                Ok(self.reply.clone())
            } else {
                Err(crate::ports::LlmError("boom".into()))
            }
        }
    }

    #[test]
    fn a_model_error_mid_chain_keeps_the_fixes_that_already_landed() {
        let service = service(Ok(spec()), green());
        let llm = FlakyLlm {
            reply: REWORDING.into(),
            calls: std::cell::RefCell::new(0),
        };
        // Two findings: the first call lands the full fix, the second
        // fails - the reworded draft still seeds the next pass.
        let mut prompter = ScriptedPrompter::answering(&[
            "",
            "Comma sums",
            "The user gets comma sums so that totals come from one input",
            CLEAN_CRITERION,
            "",
            "",
            "",
            "",
            "",
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        let told = |line: &str| {
            assert!(
                prompter.transcript.iter().any(|l| l == line),
                "missing {line:?} in transcript: {:#?}",
                prompter.transcript
            );
        };
        told("The model call failed (boom).");
        told(
            "The model reworded the draft. Each prompt shows its proposal - Enter \
             accepts it, or type your own wording.",
        );
        told("REQ-008 title [Comma separated numbers are summed] (Enter keeps it):");
    }

    #[test]
    fn an_unusable_rewording_reply_falls_back_to_hand_rewording() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok("Sure! Here is a better wording:".into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "",
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            "",
            "",
            "",
            "",
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert!(
            prompter.transcript.iter().any(|l| l
                == "The model's rewording for finding 1 was unusable - it stays \
                          yours to fix."),
            "transcript: {:#?}",
            prompter.transcript
        );
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l.starts_with("Reword the requirement to address each finding.")),
            "the manual instructions still print on fallback"
        );
    }

    #[test]
    fn a_model_error_during_rewording_falls_back_to_hand_rewording() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Err("boom".into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "",
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            "",
            "",
            "",
            "",
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l == "The model call failed (boom)."),
            "transcript: {:#?}",
            prompter.transcript
        );
    }

    #[test]
    fn a_model_error_falls_back_to_manual_drafting() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Err("ollama is not reachable".into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers from a string",
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        let told = |fragment: &str| {
            assert!(
                prompter.transcript.iter().any(|l| l.contains(fragment)),
                "missing {fragment:?} in transcript: {:#?}",
                prompter.transcript
            );
        };
        told("The model call failed (ollama is not reachable).");
        told("drafting manually");
    }

    #[test]
    fn an_unusable_reply_falls_back_to_manual_drafting() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok("Sure! Here are the requirements:".into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers from a string",
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l.contains("The description gave no complete requirement")),
            "transcript: {:#?}",
            prompter.transcript
        );
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l.contains("asking again (2 of 3)")),
            "invalid replies must be retried: {:#?}",
            prompter.transcript
        );
    }

    #[test]
    fn an_invalid_split_is_retried_with_the_prior_reply() {
        let service = service(Ok(spec()), green());
        let llm = RecordingLlm {
            reply: "Sure! Here are the requirements:".into(),
            prompts: Default::default(),
        };
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers from a string",
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        let prompts = llm.prompts.borrow();
        assert_eq!(prompts.len(), 3, "default is three attempts: {prompts:#?}");
        assert!(
            prompts[1].contains("Your previous reply was invalid"),
            "retry 2: {}",
            prompts[1]
        );
        assert!(
            prompts[1].contains("Sure! Here are the requirements:"),
            "retry 2: {}",
            prompts[1]
        );
        assert!(
            prompts[2].contains("Your previous reply was invalid"),
            "retry 3: {}",
            prompts[2]
        );
    }

    #[test]
    fn a_single_proposal_seeds_the_wizard_without_a_selection_question() {
        let single = r#"[{"title": "Empty string returns zero",
            "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
            "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]"#;
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(single.into()));
        let mut prompter =
            ScriptedPrompter::answering(&["empty input means zero", "", "", "", "", "y"]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert_eq!(report.title, "Empty string returns zero");
        assert!(
            !prompter
                .transcript
                .iter()
                .any(|l| l.contains("Which requirement first?")),
            "transcript: {:#?}",
            prompter.transcript
        );
        assert!(
            !prompter
                .transcript
                .iter()
                .any(|l| l.contains("Accept [Enter for all")),
            "transcript: {:#?}",
            prompter.transcript
        );
        assert!(
            prompter
                .transcript
                .iter()
                .any(|l| l == "REQ-008 title [Empty string returns zero] (Enter keeps it):"),
            "transcript: {:#?}",
            prompter.transcript
        );
    }

    #[test]
    fn the_pick_selects_the_first_to_work_and_every_accepted_proposal_is_stored() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers, empty means zero",
            "",
            "2",
            "",
            "",
            "",
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        // Accept-all stores both; the pick decides which stored
        // requirement the wizard walks through first - here the
        // second, stored as REQ-009.
        assert_eq!(report.id, "REQ-009");
        assert_eq!(report.title, "Empty string returns zero");
        let told = |fragment: &str| {
            assert!(
                prompter.transcript.iter().any(|l| l.contains(fragment)),
                "missing {fragment:?} in transcript: {:#?}",
                prompter.transcript
            );
        };
        told(
            "Accept all these requirements to refine, or enter comma-separated \
             numbers of the ones to accept.",
        );
        told("The description holds 2 requirement(s):");
        told("1. Comma separated numbers are summed");
        told("2. Empty string returns zero");
        told("Accepted requirements are now stored in requirements/requirements.json as pending:");
        told("REQ-008 Comma separated numbers are summed");
        told("REQ-009 Empty string returns zero");
        told("Which requirement first to review and refine?");
        // Both accepted proposals sit in the staged spec.
        let staged: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert_eq!(staged.requirements.len(), 4);
        assert_eq!(staged.requirements[2].id, "REQ-008");
        assert_eq!(
            staged.requirements[2].title,
            "Comma separated numbers are summed"
        );
        assert_eq!(staged.requirements[2].status, "pending");
        assert_eq!(staged.requirements[3].id, "REQ-009");
        assert_eq!(staged.requirements[3].title, "Empty string returns zero");
        assert_eq!(staged.requirements[3].status, "pending");
        assert_eq!(
            service.store.summaries()[0],
            "draft REQ-008, REQ-009 from the description"
        );
    }

    #[test]
    fn a_comma_separated_accept_stores_only_those_proposals() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers, empty means zero",
            "2",
            "",
            "",
            "",
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert_eq!(report.id, "REQ-008");
        assert_eq!(report.title, "Empty string returns zero");
        assert!(
            !prompter
                .transcript
                .iter()
                .any(|l| l.contains("Which requirement first?")),
            "a single accepted proposal skips the which-first question: {:#?}",
            prompter.transcript
        );
        let staged: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert_eq!(staged.requirements.len(), 3);
        assert_eq!(staged.requirements[2].id, "REQ-008");
        assert_eq!(staged.requirements[2].title, "Empty string returns zero");
        assert_eq!(
            service.store.summaries()[0],
            "draft REQ-008 from the description"
        );
    }

    #[test]
    fn an_invalid_accept_reasks_until_a_selection_arrives() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers, empty means zero",
            "9",
            "all",
            "1,2",
            "2",
            "",
            "",
            "",
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert_eq!(report.title, "Empty string returns zero");
        assert_eq!(
            prompter
                .transcript
                .iter()
                .filter(|l| l.as_str() == "Pick numbers between 1 and 2, separated by commas.")
                .count(),
            2,
            "transcript: {:#?}",
            prompter.transcript
        );
    }

    #[test]
    fn an_invalid_pick_reasks_until_a_number_from_the_list_arrives() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers, empty means zero",
            "",
            "9",
            "first",
            "2",
            "",
            "",
            "",
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert_eq!(report.title, "Empty string returns zero");
        assert_eq!(
            prompter
                .transcript
                .iter()
                .filter(|l| l.as_str() == "Pick a number between 1 and 2.")
                .count(),
            2,
            "transcript: {:#?}",
            prompter.transcript
        );
    }

    #[test]
    fn an_empty_pick_means_the_first_accepted_proposal() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&[
            "sum numbers, empty means zero",
            "",
            "",
            "",
            "",
            "",
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap();
        assert!(report.staged, "report: {report:?}");
        assert_eq!(report.title, "Comma separated numbers are summed");
    }

    #[test]
    fn a_description_prompt_error_propagates() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&[]);
        let error = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap_err();
        assert!(error.0.contains("script exhausted"), "error: {error:?}");
    }

    #[test]
    fn an_accept_prompt_error_propagates() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&["sum numbers, empty means zero"]);
        let error = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap_err();
        assert!(error.0.contains("script exhausted"), "error: {error:?}");
    }

    #[test]
    fn a_pick_prompt_error_propagates() {
        let service = service(Ok(spec()), green());
        let llm = FakeLlm(Ok(PROPOSALS.into()));
        let mut prompter = ScriptedPrompter::answering(&["sum numbers, empty means zero", ""]);
        let error = service
            .draft_assisted(&mut prompter, "test-model", &llm)
            .unwrap_err();
        assert!(error.0.contains("script exhausted"), "error: {error:?}");
    }

    #[test]
    fn parse_accept_selection_keeps_every_index_on_enter() {
        assert_eq!(parse_accept_selection("", 5), Ok(vec![0, 1, 2, 3, 4]));
        assert_eq!(parse_accept_selection("  ", 2), Ok(vec![0, 1]));
    }

    #[test]
    fn parse_accept_selection_dedupes_and_orders_comma_separated_picks() {
        assert_eq!(parse_accept_selection("5, 1, 1, 3", 5), Ok(vec![0, 2, 4]));
    }

    #[test]
    fn parse_accept_selection_rejects_out_of_range_and_empty_lists() {
        assert!(parse_accept_selection("9", 5).is_err());
        assert!(parse_accept_selection("all", 5).is_err());
        assert!(parse_accept_selection(",", 5).is_err());
    }

    #[test]
    fn a_declined_draft_stages_nothing() {
        let service = service(Ok(spec()), green());
        let mut prompter = ScriptedPrompter::answering(&[
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "n",
        ]);
        let report = service.draft(&mut prompter).unwrap();
        assert!(!report.staged);
        assert!(report.next_step.starts_with("Nothing was staged."));
        assert_eq!(service.store.content(SPEC_PATH).unwrap(), None);
    }

    #[test]
    fn drafting_stacks_on_a_previously_staged_spec() {
        let service = service(Ok(spec()), green());
        service
            .store
            .stage(
                SPEC_PATH,
                &serde_json::to_string(&Spec {
                    project: "Kata".into(),
                    requirements: vec![requirement("REQ-011")],
                    ..Spec::default()
                })
                .unwrap(),
                "earlier draft",
            )
            .unwrap();
        let mut prompter = ScriptedPrompter::answering(&[
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        let report = service.draft(&mut prompter).unwrap();
        assert_eq!(report.id, "REQ-012");
    }

    #[test]
    fn an_exhausted_prompt_script_propagates_as_an_error() {
        let service = service(Ok(spec()), green());
        let mut prompter = ScriptedPrompter::answering(&["Comma sums"]);
        let error = service.draft(&mut prompter).unwrap_err();
        assert_eq!(
            error,
            ServiceError("input is not readable - script exhausted".into())
        );
    }

    #[test]
    fn a_prompt_error_on_a_rewording_pass_field_propagates() {
        let service = service(Ok(spec()), green());
        // The script ends right where the second pass re-asks the title.
        let mut prompter = ScriptedPrompter::answering(&[
            "Comma sums",
            "the calculator should handle commas quickly",
            "the result is 3",
            "",
        ]);
        let error = service.draft(&mut prompter).unwrap_err();
        assert_eq!(
            error,
            ServiceError("input is not readable - script exhausted".into())
        );
    }

    #[test]
    fn a_prompt_error_on_a_prior_criterion_propagates() {
        let service = service(Ok(spec()), green());
        // The script ends right where the second pass offers criterion 1.
        let mut prompter = ScriptedPrompter::answering(&[
            "Comma sums",
            "the calculator should handle commas quickly",
            "the result is 3",
            "",
            "",
            "",
        ]);
        let error = service.draft(&mut prompter).unwrap_err();
        assert_eq!(
            error,
            ServiceError("input is not readable - script exhausted".into())
        );
    }

    #[test]
    fn draft_propagates_a_failing_repository() {
        let service = service(Err(SpecError("spec: boom".into())), green());
        let mut prompter = ScriptedPrompter::default();
        assert_eq!(
            service.draft(&mut prompter).unwrap_err(),
            ServiceError("spec: boom".into())
        );
    }

    #[test]
    fn a_malformed_staged_spec_is_a_structured_error() {
        let service = service(Ok(spec()), green());
        service.store.stage(SPEC_PATH, "not json", "oops").unwrap();
        let mut prompter = ScriptedPrompter::default();
        let error = service.draft(&mut prompter).unwrap_err();
        assert!(
            error
                .0
                .starts_with("spec: staged requirements/requirements.json is not readable JSON -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn the_first_draft_of_an_empty_spec_gets_req_001() {
        let service = service(
            Ok(Spec {
                project: "Kata".into(),
                requirements: vec![],
                ..Spec::default()
            }),
            green(),
        );
        let mut prompter = ScriptedPrompter::answering(&[
            "Comma sums",
            CLEAN_STORY,
            CLEAN_CRITERION,
            EDGE_CRITERION,
            "",
            "y",
        ]);
        assert_eq!(service.draft(&mut prompter).unwrap().id, "REQ-001");
    }

    #[test]
    fn mark_implemented_on_green_stages_the_status_flip_and_the_feature_file() {
        let service = service(Ok(spec()), green());
        let report = service.mark_implemented("REQ-001").unwrap();
        assert_eq!(
            report,
            MarkReport {
                id: "REQ-001".into(),
                status: "implemented".into(),
                staged: true,
                next_step: "Review with changes show, run bdd validate (it checks the \
                            @REQ-001 scenario exists), then bdd changes commit."
                    .into(),
            }
        );
        let staged = service.store.content(SPEC_PATH).unwrap().unwrap();
        let staged_spec: Spec = serde_json::from_str(&staged).unwrap();
        assert_eq!(staged_spec.requirements[0].status, "implemented");
        assert_eq!(
            staged_spec.requirements[0].feature_file.as_deref(),
            Some("features/calc.feature"),
            "the tagged feature is recorded so the spec validates"
        );
        assert_eq!(staged_spec.requirements[1].status, "pending");
    }

    #[test]
    fn mark_implemented_without_a_tagged_scenario_names_the_recovery_commands() {
        let service = service(Ok(spec()), green());
        let error = service.mark_implemented("REQ-007").unwrap_err();
        assert_eq!(
            error.0,
            "No scenario is tagged @REQ-007 - implemented requirements need an \
             executable scenario. Add one with bdd scenario add, apply it with \
             bdd changes commit, then mark REQ-007 implemented."
        );
        assert_eq!(service.store.content(SPEC_PATH).unwrap(), None);
    }

    #[test]
    fn a_rerun_backfills_the_feature_file_of_an_already_implemented_requirement() {
        // The broken-on-disk shape: implemented but no featureFile.
        let mut broken = spec();
        broken.requirements[0].status = "implemented".into();
        broken.requirements[0].feature_file = None;
        let service = service(Ok(broken), green());
        let report = service.mark_implemented("REQ-001").unwrap();
        assert!(report.staged);
        let staged: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert_eq!(staged.requirements[0].status, "implemented");
        assert_eq!(
            staged.requirements[0].feature_file.as_deref(),
            Some("features/calc.feature")
        );
    }

    #[test]
    fn mark_implemented_is_refused_off_green() {
        for phase in [TddPhase::Start, TddPhase::Red, TddPhase::Refactor] {
            let service = service(Ok(spec()), FixedStateStore::holding(TddSnapshot::at(phase)));
            let error = service.mark_implemented("REQ-001").unwrap_err();
            assert_eq!(
                error.0,
                format!(
                    "Requirements are only marked implemented on GREEN (current \
                     phase: {phase}). Run the tests and make them pass first."
                )
            );
        }
    }

    #[test]
    fn mark_implemented_of_an_unknown_id_names_the_recovery_tool() {
        let service = service(Ok(spec()), green());
        assert_eq!(
            service.mark_implemented("REQ-999").unwrap_err(),
            ServiceError(
                "No requirement with id 'REQ-999'. Call list_requirements to see valid ids.".into()
            )
        );
    }

    #[test]
    fn mark_implemented_propagates_a_failing_state_store() {
        let service = service(
            Ok(spec()),
            FixedStateStore::failing(".bdd-state.json is not readable - boom"),
        );
        assert_eq!(
            service.mark_implemented("REQ-001").unwrap_err(),
            ServiceError(".bdd-state.json is not readable - boom".into())
        );
    }

    #[test]
    fn a_flag_draft_stages_the_next_id_without_a_tty() {
        let service = service(Ok(spec()), green());
        let report = service
            .draft_direct(
                "Newlines as delimiters",
                "As a calculator user, I want newlines to separate numbers in addition to commas so that multi-line input just works.",
                vec![
                    "Given the input \"1\\n2,3\", when add is called, then the result is 6".into(),
                    EDGE_CRITERION.into(),
                ],
            )
            .unwrap();
        assert_eq!(report.id, "REQ-008");
        assert!(report.staged);
        let listed = service.list_requirements().unwrap();
        assert!(
            listed.iter().any(|r| r.id == "REQ-008" && r.staged),
            "listed: {listed:?}"
        );
        assert!(listed.iter().any(|r| r.id == "REQ-001" && !r.staged));
    }

    #[test]
    fn a_duplicate_draft_warns_without_blocking() {
        let service = service(Ok(spec()), green());
        let report = service
            .draft_direct(
                "A title",
                CLEAN_STORY,
                vec![CLEAN_CRITERION.into(), EDGE_CRITERION.into()],
            )
            .unwrap();
        assert!(
            report.next_step.contains("looks similar to REQ-001"),
            "next step: {}",
            report.next_step
        );
    }

    #[test]
    fn set_feature_stages_a_path_rewrite() {
        let service = service(Ok(spec()), green());
        let report = service
            .set_feature("REQ-001", "src/test/resources/features/calc.feature")
            .unwrap();
        assert!(report.staged);
        assert_eq!(
            report.feature_file,
            "src/test/resources/features/calc.feature"
        );
        let noop = service
            .set_feature("REQ-001", "src/test/resources/features/calc.feature")
            .unwrap();
        assert!(!noop.staged);
    }

    #[test]
    fn reword_direct_replaces_an_existing_row() {
        let service = service(Ok(spec()), green());
        let report = service
            .reword_direct(
                "REQ-001",
                Some("Comma sums".into()),
                Some(CLEAN_STORY.into()),
                vec![CLEAN_CRITERION.into(), EDGE_CRITERION.into()],
            )
            .unwrap();
        assert_eq!(report.id, "REQ-001");
        let staged: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert_eq!(staged.requirements[0].title, "Comma sums");
        assert_eq!(staged.requirements.len(), 2);
    }

    // ---- catalog includes ---------------------------------------------------

    /// Stage a two-file catalog: the root includes core.json, which
    /// holds REQ-001 (tagged in the feature catalog) and REQ-007.
    fn stage_split_spec(
        service: &SpecMutationService<
            InMemorySpecRepository,
            FakeFeatureFiles,
            InMemoryFeatureCatalog,
            InMemoryChangeStore,
            FixedStateStore,
        >,
    ) {
        service
            .store
            .stage(
                SPEC_PATH,
                r#"{"project":"Kata","includes":["core.json"],"requirements":[]}"#,
                "split the spec",
            )
            .unwrap();
        service
            .store
            .stage(
                "requirements/core.json",
                &serde_json::to_string(&Spec {
                    requirements: vec![requirement("REQ-001"), requirement("REQ-007")],
                    ..Spec::default()
                })
                .unwrap(),
                "the core spec file",
            )
            .unwrap();
    }

    #[test]
    fn a_draft_into_an_included_file_stages_only_that_file() {
        let service = service(Ok(spec()), green());
        stage_split_spec(&service);
        let report = service
            .draft_direct_in(
                "Newlines as delimiters",
                CLEAN_STORY,
                vec![CLEAN_CRITERION.into(), EDGE_CRITERION.into()],
                Some("requirements/core.json"),
            )
            .unwrap();
        assert_eq!(report.id, "REQ-008", "ids count across the whole catalog");
        let child: Spec = serde_json::from_str(
            &service
                .store
                .content("requirements/core.json")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(child.requirements.len(), 3);
        assert_eq!(child.requirements[2].id, "REQ-008");
        let root: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert!(
            root.requirements.is_empty(),
            "the root document was left alone"
        );
    }

    #[test]
    fn a_draft_into_a_file_outside_the_catalog_is_refused() {
        let service = service(Ok(spec()), green());
        let error = service
            .draft_direct_in(
                "Newlines as delimiters",
                CLEAN_STORY,
                vec![CLEAN_CRITERION.into()],
                Some("requirements/other.json"),
            )
            .unwrap_err();
        assert!(
            error.0.contains("not part of the spec catalog"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn mark_implemented_writes_back_to_the_file_the_requirement_lives_in() {
        let service = service(Ok(spec()), green());
        stage_split_spec(&service);
        let report = service.mark_implemented("REQ-001").unwrap();
        assert!(report.staged);
        let child: Spec = serde_json::from_str(
            &service
                .store
                .content("requirements/core.json")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(child.requirements[0].status, "implemented");
        assert_eq!(
            child.requirements[0].feature_file.as_deref(),
            Some("features/calc.feature")
        );
        let root: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert!(root.requirements.is_empty());
    }

    #[test]
    fn reword_direct_writes_back_to_the_included_file() {
        let service = service(Ok(spec()), green());
        stage_split_spec(&service);
        let report = service
            .reword_direct(
                "REQ-007",
                Some("Comma sums".into()),
                Some(CLEAN_STORY.into()),
                vec![CLEAN_CRITERION.into(), EDGE_CRITERION.into()],
            )
            .unwrap();
        assert_eq!(report.id, "REQ-007");
        let child: Spec = serde_json::from_str(
            &service
                .store
                .content("requirements/core.json")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(child.requirements[1].title, "Comma sums");
    }

    #[test]
    fn listed_requirements_name_the_file_they_live_in() {
        let service = service(Ok(spec()), green());
        stage_split_spec(&service);
        let listed = service.list_requirements().unwrap();
        assert_eq!(listed.len(), 2);
        assert!(
            listed.iter().all(|r| r.file == "requirements/core.json"),
            "listed: {listed:?}"
        );
    }

    #[test]
    fn include_add_stages_the_parent_entry_and_an_empty_child() {
        let service = service(Ok(spec()), green());
        let report = service
            .include_add("requirements/core/math.json", None)
            .unwrap();
        assert!(report.staged);
        assert!(report.created);
        assert_eq!(report.file, "requirements/core/math.json");
        assert_eq!(report.parent, "requirements/requirements.json");
        let root: Spec =
            serde_json::from_str(&service.store.content(SPEC_PATH).unwrap().unwrap()).unwrap();
        assert_eq!(root.includes, vec!["core/math.json"]);
        assert_eq!(root.requirements.len(), 2, "existing rows survive");
        let child: Spec = serde_json::from_str(
            &service
                .store
                .content("requirements/core/math.json")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(child.requirements.is_empty());
    }

    #[test]
    fn a_staged_include_is_immediately_draftable_into() {
        let service = service(Ok(spec()), green());
        service
            .include_add("requirements/core/math.json", None)
            .unwrap();
        let report = service
            .draft_direct_in(
                "Newlines as delimiters",
                CLEAN_STORY,
                vec![CLEAN_CRITERION.into(), EDGE_CRITERION.into()],
                Some("requirements/core/math.json"),
            )
            .unwrap();
        assert_eq!(report.id, "REQ-008");
        let child: Spec = serde_json::from_str(
            &service
                .store
                .content("requirements/core/math.json")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(child.requirements[0].id, "REQ-008");
    }

    #[test]
    fn include_add_of_an_already_included_file_is_a_no_op() {
        let service = service(Ok(spec()), green());
        service
            .include_add("requirements/core/math.json", None)
            .unwrap();
        let report = service
            .include_add("requirements/core/math.json", None)
            .unwrap();
        assert!(!report.staged);
        assert!(!report.created);
        assert!(
            report
                .next_step
                .contains("already part of the spec catalog")
        );
    }

    #[test]
    fn include_add_from_a_child_writes_the_entry_relative_to_that_child() {
        let service = service(Ok(spec()), green());
        service
            .include_add("requirements/core/math.json", None)
            .unwrap();
        let report = service
            .include_add(
                "requirements/core/edge.json",
                Some("requirements/core/math.json"),
            )
            .unwrap();
        assert!(report.staged);
        assert_eq!(report.parent, "requirements/core/math.json");
        let parent: Spec = serde_json::from_str(
            &service
                .store
                .content("requirements/core/math.json")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(parent.includes, vec!["edge.json"]);
    }

    #[test]
    fn include_add_refuses_non_json_and_escaping_paths() {
        let service = service(Ok(spec()), green());
        let error = service
            .include_add("requirements/core/math.yaml", None)
            .unwrap_err();
        assert!(error.0.contains("not a .json file"), "got: {}", error.0);
        let error = service.include_add("../outside.json", None).unwrap_err();
        assert!(error.0.contains("escapes"), "got: {}", error.0);
    }

    #[test]
    fn include_add_from_an_unknown_parent_is_refused() {
        let service = service(Ok(spec()), green());
        let error = service
            .include_add(
                "requirements/core/edge.json",
                Some("requirements/missing.json"),
            )
            .unwrap_err();
        assert!(
            error.0.contains("not part of the spec catalog"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn relative_to_walks_up_and_down_between_catalog_directories() {
        assert_eq!(relative_to("", "core/math.json"), "core/math.json");
        assert_eq!(relative_to("core", "core/edge.json"), "edge.json");
        assert_eq!(relative_to("core", "other/b.json"), "../other/b.json");
        assert_eq!(relative_to("a/b", "a/c/d.json"), "../c/d.json");
    }

    #[test]
    fn reword_of_an_unknown_id_names_the_recovery_command() {
        let service = service(Ok(spec()), green());
        let error = service
            .reword(&mut ScriptedPrompter::answering(&[]), "REQ-999")
            .unwrap_err();
        assert!(
            error.0.contains("No requirement with id 'REQ-999'"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn reword_direct_of_an_unknown_id_names_the_recovery_command() {
        let service = service(Ok(spec()), green());
        let error = service
            .reword_direct("REQ-999", None, None, vec![])
            .unwrap_err();
        assert!(
            error.0.contains("No requirement with id 'REQ-999'"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn set_feature_of_an_unknown_id_names_the_recovery_command() {
        let service = service(Ok(spec()), green());
        let error = service
            .set_feature("REQ-999", "features/x.feature")
            .unwrap_err();
        assert!(
            error.0.contains("No requirement with id 'REQ-999'"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn draft_direct_refuses_structural_problems() {
        let service = service(Ok(spec()), green());
        let error = service
            .draft_direct("", CLEAN_STORY, vec![CLEAN_CRITERION.into()])
            .unwrap_err();
        assert!(
            error.0.contains("title is missing") || error.0.contains("must be phrased"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn draft_direct_reports_refine_findings_without_blocking() {
        let service = service(Ok(spec()), green());
        let report = service
            .draft_direct(
                "Comma sums",
                "the calculator should add quickly",
                vec![CLEAN_CRITERION.into(), EDGE_CRITERION.into()],
            )
            .unwrap();
        assert!(report.staged);
        assert!(
            report.next_step.contains("refine findings") || report.next_step.contains("reword"),
            "next step: {}",
            report.next_step
        );
    }

    #[test]
    fn list_requirements_with_a_bare_spec_path_keeps_catalog_paths() {
        let service = SpecMutationService::new(
            InMemorySpecRepository(Ok(spec())),
            FakeFeatureFiles::default(),
            calculator_catalog(),
            InMemoryChangeStore::default(),
            green(),
            "requirements.json".into(),
        );
        let listed = service.list_requirements().unwrap();
        assert!(
            listed.iter().any(|r| r.file == "requirements.json"),
            "listed: {listed:?}"
        );
    }

    #[test]
    fn reword_keeps_a_clean_requirement_with_enter() {
        let mut spec = spec();
        spec.requirements[0].title = "Comma sums".into();
        spec.requirements[0].story = CLEAN_STORY.into();
        spec.requirements[0].acceptance_criteria =
            vec![CLEAN_CRITERION.into(), EDGE_CRITERION.into()];
        let service = service(Ok(spec), green());
        let mut prompter = ScriptedPrompter::answering(&["", "", "", "", "", "y"]);
        let report = service.reword(&mut prompter, "REQ-001").unwrap();
        assert!(report.staged);
        assert_eq!(report.id, "REQ-001");
    }
}
