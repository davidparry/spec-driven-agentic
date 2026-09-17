Feature: The smoke test plans and sweeps all 25 MCP tools
  The Java smoke test is an independent conformance harness against spec mcp serve.
  ToolPlan is the data; ToolSweep is the driver; these scenarios are CLI-007
  through CLI-011 in executable form.

  @CLI-007
  Scenario: ServerLaunch.spec starts spec mcp serve against the workshop root
    When a spec launch is built for root "/tmp/workshop" and binary "/usr/bin/spec"
    Then the launch program is "/usr/bin/spec"
    And the launch args are "mcp", "serve", "--root", "/tmp/workshop"

  @CLI-008
  Scenario: Discovery keeps the input schema so required arguments can be named
    Given a discovered tool "get_requirement" whose schema requires "id"
    Then the required arguments are "id"

  @CLI-009
  Scenario: The plan names exactly 25 tools
    Then the tool plan names exactly 25 tools
    And the tool plan includes "project_root"
    And the tool plan includes "unit_test_create"
    And the tool plan includes "command_run"
    And the tool plan includes "requirement_reword"

  @CLI-009
  Scenario: An unplanned discovered tool is unexpected
    Given a server that exposes the planned tools plus "bonus_tool"
    When a read-only sweep runs
    Then the sweep reports "bonus_tool" as unexpected

  @CLI-010
  Scenario: A read-only sweep skips staging and gated tools
    Given a server that exposes every planned tool
    When a read-only sweep runs
    Then the sweep called "list_requirements"
    And the sweep did not call "scenario_add"
    And the sweep did not call "command_run"

  @CLI-011
  Scenario: A gated refusal is recorded as a call, not a failure
    Given a server that exposes every planned tool
    And the gated tool "start_refactor" answers with an error "not GREEN"
    When a mutating sweep runs
    Then the sweep called "start_refactor"
    And the sweep has no failures
