//! The refactor loop: rewrite the production code, run the suite, and
//! keep the result only if the bar is exactly where it was.
//!
//! Every other model-backed command in this harness stages its work and
//! leaves the judgement to a human reading a diff. This one cannot: the
//! only thing that can tell a refactor from a rewrite is the test suite,
//! and the suite runs against files on disk. So the loop writes, and
//! earns the right to by holding two promises instead - it never writes
//! a test, and a round it cannot get green is restored to the byte.

use serde::Serialize;

use crate::application::LlmReplyError;
use crate::application::assets::{find_requirement, load_effective_spec, production_path};
use crate::application::generation_service::ResolvedLlm;
use crate::application::spec_service::ServiceError;
use crate::domain::language::Language;
use crate::domain::memory::ProjectStructure;
use crate::domain::model::{Requirement, TestRunSummary};
use crate::domain::refactor::{
    RefactorRound, RefactorScope, declared_libraries, is_test_path, manifest_names,
    parse_refactor_updates, refactor_prompt, scope,
};
use crate::domain::steps::source_extension;
use crate::ports::{
    ChangeStore, LlmConversation, Prompter, RunnerError, SourceFiles, SpecRepository, TestFilter,
    TestRunner, ToolBroker,
};

/// What one `spec refactor` run did: how much of its budget it spent,
/// what it changed, and whether any of it survived.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RefactorReport {
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Rounds actually spent, of the configured budget.
    pub rounds: u32,
    pub attempts: u32,
    /// The production files whose content changed and stayed changed.
    pub targets: Vec<String>,
    pub tests: u32,
    /// Whether a green refactor is now in the working tree.
    pub applied: bool,
    /// Whether the code was restored to what it was before the run.
    pub reverted: bool,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct RefactorService<S, C, R, L, B = crate::application::agent_service::NullBroker>
where
    S: SourceFiles,
    C: ChangeStore,
    R: SpecRepository,
    L: LlmConversation,
    B: ToolBroker,
{
    sources: S,
    store: C,
    spec: R,
    language: Language,
    layout: ProjectStructure,
    llm: Option<ResolvedLlm<L, B>>,
    attempts: u32,
}

