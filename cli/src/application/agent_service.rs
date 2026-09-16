//! The tool-calling agent loop: drop-in for `generate_valid`.

use crate::application::{LlmReplyError, chat_logged};
use crate::domain::generation::strip_think_block;
use crate::domain::prompts::{RenderedPrompt, correction_user};
use crate::domain::tools::{
    ChatMessage, TOOL_REPLY_CAP, ToolDefinition, arguments_object, confirm_question,
    declined_reply, find, narrate_call, truncate_for_model, unknown_tool_reply,
};
use crate::ports::{LlmConversation, PromptError, Prompter, ToolBroker, ToolError};

pub const DEFAULT_MAX_ROUNDS: u32 = 12;

/// A broker that never successfully invokes a tool. Used by tests and by
/// callers whose profile is empty.
pub struct NullBroker;

impl ToolBroker for NullBroker {
    fn call(
        &self,
        name: &str,
        _arguments: &serde_json::Value,
    ) -> Result<crate::domain::tools::ToolOutcome, ToolError> {
        Err(ToolError(format!("no tools available ({name})")))
    }
}

/// A prompter for unattended / MCP paths: tells are dropped, confirms
/// decline, asks fail. Piped CI never hangs.
pub struct NullPrompter;

impl Prompter for NullPrompter {
    fn tell(&mut self, _message: &str) {}
    fn ask(&mut self, _question: &str) -> Result<String, PromptError> {
        Err(PromptError("no human is attached".into()))
    }
    fn confirm(&mut self, _question: &str) -> Result<bool, PromptError> {
        Ok(false)
    }
}

pub struct Agent<C: LlmConversation, B: ToolBroker> {
    model: String,
    chat: C,
    broker: B,
    tools: Vec<ToolDefinition>,
    section: &'static str,
    attempts: u32,
    max_rounds: u32,
    confirm: Vec<String>,
}

/// Policy knobs for [`Agent::new`] — kept off the constructor so clippy
/// does not treat every wiring site as an 8-argument function.
pub struct AgentConfig {
    pub section: &'static str,
    pub attempts: u32,
    pub max_rounds: u32,
    pub confirm: Vec<String>,
}

impl AgentConfig {
    pub fn new(
        section: &'static str,
        attempts: u32,
        max_rounds: u32,
        confirm: impl Into<Vec<String>>,
    ) -> Self {
        Self {
            section,
            attempts: attempts.max(1),
            max_rounds: max_rounds.max(1),
            confirm: confirm.into(),
        }
    }
}

impl<C: LlmConversation, B: ToolBroker> Agent<C, B> {
    pub fn new(
        model: impl Into<String>,
        chat: C,
        broker: B,
        tools: Vec<ToolDefinition>,
        config: AgentConfig,
    ) -> Self {
        Self {
            model: model.into(),
            chat,
            broker,
            tools,
            section: config.section,
            attempts: config.attempts,
            max_rounds: config.max_rounds,
            confirm: config.confirm,
        }
    }

    #[cfg(test)]
    pub(crate) fn chat(&self) -> &C {
        &self.chat
    }

