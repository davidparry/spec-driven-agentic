Feature: Explain spec command by name

  @HARNESS-023
  Scenario: Explain spec command by name case 1
    Given a valid command name "build"
    When the explain command is called with the argument "build"
    Then the output is a description of the "build" command.

  @HARNESS-023
  Scenario: Explain spec command by name case 2
    Given an invalid command name "xyz"
    When the explain command is called with the argument "xyz"
    Then an error message "Command not found" is returned.

  @HARNESS-023
  Scenario: Explain spec command by name case 3
    Given an empty string ""
    When the explain command is called with the argument ""
    Then an error message "Command name cannot be empty" is returned.

  @HARNESS-023
  Scenario: Explain spec command by name case 4
    Given the command name "test"
    When the explain command is called with the argument "test"
    Then the output contains the word "run tests".

  @HARNESS-023
  Scenario: Explain spec command by name case 5
    Given a command name with leading whitespace " build"
    When the explain command is called with the argument " build"
    Then the output is a description of the "build" command.
