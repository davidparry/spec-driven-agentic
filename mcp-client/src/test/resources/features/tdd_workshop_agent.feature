Feature: The workshop agent narrates one spec-to-green walkthrough
  The agent harness is the workshop's proof that MCP feeds the workflow tools
  into any client. Its own spec lives in
  mcp-client/requirements/client-requirements.json; these scenarios are that
  spec's executable form, driven through AgentWorkflow against a scripted
  server.

  Background:
    Given a workflow server that identifies as "tdd-workflow-server" version "1.0.0"

  @CLI-001
  Scenario: The handshake narration names the server and quotes its instructions
    Given the server provides the instructions "Drives a spec-driven TDD/BDD workflow."
    When the walkthrough runs
    Then the narration contains "Server identified itself as: tdd-workflow-server v1.0.0"
    And the narration contains "    Drives a spec-driven TDD/BDD workflow."

  @CLI-001
  Scenario: A server without instructions gets no instructions block
    When the walkthrough runs
    Then the narration does not contain "Server instructions for the agent"

  @CLI-002
  Scenario: Discovery narrates each tool with the first sentence of its description
    Given the server exposes a tool "run_tests" described as "Run the kata test suite. Updates the bar color."
    And the server exposes a tool "get_tdd_state" described as "Get the current phase"
    When the walkthrough runs
    Then the narration contains "- run_tests: Run the kata test suite."
    And the narration contains "- get_tdd_state: Get the current phase"

  @CLI-003
  Scenario: The baseline test run announces the bar color
    Given the tool "run_tests" answers with phase "GREEN"
    When the walkthrough runs
    Then the narration contains ">>> The bar is GREEN."

  @CLI-004
  Scenario: A pending requirement is fetched and named in the handoff
    Given the backlog lists "REQ-001" as "implemented" and "REQ-003" as "pending"
    When the walkthrough runs
    Then the narration contains "STEP 6 — tools/call get_requirement (REQ-003)"
    And the narration contains "add a Gherkin scenario for REQ-003"

  @CLI-004
  Scenario: No pending work skips the detail step and uses a placeholder
    Given the backlog lists "REQ-001" as "implemented" and "REQ-002" as "implemented"
    When the walkthrough runs
    Then the narration does not contain "STEP 6"
    And the narration contains "add a Gherkin scenario for the next requirement"

  @CLI-005
  Scenario: An error result is flagged in the narration
    Given the tool "get_tdd_state" answers with an error "TDD state unavailable"
    When the walkthrough runs
    Then the narration contains "Result (isError=true):"
    And the narration contains "    TDD state unavailable"

  @CLI-006
  Scenario: The connection is closed after a completed walkthrough
    When the walkthrough runs
    Then the server connection has been closed

  @CLI-006
  Scenario: The connection is closed even when a tool call blows up
    Given the tool "run_tests" fails catastrophically
    When the walkthrough runs and fails
    Then the server connection has been closed