    pub fn ask<T>(
        &self,
        prompter: &mut dyn Prompter,
        prompt: &RenderedPrompt,
        parse: impl Fn(&str) -> Result<T, String>,
        mut on_retry: impl FnMut(u32, u32, &str),
    ) -> Result<T, LlmReplyError> {
        let mut messages = vec![
            ChatMessage::system(prompt.system.clone()),
            ChatMessage::user(prompt.user.clone()),
        ];
        let mut answers = 0u32;
        let names: Vec<String> = self.tools.iter().map(|t| t.name.clone()).collect();
        for _round in 1..=self.max_rounds {
            let turn = chat_logged(
                &self.chat,
                &self.model,
                self.section,
                &messages,
                &self.tools,
            )
            .map_err(LlmReplyError::Call)?;
            if turn.tool_calls.is_empty() {
                match parse(&strip_think_block(&turn.content)) {
                    Ok(value) => return Ok(value),
                    Err(reason) => {
                        answers += 1;
                        if answers >= self.attempts {
                            return Err(LlmReplyError::Invalid {
                                reason: format!("{reason} (after {answers} attempts)"),
                            });
                        }
                        on_retry(answers + 1, self.attempts, &reason);
                        messages.push(ChatMessage::assistant(turn.content.clone(), Vec::new()));
                        messages.push(ChatMessage::user(correction_user(&reason, &turn.content)));
                        continue;
                    }
                }
            }
            messages.push(ChatMessage::assistant(
                turn.content.clone(),
                turn.tool_calls.clone(),
            ));
            if !turn.content.trim().is_empty() {
                prompter.tell(&turn.content);
            }
            for call in &turn.tool_calls {
                let args = arguments_object(&call.arguments);
                prompter.tell(&narrate_call(&call.name, &args));
                let reply = match find(&self.tools, &call.name) {
                    Err(_) => unknown_tool_reply(&call.name, &names),
                    Ok(definition) if self.confirm.iter().any(|n| n == &definition.name) => {
                        match prompter.confirm(&confirm_question(&definition.name, &args)) {
                            Ok(true) => self.invoke(definition, &args, prompter),
                            _ => declined_reply(&definition.name),
                        }
                    }
                    Ok(definition) => self.invoke(definition, &args, prompter),
                };
                messages.push(ChatMessage::tool(
                    &call.name,
                    truncate_for_model(&reply, TOOL_REPLY_CAP),
                ));
            }
        }
        Err(LlmReplyError::Invalid {
            reason: format!(
                "the model kept calling tools for {} rounds without an answer",
                self.max_rounds
            ),
        })
    }

