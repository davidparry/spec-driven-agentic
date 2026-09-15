Feature: Per-command tool profiles
  As a developer running an LLM-backed bdd command
  I want each caller offered only the tools that step of the loop needs
  So that the model stays on-task and cannot stage or commit by default

  Scenario Outline: Each caller is offered exactly its default tools
    When the tools for "<caller>" are listed offline
    Then the offered tools are "<tools>"

    Examples:
      | caller             | tools                                                                                          |
      | spec-draft         | list_requirements, get_requirement, validate_spec, refine_requirement                          |
      | spec-reword        | get_requirement, validate_spec, refine_requirement                                             |
      | steps-generate     | project_inspect, feature_list, feature_read, step_definitions_find                             |
      | unittest-generate  | project_inspect, get_requirement, feature_read, step_definitions_find                          |
      | implement-advice   | get_tdd_state, validate_spec, feature_list, changes_show, changes_validate                      |
      | implement          | get_requirement, feature_read, step_definitions_find, get_tdd_state, run_tests, command_run, changes_show |
      | status             | project_root, list_requirements, get_requirement, get_tdd_state, validate_spec, changes_show, changes_validate |
      | ask                | project_root, list_requirements, get_requirement, validate_spec, refine_requirement, get_tdd_state, project_inspect, feature_list, feature_read, step_definitions_find, changes_show, changes_validate |

  Scenario: No default profile offers a staging or commit tool
    When every default profile is inspected
    Then no default profile offers a staging or commit tool

  Scenario: command_run appears only for implement
    When every default profile is inspected
    Then command_run appears only for implement

  Scenario: A profiles table replaces a caller's set
    Given the config file contains:
      """
      [tools.profiles]
      status = ["get_tdd_state"]
      """
    When the tools for "status" are listed offline
    Then the offered tools are "get_tdd_state"

  Scenario: A profiles list of builtin-qualified names replaces the default
    Given the config file contains:
      """
      [tools.profiles]
      status = ["builtin:get_tdd_state"]
      """
    When the tools for "status" are listed offline
    Then the offered tools are "get_tdd_state"

  Scenario: An enabled table adds to a caller's set
    Given the config file contains:
      """
      [tools.enabled]
      status = ["feature_list"]
      """
    When the tools for "status" are listed offline
    Then the offered tools are "project_root, list_requirements, get_requirement, get_tdd_state, validate_spec, changes_show, changes_validate, feature_list"

  Scenario: A disabled table removes from a caller's set
    Given the config file contains:
      """
      [tools.disabled]
      status = ["changes_show"]
      """
    When the tools for "status" are listed offline
    Then the offered tools are "project_root, list_requirements, get_requirement, get_tdd_state, validate_spec, changes_validate"

  Scenario: Removal beats attachment
    Given the config file contains:
      """
      [tools.enabled]
      implement = ["command_run"]
      [tools.disabled]
      implement = ["command_run"]
      """
    When the tools for "implement" are listed offline
    Then the offered tools do not include "command_run"

  Scenario: A config-named tool that does not exist warns and is skipped
    Given the config file contains:
      """
      [tools.enabled]
      status = ["nope_tool"]
      """
    When the tools for "status" are listed offline
    Then a tool warning contains "nope_tool"
    And the offered tools do not include "nope_tool"

  Scenario: Enabling a tool for implement persists under enabled and preserves llm
    Given the config file contains:
      """
      [llm]
      model = "keep-me"
      """
    When "feature_list" is enabled for "implement"
    Then the config file contains "feature_list"
    And the config file contains "keep-me"
    And the config file contains "[tools.enabled]"

  Scenario: Enabling without --for lists the callers
    When tools enable is invoked without --for
    Then the tool error contains "requires --for"
    And the tool error contains "spec-draft"
    And the tool error contains "implement"

  Scenario: Enabling for an unknown caller is refused
    When "feature_list" is enabled for the unknown caller "nonsense"
    Then the tool error contains "unknown caller"
    And the tool error contains "status"

  Scenario: tools profiles prints every caller and its resolved set
    When the tool profiles are listed
    Then the profile for "spec-draft" offers "list_requirements, get_requirement, validate_spec, refine_requirement"
    And the profile for "implement" offers "get_requirement, feature_read, step_definitions_find, get_tdd_state, run_tests, command_run, changes_show"

  Scenario: --tools replaces a profile for one run
    When the tools for "status" are listed with --tools "get_tdd_state,changes_show"
    Then the offered tools are "get_tdd_state, changes_show"

  Scenario: tools list --for implement shows only that caller's set
    When the tools for "implement" are listed offline
    Then the offered tools are "get_requirement, feature_read, step_definitions_find, get_tdd_state, run_tests, command_run, changes_show"

  Scenario: tools list --offline never connects
    When the tools are listed offline
    Then discovery did not connect

  Scenario: tools show on an unknown name is refused
    When the tool "nope" is shown
    Then the tool error contains "unknown tool"
