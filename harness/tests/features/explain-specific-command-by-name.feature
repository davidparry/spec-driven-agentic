Feature: Explain specific command by name

  @HARNESS-023
  Scenario: Explain specific command by name case 1
    Given a valid command name "run" exists in the command tree
    When the explain command is called with "run"
    Then the output displays the command's description, its options, and the list of tools it calls

  @HARNESS-023
  Scenario: Explain specific command by name case 2
    Given the command name "unknown-cmd" does not exist in the command tree
    When the explain command is called with "unknown-cmd"
    Then an error message "Unknown command" is returned
