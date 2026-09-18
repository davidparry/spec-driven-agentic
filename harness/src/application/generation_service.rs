//! Step and unit-test generation: discovery of undefined steps, and the
//! hybrid template + LLM generation flow. The deterministic template from
//! the domain always works; when a model is resolved its output is
//! preferred after validation, otherwise the template is used silently.
//! Everything generated lands in the staging area, never in working files.

use serde::Serialize;

use crate::application::agent_service::{Agent, AgentConfig, DEFAULT_MAX_ROUNDS, NullBroker};
use crate::application::assets::{
    find_missing_steps, find_requirement, load_effective_spec, production_path,
    production_type_name, steps_path, unit_test_path,
};
use crate::application::spec_service::ServiceError;
use crate::application::{DEFAULT_LLM_ATTEMPTS, LlmReplyError};
use crate::domain::generation::{
    append_step_definitions, looks_like_step_definitions, looks_like_step_fragment,
    looks_like_unit_test, looks_like_unit_test_for, looks_like_unit_test_fragment,
    polish_fragment_prompt, polish_prompt, splice_step_definitions, splice_unit_tests,
    step_definitions_fragment, step_definitions_template, strip_code_fences, todo_placeholders,
    unit_test_fragment, unit_test_target_path, unit_test_template,
};
use crate::domain::language::Language;
use crate::domain::memory::ProjectStructure;
use crate::domain::steps::{MissingStep, extract_patterns, source_extension};
use crate::ports::{
    ChangeStore, FeatureCatalog, LlmConversation, Prompter, SourceFiles, SpecRepository, ToolBroker,
};

/// Reply of `spec steps missing`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MissingStepsReport {
    pub language: String,
    pub framework: String,
    pub missing: Vec<MissingStep>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// Reply of `spec steps generate` and `spec unittest generate`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GenerationReport {
    pub target: String,
    pub staged: bool,
    /// "template" for the deterministic output, "llm" when a model's
    /// polished version passed validation.
    pub source: String,
    pub summary: String,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// The resolved LLM, when one is available: an [`Agent`] already scoped
/// to this caller's tools.
pub struct ResolvedLlm<L: LlmConversation, B: ToolBroker = NullBroker> {
    agent: Agent<L, B>,
}

impl<L: LlmConversation> ResolvedLlm<L, NullBroker> {
    pub fn new(model: impl Into<String>, chat: L) -> Self {
        Self::with_attempts(model, chat, DEFAULT_LLM_ATTEMPTS)
    }

    pub fn with_attempts(model: impl Into<String>, chat: L, attempts: u32) -> Self {
        Self {
            agent: Agent::new(
                model,
                chat,
                NullBroker,
                Vec::new(),
                AgentConfig::new("llm", attempts, DEFAULT_MAX_ROUNDS, Vec::new()),
            ),
        }
    }
}

impl<L: LlmConversation, B: ToolBroker> ResolvedLlm<L, B> {
    pub fn connected(
        model: impl Into<String>,
        chat: L,
        broker: B,
        tools: Vec<crate::domain::tools::ToolDefinition>,
        config: crate::application::agent_service::AgentConfig,
    ) -> Self {
        Self {
            agent: Agent::new(model, chat, broker, tools, config),
        }
    }

    pub fn ask<T>(
        &self,
        prompter: &mut dyn Prompter,
        prompt: &crate::domain::prompts::RenderedPrompt,
        parse: impl Fn(&str) -> Result<T, String>,
        on_retry: impl FnMut(u32, u32, &str),
    ) -> Result<T, LlmReplyError> {
        self.agent.ask(prompter, prompt, parse, on_retry)
    }

    #[cfg(test)]
    pub(crate) fn chat(&self) -> &L {
        self.agent.chat()
    }
}