impl<S, C, R, L, B> RefactorService<S, C, R, L, B>
where
    S: SourceFiles,
    C: ChangeStore,
    R: SpecRepository,
    L: LlmConversation,
    B: ToolBroker,
{
    pub fn new(
        sources: S,
        store: C,
        spec: R,
        language: Language,
        layout: ProjectStructure,
        llm: Option<ResolvedLlm<L, B>>,
        attempts: u32,
    ) -> Self {
        Self {
            sources,
            store,
            spec,
            language,
            layout,
            llm,
            attempts: attempts.max(1),
        }
    }

    pub fn has_model(&self) -> bool {
        self.llm.is_some()
    }

    /// Refactor `req_id`'s production code toward `goal`, proving after
    /// every round that the suite is untouched, and restoring the code
    /// if the budget runs out before one lands.
    pub fn run(
        &self,
        prompter: &mut dyn Prompter,
        runner: &dyn TestRunner,
        goal: Option<&str>,
        req_id: Option<&str>,
    ) -> Result<RefactorReport, ServiceError> {
        let Some(llm) = &self.llm else {
            return Err(ServiceError(
                "No model resolved - refactor by hand and rerun spec test.".into(),
            ));
        };
        // The loop commits to apply each round. Anything already staged
        // would ride along on the first commit and then be restored to
        // something it never was, so it is the author's to settle first.
        let staged = self.store.changes()?;
        if !staged.is_empty() {
            return Err(ServiceError(format!(
                "{} change(s) are already staged, and the refactor loop applies each round to \
                 run the tests - review them with spec changes show, then spec changes commit or \
                 spec changes discard before refactoring.",
                staged.len()
            )));
        }
        let spec = load_effective_spec(&self.spec, &self.store)?;
        let requirement: Option<Requirement> = match req_id {
            Some(id) => Some(find_requirement(&spec, id)?.clone()),
            None => None,
        };
        let files = self.project_files()?;
        let focus = production_path(
            &self.sources.sources(source_extension(self.language))?,
            self.language,
            &spec.project,
            &self.layout,
        );
        let manifests = self.manifests()?;
        let scope = scope(&files, &self.layout, &focus, &manifests);
        if scope.writable.is_empty() {
            return Err(ServiceError(format!(
                "There is no production code to refactor - {focus} does not exist yet."
            )));
        }
        let libraries = declared_libraries(self.language, &manifests);
        // The recorded GREEN may predate an edit made since, so the bar
        // is measured now rather than trusted. It is also the count the
        // refactor has to come back with.
        let baseline = self.measure(runner, "Running the suite to fix the bar to beat")?;
        if !baseline.passed() {
            return Err(ServiceError(format!(
                "The suite is not green ({} test(s), {} failure(s), {} error(s)) - a refactor has \
                 nothing to preserve until it is. Fix the bar first.",
                baseline.tests, baseline.failures, baseline.errors
            )));
        }
        let snapshot = scope.writable.clone();
        let writable = scope.writable_paths();
        let mut history: Vec<RefactorRound> = Vec::new();
        let mut scope = scope;
        for round in 1..=self.attempts {
            let prompt = refactor_prompt(
                self.language,
                goal,
                requirement.as_ref(),
                &scope,
                &libraries,
                baseline.tests,
                &history,
            );
            let work = prompter.working(&format!(
                "Refactor round {round} of {} with {} - working",
                self.attempts,
                llm.model()
            ));
            let outcome = llm.ask(
                prompter,
                &prompt,
                |reply| parse_refactor_updates(reply, &writable, &self.layout),
                |_, _, _| {},
            );
            drop(work);
            let updates = match outcome {
                Ok(updates) => updates,
                Err(LlmReplyError::Call(error)) => {
                    let restored = self.restore(&snapshot, &scope.writable)?;
                    return Ok(self.gave_up(
                        goal,
                        round,
                        baseline.tests,
                        restored,
                        format!("The model call failed - {}.", error.0),
                    ));
                }
                Err(LlmReplyError::Invalid { reason }) => {
                    let restored = self.restore(&snapshot, &scope.writable)?;
                    return Ok(self.gave_up(
                        goal,
                        round,
                        baseline.tests,
                        restored,
                        format!("The model never returned a usable refactor ({reason})."),
                    ));
                }
            };
            let changed: Vec<_> = updates
                .into_iter()
                .filter(|update| {
                    !scope
                        .writable
                        .iter()
                        .any(|(path, content)| *path == update.path && *content == update.content)
                })
                .collect();
            if changed.is_empty() {
                // An empty array, or a reply that handed every file back
                // as it found it. Either way the model is saying there is
                // nothing to do, and saying it twice will not change that.
                let reverted = self.restore(&snapshot, &scope.writable)?;
                return Ok(RefactorReport {
                    phase: "REFACTOR".into(),
                    goal: goal.map(str::to_string),
                    rounds: round,
                    attempts: self.attempts,
                    targets: Vec::new(),
                    tests: baseline.tests,
                    applied: false,
                    reverted,
                    source: "llm".into(),
                    warning: Some(if history.is_empty() {
                        "The model found nothing worth refactoring and left the code as it is."
                            .into()
                    } else {
                        "The model ran out of ideas that keep the suite green and left the code \
                         as it was."
                            .into()
                    }),
                    next_step: "Run spec test to confirm the bar, then spec mark-implemented \
                                once the requirement is done."
                        .into(),
                });
            }
            let targets: Vec<String> = changed.iter().map(|u| u.path.clone()).collect();
            let summary = format!(
                "refactor round {round}{}",
                goal.map(|g| format!(": {g}")).unwrap_or_default()
            );
            for update in &changed {
                self.store.stage(&update.path, &update.content, &summary)?;
            }
            self.store.commit()?;
            prompter.tell(&format!("  round {round} rewrote: {}", targets.join(", ")));
            // The model is never handed a test path, and a reply naming
            // one is rejected before it reaches here - but "the tests did
            // not move" is the promise of this command, so it is verified
            // against the bytes rather than inferred from the guard.
            if let Some(moved) = self.tests_that_moved(&scope)? {
                let restored = self.restore(&snapshot, &scope.writable)?;
                return Ok(self.gave_up(
                    goal,
                    round,
                    baseline.tests,
                    restored,
                    format!(
                        "{moved} changed during the refactor, which must never happen - the \
                         production code was restored and nothing was kept."
                    ),
                ));
            }
            let run = self.measure(runner, &format!("Round {round}: running the suite"))?;
            if run.passed() && run.tests == baseline.tests {
                return Ok(RefactorReport {
                    phase: "REFACTOR".into(),
                    goal: goal.map(str::to_string),
                    rounds: round,
                    attempts: self.attempts,
                    targets,
                    tests: run.tests,
                    applied: true,
                    reverted: false,
                    source: "llm".into(),
                    warning: None,
                    next_step: "The refactor is in your working tree and the bar is where it \
                                was. Read it with git diff, then run spec test to record the \
                                run."
                        .into(),
                });
            }
            // A count that moved without a failure is the subtle one: the
            // suite still says green, but it is no longer the same suite,
            // so "green" stopped meaning what it meant at the baseline.
            let why = if run.passed() {
                vec![format!(
                    "the suite ran {} test(s) instead of {} - a refactor may not change how many \
                     tests there are",
                    run.tests, baseline.tests
                )]
            } else {
                run.failure_details.clone()
            };
            history.push(RefactorRound {
                targets: targets.clone(),
                failures: why,
            });
            // The next round reads the code this one wrote, so it can
            // repair its own step instead of starting over blind.
            scope.writable = self.reread(&writable)?;
        }
        let restored = self.restore(&snapshot, &scope.writable)?;
        Ok(self.gave_up(
            goal,
            self.attempts,
            baseline.tests,
            restored,
            format!(
                "{} round(s) each broke the suite, so the code you started with was restored.",
                self.attempts
            ),
        ))
    }

    fn gave_up(
        &self,
        goal: Option<&str>,
        rounds: u32,
        tests: u32,
        reverted: bool,
        warning: String,
    ) -> RefactorReport {
        RefactorReport {
            phase: "REFACTOR".into(),
            goal: goal.map(str::to_string),
            rounds,
            attempts: self.attempts,
            targets: Vec::new(),
            tests,
            applied: false,
            reverted,
            source: "llm".into(),
            warning: Some(warning),
            next_step: "Nothing was kept. Refactor by hand and run spec test, or run spec \
                        refactor again with a smaller --note."
                .into(),
        }
    }

    /// Put every writable file back exactly as it was, and report
    /// whether anything had to move to get there.
    fn restore(
        &self,
        snapshot: &[(String, String)],
        current: &[(String, String)],
    ) -> Result<bool, ServiceError> {
        let mut restored = false;
        for (path, original) in snapshot {
            let now = current
                .iter()
                .find(|(candidate, _)| candidate == path)
                .map(|(_, content)| content.as_str());
            if now == Some(original.as_str()) {
                continue;
            }
            self.store
                .stage(path, original, "restore the code the refactor started from")?;
            restored = true;
        }
        if restored {
            self.store.commit()?;
        }
        Ok(restored)
    }

    /// The read-only file whose bytes no longer match what the loop was
    /// shown, if any.
    fn tests_that_moved(&self, scope: &RefactorScope) -> Result<Option<String>, ServiceError> {
        let now = self.project_files()?;
        Ok(scope
            .readonly
            .iter()
            .filter(|(path, _)| is_test_path(path, &self.layout))
            .find(|(path, before)| {
                now.iter()
                    .find(|(candidate, _)| candidate == path)
                    .is_some_and(|(_, after)| after != before)
            })
            .map(|(path, _)| path.clone()))
    }

    fn reread(&self, writable: &[String]) -> Result<Vec<(String, String)>, ServiceError> {
        let files = self.project_files()?;
        Ok(files
            .into_iter()
            .filter(|(path, _)| writable.iter().any(|wanted| wanted == path))
            .collect())
    }

    fn measure(
        &self,
        runner: &dyn TestRunner,
        _narration: &str,
    ) -> Result<TestRunSummary, ServiceError> {
        runner.run(&TestFilter::default()).map_err(|error| {
            ServiceError(match error {
                RunnerError::RuntimeMissing { runtime, hint } => {
                    format!("{runtime} is not installed - {hint}")
                }
                RunnerError::Failed(message) => message,
            })
        })
    }

    fn project_files(&self) -> Result<Vec<(String, String)>, ServiceError> {
        Ok(self
            .sources
            .sources(source_extension(self.language))?
            .into_iter()
            .map(|file| (file.path, file.content))
            .collect())
    }

    /// The build manifests, read by the extension each one happens to
    /// have - the sources port reads by extension, and a manifest is
    /// just another file to it.
    fn manifests(&self) -> Result<Vec<(String, String)>, ServiceError> {
        let mut found: Vec<(String, String)> = Vec::new();
        for name in manifest_names(self.language) {
            let Some(extension) = name.rsplit('.').next() else {
                continue;
            };
            for file in self.sources.sources(extension)? {
                let file_name = file.path.rsplit('/').next().unwrap_or(&file.path);
                if file_name == *name && !found.iter().any(|(path, _)| *path == file.path) {
                    found.push((file.path, file.content));
                }
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::Spec;
    use crate::ports::{PromptError, SourceError, SourceFile, StageError, StagedChange};
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::rc::Rc;

    const PRODUCTION: &str = "src/main/java/StringCalculator.java";
    const UNIT_TEST: &str = "src/test/java/StringCalculatorTest.java";
    const ORIGINAL: &str = "class StringCalculator { int add(String s) { return 0; } }";
    const CLEANED: &str = "class StringCalculator { static final String COMMA = \",\"; int add(String s) { return 0; } }";
    const BREAKS: &str = "class StringCalculator { int add(String s) { return 1; } }";

    /// A working tree with a staging area in front of it, which is the
    /// pair the loop actually depends on: it stages, commits, and then
    /// expects to read its own write back.
    #[derive(Default)]
    struct Tree {
        applied: RefCell<BTreeMap<String, String>>,
        staged: RefCell<BTreeMap<String, String>>,
        summaries: RefCell<Vec<String>>,
    }

    impl Tree {
        fn holding(files: &[(&str, &str)]) -> Rc<Self> {
            let tree = Self::default();
            for (path, content) in files {
                tree.applied
                    .borrow_mut()
                    .insert((*path).to_string(), (*content).to_string());
            }
            Rc::new(tree)
        }

        /// What the working tree holds, which is only what has been
        /// committed - the staging area in front of it is separate.
        fn on_disk(&self, path: &str) -> String {
            self.applied.borrow().get(path).cloned().unwrap_or_default()
        }
    }

    impl ChangeStore for Rc<Tree> {
        fn stage(
            &self,
            path: &str,
            content: &str,
            summary: &str,
        ) -> Result<StagedChange, StageError> {
            self.staged
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            self.summaries.borrow_mut().push(summary.to_string());
            Ok(StagedChange {
                path: path.into(),
                action: "modify".into(),
                summary: summary.into(),
            })
        }

        fn changes(&self) -> Result<Vec<StagedChange>, StageError> {
            Ok(self
                .staged
                .borrow()
                .keys()
                .map(|path| StagedChange {
                    path: path.clone(),
                    action: "modify".into(),
                    summary: String::new(),
                })
                .collect())
        }

        fn content(&self, path: &str) -> Result<Option<String>, StageError> {
            Ok(self.staged.borrow().get(path).cloned())
        }

        fn commit(&self) -> Result<Vec<StagedChange>, StageError> {
            let changes = self.changes()?;
            let staged = std::mem::take(&mut *self.staged.borrow_mut());
            self.applied.borrow_mut().extend(staged);
            Ok(changes)
        }

        fn discard(&self) -> Result<Vec<StagedChange>, StageError> {
            let changes = self.changes()?;
            self.staged.borrow_mut().clear();
            Ok(changes)
        }
    }

    impl SourceFiles for Rc<Tree> {
        fn sources(&self, extension: &str) -> Result<Vec<SourceFile>, SourceError> {
            Ok(self
                .applied
                .borrow()
                .iter()
                .filter(|(path, _)| path.ends_with(&format!(".{extension}")))
                .map(|(path, content)| SourceFile {
                    path: path.clone(),
                    content: content.clone(),
                })
                .collect())
        }
    }

    /// The suite's verdict is a function of the code in the tree, which
    /// is what makes write-then-test real here rather than scripted.
    struct Suite {
        tree: Rc<Tree>,
        verdict: fn(&str) -> TestRunSummary,
        runs: Cell<u32>,
    }

    impl TestRunner for Suite {
        fn run(&self, _filter: &TestFilter) -> Result<TestRunSummary, RunnerError> {
            self.runs.set(self.runs.get() + 1);
            Ok((self.verdict)(&self.tree.on_disk(PRODUCTION)))
        }
    }

    fn green(tests: u32) -> TestRunSummary {
        TestRunSummary {
            tests,
            failures: 0,
            errors: 0,
            skipped: 0,
            failure_details: Vec::new(),
        }
    }

    fn passes_unless_broken(code: &str) -> TestRunSummary {
        if code.contains("return 1") {
            TestRunSummary {
                tests: 9,
                failures: 1,
                errors: 0,
                skipped: 0,
                failure_details: vec!["StringCalculatorTest.adds: expected 3 but was 1".into()],
            }
        } else {
            green(9)
        }
    }

    fn always_green(_: &str) -> TestRunSummary {
        green(9)
    }

    fn always_red(_: &str) -> TestRunSummary {
        TestRunSummary {
            tests: 9,
            failures: 1,
            errors: 0,
            skipped: 0,
            failure_details: vec!["StringCalculatorTest.adds: expected 3 but was 0".into()],
        }
    }

    /// Green, but at a different count - the refactor shed a test.
    fn fewer_tests_once_cleaned(code: &str) -> TestRunSummary {
        if code.contains("COMMA") {
            green(8)
        } else {
            green(9)
        }
    }

    /// One reply per round, repeating the last once the script runs out.
    struct Replies {
        replies: RefCell<Vec<String>>,
        prompts: RefCell<Vec<String>>,
    }

    impl LlmConversation for Replies {
        fn chat(
            &self,
            _model: &str,
            messages: &[crate::domain::tools::ChatMessage],
            _tools: &[crate::domain::tools::ToolDefinition],
        ) -> Result<crate::domain::tools::ChatTurn, crate::ports::LlmError> {
            let (system, user) = crate::domain::tools::system_and_user(messages);
            self.prompts.borrow_mut().push(format!("{system}\n{user}"));
            let mut replies = self.replies.borrow_mut();
            let reply = if replies.len() > 1 {
                replies.remove(0)
            } else {
                replies.first().cloned().unwrap_or_default()
            };
            Ok(crate::domain::tools::text_turn(reply))
        }
    }

    #[derive(Default)]
    struct Quiet {
        told: Vec<String>,
    }

    impl Prompter for Quiet {
        fn tell(&mut self, message: &str) {
            self.told.push(message.to_string());
        }

        fn ask(&mut self, _question: &str) -> Result<String, PromptError> {
            Err(PromptError::ended("a test never answers"))
        }

        fn confirm(&mut self, _question: &str) -> Result<bool, PromptError> {
            Ok(false)
        }
    }

    fn rewrite(path: &str, content: &str) -> String {
        serde_json::json!([{"path": path, "content": content}]).to_string()
    }

    fn layout() -> ProjectStructure {
        ProjectStructure {
            production: Some("src/main/java".into()),
            tests: Some("src/test/java".into()),
            ..Default::default()
        }
    }

    fn spec() -> Spec {
        Spec {
            project: "StringCalculator".into(),
            requirements: vec![Requirement {
                id: "REQ-003".into(),
                title: "Comma separated numbers are summed".into(),
                status: "implemented".into(),
                story: "As a user, I want comma sums so that totals arrive.".into(),
                acceptance_criteria: vec![
                    "Given \"1,2\", when add is called, then the result is 3".into(),
                ],
                feature_file: None,
            }],
            ..Spec::default()
        }
    }

    type Subject = RefactorService<Rc<Tree>, Rc<Tree>, InMemorySpecRepository, Replies>;

    struct InMemorySpecRepository(Spec);

    impl SpecRepository for InMemorySpecRepository {
        fn load(&self) -> Result<Spec, crate::ports::SpecError> {
            Ok(self.0.clone())
        }
    }

    fn service(tree: &Rc<Tree>, replies: &[&str], attempts: u32) -> Subject {
        RefactorService::new(
            tree.clone(),
            tree.clone(),
            InMemorySpecRepository(spec()),
            Language::Java,
            layout(),
            Some(ResolvedLlm::with_attempts(
                "test-model",
                Replies {
                    replies: RefCell::new(replies.iter().map(|r| r.to_string()).collect()),
                    prompts: RefCell::new(Vec::new()),
                },
                1,
            )),
            attempts,
        )
    }

    fn suite(tree: &Rc<Tree>, verdict: fn(&str) -> TestRunSummary) -> Suite {
        Suite {
            tree: tree.clone(),
            verdict,
            runs: Cell::new(0),
        }
    }

    fn kata() -> Rc<Tree> {
        Tree::holding(&[
            (PRODUCTION, ORIGINAL),
            (UNIT_TEST, "class StringCalculatorTest {}"),
        ])
    }

    #[test]
    fn a_green_refactor_stays_in_the_working_tree() {
        let tree = kata();
        let service = service(&tree, &[&rewrite(PRODUCTION, CLEANED)], 10);
        let suite = suite(&tree, always_green);
        let report = service
            .run(
                &mut Quiet::default(),
                &suite,
                Some("extract comma delimiter constant"),
                Some("REQ-003"),
            )
            .unwrap();
        assert!(report.applied, "report: {report:?}");
        assert!(!report.reverted);
        assert_eq!(report.rounds, 1);
        assert_eq!(report.attempts, 10);
        assert_eq!(report.targets, vec![PRODUCTION]);
        assert_eq!(report.tests, 9);
        assert_eq!(tree.on_disk(PRODUCTION), CLEANED);
        assert_eq!(
            report.goal.as_deref(),
            Some("extract comma delimiter constant")
        );
    }

    #[test]
    fn the_whole_budget_is_spent_then_the_original_code_comes_back_byte_for_byte() {
        let tree = kata();
        // A different wrong answer each round, so the budget is what ends
        // the loop rather than the model repeating itself.
        let service = service(
            &tree,
            &[
                &rewrite(PRODUCTION, BREAKS),
                &rewrite(PRODUCTION, &format!("{BREAKS} // second try")),
                &rewrite(PRODUCTION, &format!("{BREAKS} // third try")),
            ],
            3,
        );
        let suite = suite(&tree, passes_unless_broken);
        let report = service
            .run(&mut Quiet::default(), &suite, Some("tidy up"), None)
            .unwrap();
        assert!(!report.applied, "report: {report:?}");
        assert!(report.reverted);
        assert_eq!(report.rounds, 3);
        assert!(report.targets.is_empty());
        assert!(
            report.warning.as_deref().unwrap().contains("3 round(s)"),
            "warning: {:?}",
            report.warning
        );
        assert_eq!(
            tree.on_disk(PRODUCTION),
            ORIGINAL,
            "the code the author started with has to come back exactly"
        );
    }

    /// A model that answers the same broken thing twice has nothing left
    /// to say, and the budget is better not spent finding that out again.
    #[test]
    fn a_round_that_repeats_the_last_one_ends_the_loop_early_and_still_reverts() {
        let tree = kata();
        let service = service(&tree, &[&rewrite(PRODUCTION, BREAKS)], 10);
        let suite = suite(&tree, passes_unless_broken);
        let report = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap();
        assert!(!report.applied, "report: {report:?}");
        assert!(report.reverted);
        assert_eq!(
            report.rounds, 2,
            "it stops as soon as the reply stops moving"
        );
        assert!(
            report
                .warning
                .as_deref()
                .unwrap()
                .contains("ran out of ideas"),
            "warning: {:?}",
            report.warning
        );
        assert_eq!(tree.on_disk(PRODUCTION), ORIGINAL);
    }

    /// The subtle one: still green, but no longer the same suite, so
    /// green stopped meaning what it meant at the baseline.
    #[test]
    fn a_green_run_at_a_different_test_count_is_not_a_refactor() {
        let tree = kata();
        let service = service(&tree, &[&rewrite(PRODUCTION, CLEANED)], 1);
        let suite = suite(&tree, fewer_tests_once_cleaned);
        let report = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap();
        assert!(!report.applied, "report: {report:?}");
        assert!(report.reverted);
        assert_eq!(tree.on_disk(PRODUCTION), ORIGINAL);
    }

    #[test]
    fn a_reply_that_rewrites_a_test_never_reaches_the_tree() {
        let tree = kata();
        let gutted = rewrite(UNIT_TEST, "class StringCalculatorTest { /* deleted */ }");
        let service = service(&tree, &[&gutted], 2);
        let suite = suite(&tree, always_green);
        let report = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap();
        assert!(!report.applied, "report: {report:?}");
        assert_eq!(
            tree.on_disk(UNIT_TEST),
            "class StringCalculatorTest {}",
            "the test has to be exactly as the author left it"
        );
        assert_eq!(tree.on_disk(PRODUCTION), ORIGINAL);
    }

    #[test]
    fn an_empty_array_means_the_code_is_already_clean_and_ends_the_loop() {
        let tree = kata();
        let service = service(&tree, &["[]"], 10);
        let suite = suite(&tree, always_green);
        let report = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap();
        assert!(!report.applied, "report: {report:?}");
        assert!(!report.reverted, "nothing moved, so nothing to restore");
        assert_eq!(report.rounds, 1, "it does not ask nine more times");
        assert!(
            report
                .warning
                .as_deref()
                .unwrap()
                .contains("nothing worth refactoring"),
            "warning: {:?}",
            report.warning
        );
        assert_eq!(tree.on_disk(PRODUCTION), ORIGINAL);
    }

    #[test]
    fn a_file_handed_back_word_for_word_is_not_a_change_either() {
        let tree = kata();
        let service = service(&tree, &[&rewrite(PRODUCTION, ORIGINAL)], 10);
        let suite = suite(&tree, always_green);
        let report = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap();
        assert!(!report.applied);
        assert_eq!(report.rounds, 1);
        assert!(report.targets.is_empty());
    }

    #[test]
    fn a_red_bar_is_refused_before_the_model_is_asked_anything() {
        let tree = kata();
        let service = service(&tree, &[&rewrite(PRODUCTION, CLEANED)], 10);
        let suite = suite(&tree, always_red);
        let error = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap_err();
        assert!(error.0.contains("not green"), "error: {}", error.0);
        assert_eq!(tree.on_disk(PRODUCTION), ORIGINAL);
        assert!(
            service
                .llm
                .as_ref()
                .unwrap()
                .chat()
                .prompts
                .borrow()
                .is_empty(),
            "a red bar has nothing to preserve, so nothing is asked"
        );
    }

    #[test]
    fn work_already_staged_is_refused_rather_than_swept_into_the_loop() {
        let tree = kata();
        tree.stage("notes.md", "mine", "my own edit").unwrap();
        let service = service(&tree, &[&rewrite(PRODUCTION, CLEANED)], 10);
        let suite = suite(&tree, always_green);
        let error = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap_err();
        assert!(error.0.contains("already staged"), "error: {}", error.0);
        assert_eq!(
            tree.on_disk("notes.md"),
            "",
            "the author's staged work is left staged, not applied"
        );
    }

    #[test]
    fn a_later_round_is_briefed_with_what_the_earlier_one_broke() {
        let tree = kata();
        let service = service(
            &tree,
            &[&rewrite(PRODUCTION, BREAKS), &rewrite(PRODUCTION, CLEANED)],
            3,
        );
        let suite = suite(&tree, passes_unless_broken);
        let report = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap();
        assert!(report.applied, "report: {report:?}");
        assert_eq!(report.rounds, 2);
        assert_eq!(tree.on_disk(PRODUCTION), CLEANED);
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert_eq!(prompts.len(), 2);
        assert!(
            prompts[1].contains("This is round 2"),
            "round 2 prompt: {}",
            prompts[1]
        );
        assert!(
            prompts[1].contains("StringCalculatorTest.adds: expected 3 but was 1"),
            "the failure it caused reaches the next round: {}",
            prompts[1]
        );
        assert!(
            prompts[1].contains("return 1"),
            "and so does the code it wrote: {}",
            prompts[1]
        );
    }

    #[test]
    fn the_prompt_shows_the_tests_as_read_only_and_never_as_writable() {
        let tree = kata();
        let service = service(&tree, &[&rewrite(PRODUCTION, CLEANED)], 1);
        let suite = suite(&tree, always_green);
        service
            .run(&mut Quiet::default(), &suite, None, Some("REQ-003"))
            .unwrap();
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(prompts[0].contains(&format!("--- {UNIT_TEST} (read-only) ---")));
        assert!(
            prompts[0].contains(&format!(
                "Only these paths may appear in your reply: {PRODUCTION}"
            )),
            "prompt: {}",
            prompts[0]
        );
        assert!(prompts[0].contains("You MUST NOT modify a test"));
        assert!(prompts[0].contains("REQ-003"));
    }

    #[test]
    fn without_a_model_the_loop_refuses_instead_of_pretending() {
        let tree = kata();
        let service: Subject = RefactorService::new(
            tree.clone(),
            tree.clone(),
            InMemorySpecRepository(spec()),
            Language::Java,
            layout(),
            None,
            10,
        );
        let suite = suite(&tree, always_green);
        let error = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap_err();
        assert!(error.0.contains("No model resolved"), "error: {}", error.0);
    }

    #[test]
    fn a_budget_of_zero_still_gets_one_round() {
        let tree = kata();
        let service = service(&tree, &[&rewrite(PRODUCTION, CLEANED)], 0);
        let suite = suite(&tree, always_green);
        let report = service
            .run(&mut Quiet::default(), &suite, None, None)
            .unwrap();
        assert_eq!(report.attempts, 1);
        assert!(report.applied, "report: {report:?}");
    }
}
