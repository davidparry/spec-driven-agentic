Feature: The tool-calling agent loop
  As a developer running an LLM-backed bdd command
  I want the model to look things up through its profile and retry invalid answers
  So that a tool result can change the answer without escaping the command's tools

  Scenario: The loop narrates each call before it runs
    Given the agent may use "list_requirements"
    And the tool "list_requirements" returns "REQ-001 is pending"
    And the model will call "list_requirements"
    And then the model will answer "work on REQ-001"
    When the agent is asked "what is next"
    Then the agent was told a line containing "list_requirements"
    And the agent answer is "work on REQ-001"

  Scenario: A tool result changes the answer
    Given the agent may use "list_requirements"
    And the tool "list_requirements" returns "only REQ-007 is pending"
    And the model will call "list_requirements"
    And then the model will answer "draft nothing; REQ-007 is next"
    When the agent is asked "what is next"
    Then the agent answer is "draft nothing; REQ-007 is next"

  Scenario: An invalid answer is retried with the correction snippet
    Given the model will answer "Sure!"
    And then the model will answer "a real answer"
    When the agent is asked "what is next" requiring a non-empty non-sure reply
    Then the agent answer is "a real answer"

  Scenario: Exhausted attempts report after N attempts
    Given the model will answer "Sure!"
    And then the model will answer "Still sure!"
    And the agent allows 2 attempts
    When asking the agent "what is next" requiring a non-empty non-sure reply fails
    Then the agent error contains "after 2 attempts"

  Scenario: Exhausted max_rounds says so
    Given the agent may use "list_requirements"
    And the tool "list_requirements" returns "ok"
    And the model will call "list_requirements"
    And then the model will call "list_requirements"
    And the agent allows 1 tool round
    When asking the agent "what is next" fails
    Then the agent error contains "kept calling tools"

  Scenario: An unknown tool is recovered
    Given the agent may use "list_requirements"
    And the model will call "nope_tool"
    And then the model will answer "I looked it up"
    When the agent is asked "what is next"
    Then the agent was told a line containing "nope_tool"
    And the agent answer is "I looked it up"

  Scenario: command_run is confirmed
    Given the agent may use "command_run"
    And command_run requires confirmation
    And the developer will confirm
    And the tool "command_run" returns "compiled"
    And the model will call "command_run"
    And then the model will answer "ready"
    When the agent is asked "implement it"
    Then the agent answer is "ready"

  Scenario: command_run is declined
    Given the agent may use "command_run"
    And command_run requires confirmation
    And the developer will decline
    And the model will call "command_run"
    And then the model will answer "skipped"
    When the agent is asked "implement it"
    Then the agent was told a line containing "command_run"
    And the agent answer is "skipped"

  Scenario: A tool failure is fed back, not fatal
    Given the agent may use "list_requirements"
    And the tool "list_requirements" fails with "disk on fire"
    And the model will call "list_requirements"
    And then the model will answer "cannot list right now"
    When the agent is asked "what is next"
    Then the agent answer is "cannot list right now"

  Scenario: A transport failure is reported as a call error
    Given the model call will fail with "ollama down"
    When asking the agent "what is next" fails
    Then the agent error contains "ollama down"

  Scenario: An oversized tool reply is truncated with a marker
    Given the agent may use "list_requirements"
    And the tool "list_requirements" returns a reply larger than the model cap
    And the model will call "list_requirements"
    And then the model will answer "trimmed"
    When the agent is asked "what is next"
    Then the agent answer is "trimmed"

  Scenario: Without a model the template fallback still reports source template
    Given a Java project marker
    And a project feature file "features/calc.feature" containing:
      """
      Feature: String calculator

        Scenario: Adds two numbers
          Given a calculator
          When add is called with "1,2"
          Then the result is 3
      """
    When step definitions are generated without a model
    Then the generation is staged at "src/test/java/GeneratedSteps.java" from "template"

  Scenario: The model is never offered a tool outside its caller's profile
    Given the agent may use "get_tdd_state, validate_spec"
    And the model will answer "stay on GREEN"
    When the agent is asked "what is next"
    Then the model was offered only "get_tdd_state, validate_spec"
