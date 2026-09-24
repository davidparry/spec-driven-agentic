Feature: External MCP server registry
  As a developer attaching an extra MCP server to one spec command
  I want mcp.json discovered, invalid entries skipped, and refresh rewriting the cache
  So that nothing external reaches a model until it is attached with --for

  Scenario: mcp.json in the root is found and its servers listed
    Given a project source file "mcp.json" containing:
      """
      {"mcpServers":{"self":{"command":"spec","args":["mcp","serve"]}}}
      """
    When the MCP registry is loaded
    Then the registry path contains "mcp.json"
    And the registry lists server "self"

  Scenario: tools mcp_config overrides the location
    Given a project source file "elsewhere.json" containing:
      """
      {"mcpServers":{"other":{"command":"spec"}}}
      """
    When the MCP registry is loaded from "elsewhere.json"
    Then the registry lists server "other"

  Scenario: A missing file is not an error and built-ins still work
    When the MCP registry is loaded
    Then the registry lists no servers
    And the registry has no problems
    And the tools listed offline include "list_requirements"

  Scenario: Invalid JSON is a reported problem, built-ins still work
    Given a project source file "mcp.json" containing:
      """
      { not json
      """
    When the MCP registry is loaded
    Then a registry problem contains "invalid JSON"
    And the tools listed offline include "validate_spec"

  Scenario: An entry without command is skipped with a reason
    Given the registry JSON:
      """
      {"mcpServers":{"broken":{"args":["mcp"]}}}
      """
    When the registry JSON is parsed
    Then a registry problem contains "broken"

  Scenario: A url or sse entry is skipped with only stdio
    Given the registry JSON:
      """
      {"mcpServers":{"remote":{"url":"https://example.invalid/mcp"}}}
      """
    When the registry JSON is parsed
    Then a registry problem contains "only stdio"

  Scenario: workspaceFolder expands
    Given the registry JSON:
      """
      {"mcpServers":{"self":{"command":"spec","args":["mcp","serve","--root","${workspaceFolder}"]}}}
      """
    When the registry JSON is parsed
    Then the registry server "self" argument contains the workspace folder

  Scenario: disabled true is skipped
    Given the registry JSON:
      """
      {"mcpServers":{"off":{"command":"spec","disabled":true},"on":{"command":"spec"}}}
      """
    When the registry JSON is parsed
    Then the registry lists server "on"
    And the registry does not list server "off"

  Scenario: Duplicate names are renamed and reported
    Given the registry JSON:
      """
      {"mcpServers":{"self":{"command":"one"},"Self":{"command":"two"}}}
      """
    When the registry JSON is parsed
    Then a registry problem contains "renamed" or the servers have distinct names

  Scenario: A server that fails to start is reported per-server while the rest still list
    Given a project source file "mcp.json" containing:
      """
      {"mcpServers":{"gone":{"command":"this-binary-does-not-exist-12345"}}}
      """
    When the tools are listed with discovery
    Then a tool problem contains "gone"
    And the tools listed include "list_requirements"

  Scenario: tools refresh rewrites the cache
    Given a project source file "mcp.json" containing:
      """
      {"mcpServers":{"self":{"command":"spec","args":["mcp","serve"]}}}
      """
    When the tool catalog is refreshed
    Then discovery connected