pub struct GenerationService<F, S, C, R, L, B = NullBroker>
where
    F: FeatureCatalog,
    S: SourceFiles,
    C: ChangeStore,
    R: SpecRepository,
    L: LlmConversation,
    B: ToolBroker,
{
    features: F,
    sources: S,
    store: C,
    spec: R,
    language: Language,
    layout: ProjectStructure,
    llm: Option<ResolvedLlm<L, B>>,
}

impl<F, S, C, R, L, B> GenerationService<F, S, C, R, L, B>
where
    F: FeatureCatalog,
    S: SourceFiles,
    C: ChangeStore,
    R: SpecRepository,
    L: LlmConversation,
    B: ToolBroker,
{
    pub fn new(
        features: F,
        sources: S,
        store: C,
        spec: R,
        language: Language,
        layout: ProjectStructure,
        llm: Option<ResolvedLlm<L, B>>,
    ) -> Self {
        Self {
            features,
            sources,
            store,
            spec,
            language,
            layout,
            llm,
        }
    }

    /// Every feature step with no matching definition (step_definitions_find).
    pub fn steps_missing(&self) -> Result<MissingStepsReport, ServiceError> {
        let missing = find_missing_steps(&self.features, &self.sources, self.language)?;
        let next_step = if missing.is_empty() {
            "Every step has a definition. Run spec test to execute the suite.".to_string()
        } else {
            format!(
                "{} step(s) have no definition. Run spec steps generate to stage pending definitions for them.",
                missing.len()
            )
        };
        Ok(MissingStepsReport {
            language: self.language.display().to_string(),
            framework: self.language.bdd_framework().to_string(),
            missing,
            next_step,
        })
    }

    /// Stage pending step definitions for every undefined step
    /// (step_definition_create). When the module already has a
    /// step-definition file, the new definitions are appended to it
    /// rather than written to a parallel file whose patterns Cucumber
    /// would reject as duplicates.
    pub fn steps_generate(
        &self,
        prompter: &mut dyn Prompter,
    ) -> Result<GenerationReport, ServiceError> {
        let missing = find_missing_steps(&self.features, &self.sources, self.language)?;
        if missing.is_empty() {
            return Err(ServiceError(
                "Every step already has a definition - nothing to generate.".into(),
            ));
        }
        let sources = self.sources.sources(source_extension(self.language))?;
        let target = steps_path(&sources, self.language, &self.layout);
        let existing = sources.iter().find(|file| file.path == target);
        let package_line = existing.and_then(|file| {
            file.content
                .lines()
                .find(|line| line.starts_with("package "))
                .map(str::to_string)
        });
        // Appending used to hand the polish pass the whole file, working
        // step definitions and all, and ask for the whole file back. The
        // model obliged - renaming and reflowing definitions that bind
        // scenarios passing today. Now it only ever sees the definitions
        // being added; everything else is spliced around its reply.
        //
        // The whole-file gate still runs on the assembled result: every
        // pattern the file declared has to come back, as must its package
        // and the class the file is named for, or the module stops
        // compiling.
        let declared = existing
            .map(|file| extract_patterns(self.language, &file.content))
            .unwrap_or_default();
        let class_name = production_type_name(&target);
        let valid_file = |code: &str| {
            looks_like_step_definitions(self.language, code, class_name.as_deref())
                && package_line
                    .as_deref()
                    .map(|package| code.contains(package.trim_end_matches(';')))
                    .unwrap_or(true)
                && {
                    let kept = extract_patterns(self.language, code);
                    declared.iter().all(|pattern| kept.contains(pattern))
                }
        };
        let fragment = existing
            .and_then(|file| step_definitions_fragment(&file.content, self.language, &missing));
        let (content, source) = match (existing, &fragment) {
            (Some(file), Some(fragment)) => {
                let expected = extract_patterns(self.language, fragment);
                self.polish_fragment(
                    prompter,
                    fragment,
                    |code| looks_like_step_fragment(self.language, code, &expected),
                    |members| splice_step_definitions(&file.content, self.language, members),
                    valid_file,
                )
            }
            // Nothing new to add, or a greenfield file: there is no
            // pre-existing content to protect, so the whole-file polish
            // pass this prompt was written for still applies.
            _ => {
                let template = match existing {
                    Some(file) => append_step_definitions(&file.content, self.language, &missing),
                    None => step_definitions_template(self.language, &missing),
                };
                self.polish(prompter, &template, valid_file)
            }
        };
        let verb = if existing.is_some() {
            "append"
        } else {
            "generate"
        };
        let summary = format!(
            "{verb} pending step definitions for {} missing step(s) ({source})",
            missing.len()
        );
        self.store.stage(&target, &content, &summary)?;
        Ok(GenerationReport {
            target,
            staged: true,
            source,
            summary,
            next_step: "Review with spec changes show, apply with spec changes commit, then run spec test (expect RED)."
                .into(),
        })
    }

    /// Stage a failing unit test derived from one requirement's acceptance
    /// criteria (unit_test_create). When a brownfield test class already
    /// exists, the new methods are appended to it instead of writing a
    /// parallel `Req00NTest`.
    pub fn unittest_generate(
        &self,
        prompter: &mut dyn Prompter,
        req_id: &str,
    ) -> Result<GenerationReport, ServiceError> {
        let spec = load_effective_spec(&self.spec, &self.store)?;
        let requirement = find_requirement(&spec, req_id)?;
        let sources = self
            .sources
            .sources(crate::domain::steps::source_extension(self.language))?;
        let target = unit_test_path(&sources, self.language, req_id, &self.layout);
        let conventional = unit_test_target_path(self.language, req_id);
        let existing = sources.iter().find(|file| file.path == target);
        let append = existing.is_some() && target != conventional;
        let production = production_path(&sources, self.language, &spec.project, &self.layout);
        let production_type = production_type_name(&production);
        let package_line = existing.filter(|_| append).and_then(|file| {
            file.content
                .lines()
                .find(|line| line.starts_with("package "))
                .map(str::to_string)
        });
        let valid_file = |code: &str| {
            if append {
                looks_like_unit_test_for(self.language, code, production_type.as_deref())
                    && package_line
                        .as_deref()
                        .map(|pkg| code.contains(pkg.trim_end_matches(';')))
                        .unwrap_or(true)
            } else {
                looks_like_unit_test(self.language, code)
            }
        };
        let (content, source) = match existing.filter(|_| append) {
            // Brownfield: the model sees only the test methods being
            // added, never the class they join.
            Some(file) => {
                let fragment = unit_test_fragment(self.language, requirement);
                let placeholders = todo_placeholders(&fragment);
                let cases = requirement.acceptance_criteria.len();
                self.polish_fragment(
                    prompter,
                    &fragment,
                    |code| looks_like_unit_test_fragment(self.language, code, &placeholders, cases),
                    |members| splice_unit_tests(&file.content, self.language, members),
                    valid_file,
                )
            }
            // Greenfield: no pre-existing content to protect, so the
            // whole-file polish pass still applies.
            None => {
                let template = unit_test_template(self.language, requirement);
                self.polish(prompter, &template, valid_file)
            }
        };
        let summary = format!(
            "generate failing unit test for {req_id} ({} criteria, {source})",
            requirement.acceptance_criteria.len()
        );
        self.store.stage(&target, &content, &summary)?;
        Ok(GenerationReport {
            target,
            staged: true,
            source,
            summary,
            next_step: "Review the assertions (they are yours to sharpen), apply with spec changes commit, then run spec test (expect RED)."
                .into(),
        })
    }

    /// The hybrid pass: prefer validated LLM output, fall back to the
    /// template silently on any failure.
    fn polish(
        &self,
        prompter: &mut dyn Prompter,
        template: &str,
        valid: impl Fn(&str) -> bool,
    ) -> (String, String) {
        let Some(llm) = &self.llm else {
            return (template.to_string(), "template".into());
        };
        let prompt = polish_prompt(self.language, template);
        match llm.ask(
            prompter,
            &prompt,
            |response| {
                let code = strip_code_fences(response);
                if valid(&code) {
                    Ok(code)
                } else {
                    Err("the reply was not a valid file for this language".into())
                }
            },
            |_, _, _| {},
        ) {
            // `strip_code_fences` leaves whatever the model ended on, and it
            // often ends on the closing brace. These are source files; give
            // them the final newline every tool expects.
            Ok(code) => (ending_in_newline(code), "llm".into()),
            Err(_) => (template.to_string(), "template".into()),
        }
    }

    /// The append-path polish pass: show the model only the members it
    /// just generated, then splice its reply back in deterministically.
    ///
    /// The file being appended to never enters the context, so the code
    /// already in it cannot be renamed, reflowed, or dropped - the bytes
    /// outside the splice point are carried over by `splice`, not
    /// retyped by a model. `assemble` re-checks the whole file anyway:
    /// the fragment gate makes that redundant, which is exactly what you
    /// want from a second line of defence.
    fn polish_fragment(
        &self,
        prompter: &mut dyn Prompter,
        fragment: &str,
        valid_fragment: impl Fn(&str) -> bool,
        splice: impl Fn(&str) -> String,
        valid_file: impl Fn(&str) -> bool,
    ) -> (String, String) {
        let template = splice(fragment);
        let Some(llm) = &self.llm else {
            return (ending_in_newline(template), "template".into());
        };
        let prompt = polish_fragment_prompt(self.language, fragment, count_members(fragment));
        match llm.ask(
            prompter,
            &prompt,
            |response| {
                let code = strip_code_fences(response);
                if valid_fragment(&code) {
                    Ok(code)
                } else {
                    Err("the reply was not the set of members that were asked for".into())
                }
            },
            |_, _, _| {},
        ) {
            Ok(polished) => {
                let assembled = splice(&shaped_like(fragment, &polished));
                if valid_file(&assembled) {
                    (ending_in_newline(assembled), "llm".into())
                } else {
                    (ending_in_newline(template), "template".into())
                }
            }
            Err(_) => (ending_in_newline(template), "template".into()),
        }
    }
}

