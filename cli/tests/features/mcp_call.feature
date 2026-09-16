Feature: Stateless MCP tool calls from the shell
  As a developer debugging a tool profile with no model in the loop
  I want `bdd mcp call` to open one session, invoke one tool, print, and exit
  So that every served tool is reachable from a shell and failures do not kill it

  Background:
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"

  Scenario: mcp call list_requirements prints the tool's own JSON
    When mcp call "list_requirements"
    Then the tool reply contains "REQ-001"
    And the tool reply contains "requirements"

  Scenario: --arg id=REQ-001 reaches the tool
    When mcp call "get_requirement" with arg "id=REQ-001"
    Then the tool reply contains "REQ-001"

  Scenario: --args supplies an array argument --arg cannot
    When mcp arguments are merged from args '{"extra":[1,2]}' and arg "id=REQ-001"
    Then the merged arguments contain array "extra"

  Scenario: --args plus --arg merges with --arg winning
    When mcp arguments are merged from args '{"id":"REQ-001","extra":[1]}' and arg "id=REQ-002"
    Then the merged argument "id" is "REQ-002"

  Scenario: An unknown tool is refused with candidates and no session is opened
    When preparing mcp call "nope" fails
    Then the tool error contains "unknown"
    And no MCP session was opened

  Scenario: A missing required argument is named before any session opens
    When preparing mcp call "get_requirement" with no arguments fails
    Then the tool error contains "id"
    And no MCP session was opened

  Scenario: A tool error prints the text and is a nonzero outcome
    When mcp call "start_refactor"
    Then the tool reply is an error
    And the tool reply contains "GREEN"

  Scenario: --json wraps the reply as tool, isError, content
    When mcp call "list_requirements" as json
    Then the JSON envelope names tool "list_requirements"
    And the JSON envelope isError is false

  Scenario: Two successive calls each get a fresh session
    When mcp call "list_requirements"
    And mcp call "get_tdd_state"
    Then 2 MCP sessions were opened

  Scenario: State written by one call is visible to the next
    When mcp call "list_requirements"
    And mcp call "get_tdd_state"
    Then the tool reply contains "phase"

  Scenario: mcp tools lists 25 names over the wire
    When mcp tools are listed over the wire
    Then 25 MCP tools are listed
    And the listed MCP tools include "project_root"
    And the listed MCP tools include "requirement_reword"
    And the listed MCP tools include "requirement_mark_implemented"
    And the listed MCP tools include "unit_test_create"
    And the listed MCP tools include "changes_validate"
