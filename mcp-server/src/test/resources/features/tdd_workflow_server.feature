Feature: TDD workflow MCP server
  The server that drives the workshop's spec-driven workflow is itself
  spec-driven: each scenario below is tagged with the requirement it
  implements from requirements/server-requirements.json, and the tools are
  exercised through the same wiring an MCP client invokes.

  Background:
    Given a workflow server backed by a spec with an implemented "REQ-001" and a pending "REQ-002"

  @SRV-001
  Scenario: Listing requirements returns every requirement and its status
    When the agent calls list_requirements
    Then the call succeeds
    And the result mentions "REQ-001" with status "implemented"
    And the result mentions "REQ-002" with status "pending"
    And the result names the project

  @SRV-002
  Scenario: Fetching a requirement returns its criteria and a tagged workflow hint
    When the agent calls get_requirement for "REQ-002"
    Then the call succeeds
    And the result contains the acceptance criteria
    And the result contains a workflow hint naming the tag "@REQ-002"

  @SRV-002
  Scenario: Fetching an unknown requirement is a tool error, not a crash
    When the agent calls get_requirement for "REQ-999"
    Then the call fails as a tool error
    And the error points the agent at "list_requirements"

  @SRV-003
  Scenario: A passing suite turns the bar GREEN
    Given the kata suite passes with 5 tests
    When the agent calls run_tests
    Then the call succeeds
    And the reported phase is "GREEN"

  @SRV-003
  Scenario: A failing suite turns the bar RED and carries the failure details
    Given the kata suite fails with "expected 3 but was 1"
    When the agent calls run_tests
    Then the call succeeds
    And the reported phase is "RED"
    And the result contains "expected 3 but was 1"

  @SRV-003
  Scenario: A build failure before any test ran is reported on the bar
    Given the kata build fails before tests can run
    When the agent calls run_tests
    Then the call succeeds
    And the reported phase is "RED"
    And the result contains "Build failed before tests could run"

  @SRV-004
  Scenario: The TDD state reports the phase, the last run, and a next step
    Given the kata suite passes with 5 tests
    And the agent calls run_tests
    When the agent calls get_tdd_state
    Then the call succeeds
    And the reported phase is "GREEN"
    And the result contains a suggested next step

  @SRV-005
  Scenario: Refactoring is allowed on a green bar and the note is logged
    Given the kata suite passes with 5 tests
    And the agent calls run_tests
    When the agent calls start_refactor with note "extract the parser"
    Then the call succeeds
    And the reported phase is "REFACTOR"
    And the refactor log records "extract the parser"

  @SRV-005
  Scenario: Refactoring on a red bar is refused by the tool
    Given the kata suite fails with "boom"
    And the agent calls run_tests
    When the agent calls start_refactor with note "cleanup"
    Then the call fails as a tool error
    And the error names the "red bar" rule

  @SRV-006
  Scenario: The assembled server identifies itself and exposes the seven tools
    When the server is assembled
    Then it identifies as "tdd-workflow-server" version "1.0.0"
    And it exposes exactly the tools "list_requirements, get_requirement, validate_spec, refine_requirement, run_tests, get_tdd_state, start_refactor"

  @SRV-007
  Scenario: A valid spec passes validation and points at the next step
    Given the spec on disk is rewritten to be valid
    When the agent calls validate_spec
    Then the call succeeds
    And the spec is reported valid
    And the result contains "write its Gherkin scenario"

  @SRV-007
  Scenario: An invalid spec returns actionable issues to iterate on
    Given the spec on disk is rewritten with a requirement missing its acceptance criteria
    When the agent calls validate_spec
    Then the call succeeds
    And the spec is reported invalid
    And the result contains "at least one acceptance criterion is required"
    And the result contains "call validate_spec again"

  @SRV-008
  Scenario: A well-worded requirement gets a clean bill and hands off to the scenario
    Given the spec on disk is rewritten with polished wording on "REQ-001"
    When the agent calls refine_requirement for "REQ-001"
    Then the call succeeds
    And the wording is reported clean
    And the result contains "Confirm it with the developer"

  @SRV-008
  Scenario: A two-valued domain answers 'valid' as concretely as a sum answers 3
    Given the spec on disk is rewritten with a validation verdict on "REQ-001"
    When the agent calls refine_requirement for "REQ-001"
    Then the call succeeds
    And the wording is reported clean
    And the result contains "Confirm it with the developer"

  @SRV-008
  Scenario: Vague wording comes back as findings for the agent to reword
    Given the spec on disk is rewritten with vague wording on "REQ-001"
    When the agent calls refine_requirement for "REQ-001"
    Then the call succeeds
    And the wording is reported unclean
    And the result contains "is ambiguous"
    And the result contains "call refine_requirement again"

  @SRV-008
  Scenario: Refining an unknown requirement is a tool error, not a crash
    When the agent calls refine_requirement for "REQ-404"
    Then the call fails as a tool error
    And the error points the agent at "list_requirements"

  @SRV-009
  Scenario: A spec split into included files serves one merged backlog
    Given the spec on disk is split into a root that includes "core.json" holding "REQ-003"
    When the agent calls list_requirements
    Then the call succeeds
    And the result mentions "REQ-001" with status "pending"
    And the result mentions "REQ-003" with status "pending"
    And the result names the project

  @SRV-009
  Scenario: An include cycle is a validation issue, not an endless loop
    Given the spec on disk is split into a root that includes "core.json" holding "REQ-003"
    And the spec file "core.json" is rewritten to include "requirements.json"
    When the agent calls validate_spec
    Then the call succeeds
    And the spec is reported invalid
    And the result contains "is included more than once"

  @SRV-009
  Scenario: A duplicate id across included files names the first declaring file
    Given the spec on disk is split into a root that includes "core.json" holding "REQ-001"
    When the agent calls validate_spec
    Then the call succeeds
    And the spec is reported invalid
    And the result contains "REQ-001: duplicate id - also declared in requirements.json"