/// How many members a generated fragment holds, for the prompt's count.
/// Definitions are joined with a blank line, so counting the annotation
/// or `function` markers is close enough to keep the model honest.
fn count_members(fragment: &str) -> usize {
    let markers = [
        "@Given", "@When", "@Then", "@Test", "[Given(", "[When(", "[Then(", "[Fact]",
    ];
    let counted: usize = markers
        .iter()
        .map(|marker| fragment.matches(marker).count())
        .sum();
    counted.max(1)
}

/// Give a polished fragment the whitespace shape the splice expects.
///
/// `strip_code_fences` trims the reply, which costs the first member its
/// indentation and the last one its newline - both of which the
/// deterministic template carries and the splice point relies on.
fn shaped_like(template: &str, polished: &str) -> String {
    let indent: String = template
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let mut shaped = String::new();
    if !polished.starts_with([' ', '\t']) {
        shaped.push_str(&indent);
    }
    shaped.push_str(polished);
    ending_in_newline(shaped)
}

fn ending_in_newline(mut code: String) -> String {
    if !code.ends_with('\n') {
        code.push('\n');
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::Spec;
    use crate::ports::StagedChange;
    use crate::test_support::{
        FailingSources, FakeLlm, FakeSources, InMemoryChangeStore, InMemoryFeatureCatalog,
        InMemorySpecRepository, calculator_catalog, calculator_spec, flat_layout,
    };

    fn service(
        sources: Vec<crate::ports::SourceFile>,
        llm: Option<ResolvedLlm<FakeLlm>>,
    ) -> GenerationService<
        InMemoryFeatureCatalog,
        FakeSources,
        InMemoryChangeStore,
        InMemorySpecRepository,
        FakeLlm,
    > {
        GenerationService::new(
            calculator_catalog(),
            FakeSources(sources),
            InMemoryChangeStore::default(),
            InMemorySpecRepository(Ok(calculator_spec())),
            Language::Java,
            flat_layout(Language::Java),
            llm,
        )
    }

    fn defined(patterns: &[&str]) -> Vec<crate::ports::SourceFile> {
        let body: String = patterns
            .iter()
            .map(|p| format!("@Given(\"{p}\")\npublic void step() {{}}\n"))
            .collect();
        vec![crate::ports::SourceFile {
            path: "src/test/java/Steps.java".into(),
            content: body,
        }]
    }

    fn staged(
        service: &GenerationService<
            InMemoryFeatureCatalog,
            FakeSources,
            InMemoryChangeStore,
            InMemorySpecRepository,
            FakeLlm,
        >,
    ) -> Vec<StagedChange> {
        service.store.changes().unwrap()
    }

    #[test]
    fn undefined_steps_are_reported_with_the_framework() {
        let report = service(vec![], None).steps_missing().unwrap();
        assert_eq!(report.language, "Java");
        assert_eq!(report.framework, "Cucumber-JVM");
        assert_eq!(report.missing.len(), 3);
        assert_eq!(
            report.next_step,
            "3 step(s) have no definition. Run spec steps generate to stage pending definitions for them."
        );
    }

    #[test]
    fn fully_defined_features_report_nothing_missing() {
        let sources = defined(&[
            "a calculator",
            "add is called with {string}",
            "the result is {int}",
        ]);
        let report = service(sources, None).steps_missing().unwrap();
        assert_eq!(report.missing, vec![]);
        assert_eq!(
            report.next_step,
            "Every step has a definition. Run spec test to execute the suite."
        );
    }

    #[test]
    fn generate_without_a_model_stages_the_template() {
        let service = service(vec![], None);
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "template");
        assert_eq!(report.target, "src/test/java/GeneratedSteps.java");
        assert!(report.staged);
        let changes = staged(&service);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "src/test/java/GeneratedSteps.java");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(content.contains("@Given(\"a calculator\")"));
        assert!(content.contains("PendingException"));
    }

    #[test]
    fn generate_with_nothing_missing_is_refused() {
        let sources = defined(&[
            "a calculator",
            "add is called with {string}",
            "the result is {int}",
        ]);
        let error = service(sources, None)
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap_err();
        assert_eq!(
            error.0,
            "Every step already has a definition - nothing to generate."
        );
    }

    #[test]
    fn validated_llm_output_replaces_the_template() {
        let reply = "public class GeneratedSteps {\n    @Given(\"a calculator\") public void polished() {}\n}";
        let llm = FakeLlm::replying(&format!("```java\n{reply}\n```"));
        let service = service(vec![], Some(llm));
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "llm");
        let content = service.store.content(&report.target).unwrap().unwrap();
        // The model's reply, given the final newline a source file needs.
        assert_eq!(content, format!("{reply}\n"));
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(
            prompts[0].contains("Cucumber-JVM"),
            "prompt names the framework"
        );
        assert!(
            prompts[0].contains("@Given(\"a calculator\")"),
            "prompt carries the template"
        );
        assert!(
            prompts[0].contains("Java best practices to follow:")
                && prompts[0].contains("Package names are lowercase"),
            "prompt pins the language's best practices"
        );
    }

    /// A file that already binds one scenario, as the append path finds it.
    const BROWNFIELD_STEPS: &str = "package com.example;\n\n\
         public class Steps {\n\
         \x20   @Given(\"a calculator\")\n\
         \x20   public void aCalculator() {}\n\
         }\n";

    fn brownfield_sources() -> Vec<crate::ports::SourceFile> {
        vec![crate::ports::SourceFile {
            path: "src/test/java/Steps.java".into(),
            content: BROWNFIELD_STEPS.into(),
        }]
    }

    /// The two definitions `calculator_catalog` leaves missing, renamed
    /// the way a polish pass legitimately may.
    const POLISHED_FRAGMENT: &str = "    @When(\"add is called with {string}\")\n\
         \x20   public void addIsCalledWith(String numbers) {\n\
         \x20       throw new PendingException();\n\
         \x20   }\n\n\
         \x20   @Then(\"the result is {int}\")\n\
         \x20   public void theResultIs(int expected) {\n\
         \x20       throw new PendingException();\n\
         \x20   }\n";

    #[test]
    fn appending_polishes_only_the_new_definitions_and_never_sends_the_file() {
        // The whole point of the fragment pass: the file being appended
        // to is not in the context, so the definition already binding a
        // passing scenario cannot be renamed however the model replies.
        let service = service(
            brownfield_sources(),
            Some(FakeLlm::replying(POLISHED_FRAGMENT)),
        );
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "llm");
        let content = service.store.content(&report.target).unwrap().unwrap();

        // The result is the untouched file with the reply spliced in -
        // byte for byte, not a model's retyping of it.
        assert_eq!(
            content,
            splice_step_definitions(BROWNFIELD_STEPS, Language::Java, POLISHED_FRAGMENT)
        );
        assert!(
            content.contains("public void aCalculator() {}"),
            "{content}"
        );
        assert!(content.contains("addIsCalledWith"), "{content}");
        assert!(content.ends_with('\n'), "trailing newline: {content:?}");

        // And the file's own body was never shown to the model.
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        let prompt = prompts.first().expect("one polish call");
        assert!(
            !prompt.contains("aCalculator"),
            "the existing method leaked into the prompt: {prompt}"
        );
        assert!(
            !prompt.contains("package com.example"),
            "the existing package leaked into the prompt: {prompt}"
        );
        assert!(
            prompt.contains("add is called with {string}"),
            "the generated members are missing: {prompt}"
        );
    }

    #[test]
    fn a_fragment_reply_that_alters_a_step_expression_falls_back_to_the_template() {
        // Renaming methods is the job; editing the cucumber expression
        // un-binds the scenario the definition was generated for.
        let altered = "    @When(\"add is called with {word}\")\n\
             \x20   public void addIsCalledWith(String numbers) {}\n\n\
             \x20   @Then(\"the result is {int}\")\n\
             \x20   public void theResultIs(int expected) {}\n";
        let service = service(brownfield_sources(), Some(FakeLlm::replying(altered)));
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "template");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(
            content.contains("@When(\"add is called with {string}\")"),
            "{content}"
        );
        assert!(!content.contains("{word}"), "{content}");
        assert!(
            content.contains("public void aCalculator() {}"),
            "{content}"
        );
    }

    #[test]
    fn a_fragment_reply_that_drops_a_generated_definition_falls_back_to_the_template() {
        let dropped = "    @When(\"add is called with {string}\")\n\
             \x20   public void addIsCalledWith(String numbers) {}\n";
        let service = service(brownfield_sources(), Some(FakeLlm::replying(dropped)));
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "template");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(
            content.contains("@Then(\"the result is {int}\")"),
            "the dropped definition is back from the template: {content}"
        );
    }

    #[test]
    fn a_fragment_reply_that_rewrites_the_whole_file_is_refused() {
        // This is the pre-fix behaviour, now a rejection: asked for
        // members, the model returns the file with its own class and a
        // renamed `aCalculator`. Splicing that would nest a class inside
        // a class, so the template stands instead.
        let whole_file = "package com.example;\n\npublic class Steps {\n\
             \x20   @Given(\"a calculator\")\n    public void calculatorIsReady() {}\n\
             \x20   @When(\"add is called with {string}\")\n    public void when(String s) {}\n\
             \x20   @Then(\"the result is {int}\")\n    public void then(int n) {}\n}\n";
        let service = service(brownfield_sources(), Some(FakeLlm::replying(whole_file)));
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "template");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(
            content.contains("public void aCalculator() {}"),
            "the original method name survived the rewrite attempt: {content}"
        );
        assert!(
            !content.contains("calculatorIsReady"),
            "the model's rename leaked in: {content}"
        );
        assert_eq!(
            content.matches("public class Steps").count(),
            1,
            "{content}"
        );
    }

    #[test]
    fn a_greenfield_file_still_goes_through_the_whole_file_polish() {
        // Nothing exists to protect, so the model is still handed - and
        // still owes - a complete file. A bare fragment is refused here
        // precisely because the class declaration is required.
        let fragment = service(vec![], Some(FakeLlm::replying(POLISHED_FRAGMENT)));
        let report = fragment
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "template");

        let prompts = fragment.llm.as_ref().unwrap().chat().prompts.borrow();
        let prompt = prompts.first().expect("one polish call");
        assert!(
            prompt.contains("complete file content"),
            "greenfield should use the whole-file prompt: {prompt}"
        );
    }

    #[test]
    fn invalid_llm_output_falls_back_to_the_template_silently() {
        let service = service(vec![], Some(FakeLlm::replying("I cannot help with that.")));
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "template");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(content.contains("PendingException"));
    }

    #[test]
    fn an_llm_failure_falls_back_to_the_template_silently() {
        let service = service(vec![], Some(FakeLlm::failing()));
        let report = service
            .steps_generate(&mut crate::application::agent_service::NullPrompter)
            .unwrap();
        assert_eq!(report.source, "template");
    }

    #[test]
    fn a_unit_test_is_staged_from_the_requirements_criteria() {
        let service = service(vec![], None);
        let report = service
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-001",
            )
            .unwrap();
        assert_eq!(report.target, "src/test/java/Req001Test.java");
        assert_eq!(report.source, "template");
        assert!(report.summary.contains("1 criteria"));
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(content.contains("Generated from REQ-001: Adds two numbers"));
        assert!(content.contains("fail(\"TODO: assert -"));
    }

    #[test]
    fn a_unit_test_for_an_unknown_requirement_is_refused() {
        let error = service(vec![], None)
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-999",
            )
            .unwrap_err();
        assert_eq!(
            error.0,
            "No requirement with id REQ-999. Call spec list to see valid ids."
        );
    }

    #[test]
    fn validated_llm_output_replaces_the_unit_test_template() {
        let llm = FakeLlm::replying("@Test void polished() {}");
        let service = service(vec![], Some(llm));
        let report = service
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-001",
            )
            .unwrap();
        assert_eq!(report.source, "llm");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert_eq!(content, "@Test void polished() {}\n");
    }

    #[test]
    fn a_brownfield_unit_test_is_appended_not_written_in_parallel() {
        let sources = vec![crate::ports::SourceFile {
            path: "src/test/java/com/example/StringCalculatorTest.java".into(),
            content: "package com.example;\n\nclass StringCalculatorTest {\n}\n".into(),
        }];
        let service = service(sources, None);
        let report = service
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-001",
            )
            .unwrap();
        assert_eq!(
            report.target,
            "src/test/java/com/example/StringCalculatorTest.java"
        );
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(content.contains("package com.example;"));
        assert!(content.contains("class StringCalculatorTest"));
        assert!(content.contains("REQ-001"));
        assert!(content.contains("fail(\"TODO: assert -"));
        assert!(!content.contains("class Req001Test"));
    }

    #[test]
    fn llm_polish_that_renames_the_package_falls_back_to_the_template() {
        let sources = vec![crate::ports::SourceFile {
            path: "src/test/java/com/example/StringCalculatorTest.java".into(),
            content: "package com.example;\n\nclass StringCalculatorTest {\n    private final StringCalculator calculator = new StringCalculator();\n}\n".into(),
        }];
        let llm = FakeLlm::replying(
            "package com.wrong;\n@Test void two() { fail(\"TODO\"); new StringCalculator(); }",
        );
        let service = service(sources, Some(llm));
        let report = service
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-001",
            )
            .unwrap();
        assert_eq!(report.source, "template");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(content.contains("package com.example;"));
        assert!(!content.contains("package com.wrong;"));
    }

    /// A brownfield test class with a field the polish pass must not lose.
    const BROWNFIELD_TEST: &str = "package com.example;\n\n\
         class StringCalculatorTest {\n\
         \x20   private final StringCalculator calculator = new StringCalculator();\n\
         }\n";

    fn brownfield_test_sources() -> Vec<crate::ports::SourceFile> {
        vec![crate::ports::SourceFile {
            path: "src/test/java/com/example/StringCalculatorTest.java".into(),
            content: BROWNFIELD_TEST.into(),
        }]
    }

    #[test]
    fn appending_a_unit_test_polishes_only_the_new_methods() {
        // REQ-001 has one criterion, so one test case with one TODO.
        let polished = "    @Test\n    @DisplayName(\"REQ-001: Given \\\"1,2\\\", when add is called, then the result is 3\")\n\
             \x20   void addsOneAndTwo() {\n\
             \x20       fail(\"TODO: assert - Given \\\"1,2\\\", when add is called, then the result is 3\");\n\
             \x20   }\n";
        let service = service(brownfield_test_sources(), Some(FakeLlm::replying(polished)));
        let report = service
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-001",
            )
            .unwrap();
        assert_eq!(report.source, "llm");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(content.contains("addsOneAndTwo"), "{content}");
        assert!(
            content.contains("private final StringCalculator calculator"),
            "the existing field survived: {content}"
        );
        assert!(content.ends_with('\n'), "trailing newline: {content:?}");

        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        let prompt = prompts.first().expect("one polish call");
        assert!(
            !prompt.contains("private final StringCalculator calculator"),
            "the existing class body leaked into the prompt: {prompt}"
        );
    }

    #[test]
    fn a_unit_test_reply_that_resolves_the_todo_falls_back_to_the_template() {
        // Sharpening the assertion is the developer's exercise. A reply
        // that writes it has taken the RED bar away.
        let resolved = "    @Test\n    void addsOneAndTwo() {\n\
             \x20       assertEquals(3, calculator.add(\"1,2\"));\n    }\n";
        let service = service(brownfield_test_sources(), Some(FakeLlm::replying(resolved)));
        let report = service
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-001",
            )
            .unwrap();
        assert_eq!(report.source, "template");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert!(content.contains("fail(\"TODO: assert -"), "{content}");
        assert!(!content.contains("assertEquals(3"), "{content}");
    }

    #[test]
    fn a_unit_test_reply_carrying_its_own_class_is_refused() {
        let whole_file = "package com.example;\n\nclass StringCalculatorTest {\n\
             \x20   @Test void addsOneAndTwo() { fail(\"TODO: assert - x\"); }\n}\n";
        let service = service(
            brownfield_test_sources(),
            Some(FakeLlm::replying(whole_file)),
        );
        let report = service
            .unittest_generate(
                &mut crate::application::agent_service::NullPrompter,
                "REQ-001",
            )
            .unwrap();
        assert_eq!(report.source, "template");
        let content = service.store.content(&report.target).unwrap().unwrap();
        assert_eq!(
            content.matches("class StringCalculatorTest").count(),
            1,
            "{content}"
        );
        assert!(
            content.contains("private final StringCalculator calculator"),
            "{content}"
        );
    }

    #[test]
    fn source_scan_failures_become_service_errors() {
        let service: GenerationService<_, _, InMemoryChangeStore, InMemorySpecRepository, FakeLlm> =
            GenerationService::new(
                calculator_catalog(),
                FailingSources,
                InMemoryChangeStore::default(),
                InMemorySpecRepository(Ok(Spec::default())),
                Language::Java,
                flat_layout(Language::Java),
                None,
            );
        assert_eq!(service.steps_missing().unwrap_err().0, "disk on fire");
    }
}