    fn invoke(
        &self,
        definition: &ToolDefinition,
        args: &serde_json::Value,
        prompter: &mut dyn Prompter,
    ) -> String {
        let _work = prompter.working(&format!("calling {}", definition.name));
        match self.broker.call(&definition.name, args) {
            Ok(outcome) => {
                if outcome.is_error {
                    format!("tool error: {}", outcome.text)
                } else {
                    outcome.text
                }
            }
            Err(ToolError(message)) => format!("tool error: {message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::prompts::RenderedPrompt;
    use crate::domain::tools::{ChatTurn, ToolCall, ToolOrigin, ToolOutcome};
    use crate::ports::{LlmError, PromptError, Working};
    use std::cell::RefCell;

    struct ScriptedConversation {
        turns: RefCell<Vec<Result<ChatTurn, LlmError>>>,
        offered: RefCell<Vec<Vec<String>>>,
    }

    impl LlmConversation for ScriptedConversation {
        fn chat(
            &self,
            _model: &str,
            _messages: &[ChatMessage],
            tools: &[ToolDefinition],
        ) -> Result<ChatTurn, LlmError> {
            self.offered
                .borrow_mut()
                .push(tools.iter().map(|t| t.name.clone()).collect());
            let mut turns = self.turns.borrow_mut();
            if turns.is_empty() {
                return Err(LlmError("script exhausted".into()));
            }
            turns.remove(0)
        }
    }

    #[derive(Default)]
    struct FakeBroker {
        replies: RefCell<std::collections::HashMap<String, Result<ToolOutcome, ToolError>>>,
        calls: RefCell<Vec<(String, serde_json::Value)>>,
    }

    impl ToolBroker for FakeBroker {
        fn call(
            &self,
            name: &str,
            arguments: &serde_json::Value,
        ) -> Result<ToolOutcome, ToolError> {
            self.calls
                .borrow_mut()
                .push((name.to_string(), arguments.clone()));
            self.replies
                .borrow()
                .get(name)
                .cloned()
                .unwrap_or(Ok(ToolOutcome {
                    text: format!("{name}-ok"),
                    is_error: false,
                }))
        }
    }

    struct ScriptedPrompter {
        confirms: RefCell<Vec<Result<bool, PromptError>>>,
        told: RefCell<Vec<String>>,
    }

    impl Prompter for ScriptedPrompter {
        fn tell(&mut self, message: &str) {
            self.told.borrow_mut().push(message.to_string());
        }
        fn ask(&mut self, _question: &str) -> Result<String, PromptError> {
            Ok(String::new())
        }
        fn confirm(&mut self, _question: &str) -> Result<bool, PromptError> {
            let mut confirms = self.confirms.borrow_mut();
            if confirms.is_empty() {
                return Err(PromptError("nobody to ask".into()));
            }
            confirms.remove(0)
        }
        fn working(&mut self, _message: &str) -> Box<dyn Working> {
            Box::new(crate::ports::ToldOnce)
        }
    }

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: String::new(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Builtin,
        }
    }

    fn prompt() -> RenderedPrompt {
        RenderedPrompt {
            section: "next_step".into(),
            system: "advise".into(),
            user: "what next".into(),
        }
    }

    fn parse_ok(text: &str) -> Result<String, String> {
        if text.trim().is_empty() {
            Err("empty".into())
        } else {
            Ok(text.trim().to_string())
        }
    }

    fn make_agent(
        turns: Vec<Result<ChatTurn, LlmError>>,
        broker: FakeBroker,
        confirm: Vec<String>,
        attempts: u32,
        max_rounds: u32,
    ) -> Agent<ScriptedConversation, FakeBroker> {
        Agent::new(
            "m",
            ScriptedConversation {
                turns: RefCell::new(turns),
                offered: RefCell::new(Vec::new()),
            },
            broker,
            vec![tool("get_tdd_state"), tool("command_run")],
            AgentConfig::new("next_step", attempts, max_rounds, confirm),
        )
    }

    fn silent() -> ScriptedPrompter {
        ScriptedPrompter {
            confirms: RefCell::new(Vec::new()),
            told: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn a_text_answer_is_accepted() {
        let agent = make_agent(
            vec![Ok(ChatTurn {
                content: "run bdd test".into(),
                tool_calls: vec![],
            })],
            FakeBroker::default(),
            vec![],
            3,
            12,
        );
        let mut prompter = silent();
        let value = agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap();
        assert_eq!(value, "run bdd test");
    }

    #[test]
    fn one_call_then_an_answer() {
        let agent = make_agent(
            vec![
                Ok(ChatTurn {
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        name: "get_tdd_state".into(),
                        arguments: serde_json::json!({}),
                    }],
                }),
                Ok(ChatTurn {
                    content: "phase is GREEN".into(),
                    tool_calls: vec![],
                }),
            ],
            FakeBroker::default(),
            vec![],
            3,
            12,
        );
        let mut prompter = silent();
        let value = agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap();
        assert_eq!(value, "phase is GREEN");
        assert!(
            prompter
                .told
                .borrow()
                .iter()
                .any(|l| l.contains("get_tdd_state"))
        );
    }

    #[test]
    fn two_calls_in_one_turn_run_in_order() {
        let calls = std::rc::Rc::new(RefCell::new(Vec::new()));
        struct RecBroker(std::rc::Rc<RefCell<Vec<String>>>);
        impl ToolBroker for RecBroker {
            fn call(
                &self,
                name: &str,
                _arguments: &serde_json::Value,
            ) -> Result<ToolOutcome, ToolError> {
                self.0.borrow_mut().push(name.to_string());
                Ok(ToolOutcome {
                    text: "ok".into(),
                    is_error: false,
                })
            }
        }
        let agent = Agent::new(
            "m",
            ScriptedConversation {
                turns: RefCell::new(vec![
                    Ok(ChatTurn {
                        content: String::new(),
                        tool_calls: vec![
                            ToolCall {
                                name: "get_tdd_state".into(),
                                arguments: serde_json::json!({}),
                            },
                            ToolCall {
                                name: "command_run".into(),
                                arguments: serde_json::json!({}),
                            },
                        ],
                    }),
                    Ok(ChatTurn {
                        content: "done".into(),
                        tool_calls: vec![],
                    }),
                ]),
                offered: RefCell::new(Vec::new()),
            },
            RecBroker(calls.clone()),
            vec![tool("get_tdd_state"), tool("command_run")],
            AgentConfig::new("next_step", 3, 12, vec![]),
        );
        let mut prompter = silent();
        agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap();
        assert_eq!(*calls.borrow(), vec!["get_tdd_state", "command_run"]);
    }

    #[test]
    fn unknown_tool_is_fed_back_and_recovered() {
        let agent = make_agent(
            vec![
                Ok(ChatTurn {
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        name: "explode".into(),
                        arguments: serde_json::json!({}),
                    }],
                }),
                Ok(ChatTurn {
                    content: "ok after miss".into(),
                    tool_calls: vec![],
                }),
            ],
            FakeBroker::default(),
            vec![],
            3,
            12,
        );
        let mut prompter = silent();
        let value = agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap();
        assert_eq!(value, "ok after miss");
    }

