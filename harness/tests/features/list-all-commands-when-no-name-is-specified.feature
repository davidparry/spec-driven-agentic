Feature: List all commands when no name is specified

  @HARNESS-024
  Scenario: List all commands when no name is specified case 1
    Given no command name is provided
    When the explain command is called
    Then the output lists every available command name from the command tree

  @HARNESS-024
  Scenario: List all commands when no name is specified case 2
    Given the command tree is empty
    When the explain command is called with no name
    Then the output is an empty list
