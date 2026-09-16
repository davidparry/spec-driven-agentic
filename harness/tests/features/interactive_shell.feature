# Executable spec for the interactive shell behind bare `bdd`: every
# line runs as a command without the bdd prefix, exit/quit/Ctrl+C/Ctrl+D
# end the session, and the history is saved on the way out so the next
# shell resumes it.
Feature: Interactive shell
  As a developer living in the bdd loop
  I want one shell session for many commands
  So that I never retype bdd and can come back to the session later

  Scenario: Commands run without the bdd prefix until exit
    Given the shell will read:
      """
      spec list
      state
      exit
      """
    When the interactive shell runs
    Then the shell dispatched "spec|list"
    And the shell dispatched "state"
    And the shell ended by "exit" after 2 commands
    And the session history was saved

  Scenario: A pasted one-shot command with the bdd prefix still works
    Given the shell will read:
      """
      bdd spec list
      quit
      """
    When the interactive shell runs
    Then the shell dispatched "spec|list"
    And the shell ended by "exit" after 1 command

  Scenario: Quoted arguments stay together
    Given the shell will read:
      """
      feature create --path features/calc.feature --name "String Calculator"
      exit
      """
    When the interactive shell runs
    Then the shell dispatched "feature|create|--path|features/calc.feature|--name|String Calculator"

  Scenario: Blank lines and a lone bdd are skipped
    Given the shell will read:
      """
      <empty>
      bdd
      exit
      """
    When the interactive shell runs
    Then the shell ended by "exit" after 0 commands

  Scenario: Ctrl+C ends the session and the history is still saved
    Given the shell will read:
      """
      spec list
      <ctrl-c>
      """
    When the interactive shell runs
    Then the shell ended by "Ctrl+C" after 1 command
    And the session history was saved

  Scenario: The end of input closes the session
    Given the shell will read:
      """
      <ctrl-d>
      """
    When the interactive shell runs
    Then the shell ended by "end of input" after 0 commands

  Scenario: An unbalanced quote is reported and the shell stays open
    Given the shell will read:
      """
      feature create --name "half quoted
      state
      exit
      """
    When the interactive shell runs
    Then the shell reported "unreadable input"
    And the shell dispatched "state"
    And the shell ended by "exit" after 1 command

  Scenario: A fresh greenfield project offers to start the loop
    Given the shell will read:
      """
      y
      """
    When the greenfield offer runs
    Then the shell reported "It appears you are in a greenfield"
    And the shell dispatched "greenfield"

  Scenario: Declining the greenfield offer leaves the shell ready
    Given the shell will read:
      """
      n
      """
    When the greenfield offer runs
    Then nothing was dispatched
    And the shell reported "type greenfield any time, or spec draft to begin with the spec"