    #[test]
    fn command_run_is_confirmed_or_declined() {
        let broker = FakeBroker::default();
        let make = |confirm_result: Result<bool, PromptError>| {
            let prompter = ScriptedPrompter {
                confirms: RefCell::new(vec![confirm_result]),
                told: RefCell::new(Vec::new()),
            };
            let agent = Agent::new(
                "m",
                ScriptedConversation {
                    turns: RefCell::new(vec![
                        Ok(ChatTurn {
                            content: String::new(),
                            tool_calls: vec![ToolCall {
                                name: "command_run".into(),
                                arguments: serde_json::json!({"command": ["mvn"]}),
                            }],
                        }),
                        Ok(ChatTurn {
                            content: "after".into(),
                            tool_calls: vec![],
                        }),
                    ]),
                    offered: RefCell::new(Vec::new()),
                },
                FakeBroker::default(),
                vec![tool("command_run")],
                AgentConfig::new("implementation", 3, 12, vec!["command_run".into()]),
            );
            (agent, prompter)
        };
        let (agent, mut prompter) = make(Ok(true));
        agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap();
        let (agent, mut prompter) = make(Ok(false));
        agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap();
        let (agent, mut prompter) = make(Err(PromptError("eof".into())));
        agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap();
        let _ = broker;
    }

    #[test]
    fn invalid_terminal_answer_is_retried_then_accepted() {
        let agent = make_agent(
            vec![
                Ok(ChatTurn {
                    content: String::new(),
                    tool_calls: vec![],
                }),
                Ok(ChatTurn {
                    content: "now valid".into(),
                    tool_calls: vec![],
                }),
            ],
            FakeBroker::default(),
            vec![],
            3,
            12,
        );
        let mut prompter = silent();
        let notices = RefCell::new(Vec::new());
        let value = agent
            .ask(&mut prompter, &prompt(), parse_ok, |attempt, of, reason| {
                notices.borrow_mut().push((attempt, of, reason.to_string()));
            })
            .unwrap();
        assert_eq!(value, "now valid");
        assert_eq!(*notices.borrow(), vec![(2, 3, "empty".into())]);
    }

    #[test]
    fn attempts_exhausted_and_max_rounds_exhausted() {
        let mut prompter = silent();
        let error = make_agent(
            vec![
                Ok(ChatTurn {
                    content: String::new(),
                    tool_calls: vec![],
                }),
                Ok(ChatTurn {
                    content: String::new(),
                    tool_calls: vec![],
                }),
            ],
            FakeBroker::default(),
            vec![],
            2,
            12,
        )
        .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
        .unwrap_err();
        match error {
            LlmReplyError::Invalid { reason } => {
                assert!(reason.contains("after 2 attempts"), "{reason}");
            }
            other => panic!("{other:?}"),
        }

        let agent = make_agent(
            vec![Ok(ChatTurn {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    name: "get_tdd_state".into(),
                    arguments: serde_json::json!({}),
                }],
            })],
            FakeBroker::default(),
            vec![],
            3,
            1,
        );
        let mut prompter = silent();
        let error = agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap_err();
        match error {
            LlmReplyError::Invalid { reason } => {
                assert!(reason.contains("kept calling tools"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_transport_error_is_not_retried() {
        let agent = make_agent(
            vec![Err(LlmError("connection refused".into()))],
            FakeBroker::default(),
            vec![],
            3,
            12,
        );
        let mut prompter = silent();
        let error = agent
            .ask(&mut prompter, &prompt(), parse_ok, |_, _, _| {})
            .unwrap_err();
        match error {
            LlmReplyError::Call(e) => assert_eq!(e.0, "connection refused"),
            other => panic!("{other:?}"),
        }
    }
}
