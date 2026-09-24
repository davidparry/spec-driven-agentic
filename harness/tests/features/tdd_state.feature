# Executable spec for the Red/Green/Refactor state machine — the behavior
# behind run_tests, get_tdd_state, and start_refactor.
Feature: TDD state machine
  As a developer held to the discipline by tooling
  I want the Red/Green/Refactor cycle tracked and enforced
  So that refactoring can never begin on a red bar

  Scenario: A fresh session starts at START
    Given a fresh TDD session
    Then the phase is "START"
    And the suggestion is "No tests have been run yet. Call run_tests to establish a baseline."

  Scenario: A failing test run moves to RED
    Given a fresh TDD session
    When a failing test run is recorded
    Then the phase is "RED"
    And the suggestion is "Tests are failing. Write the simplest production code that makes them pass, then call run_tests again."

  Scenario: A passing test run moves to GREEN
    Given a fresh TDD session
    When a passing test run is recorded
    Then the phase is "GREEN"
    And the suggestion is "All tests pass. Either call start_refactor to clean up, or call get_requirement for the next pending requirement and write a failing test for it."

  Scenario: Refactoring is allowed from GREEN and logs the note
    Given a fresh TDD session
    When a passing test run is recorded
    And a refactor is started with note "extract parsing of the delimited input"
    Then the phase is "REFACTOR"
    And the refactor log contains "extract parsing of the delimited input"
    And the suggestion is "A refactor is in progress. Call run_tests to prove the refactor kept the bar green."

  Scenario: Refactoring is refused on a red bar
    Given a fresh TDD session
    When a failing test run is recorded
    And a refactor is attempted
    Then the refactor is refused with a message containing "Never refactor on a red bar"
    And the phase is "RED"

  Scenario: Refactoring is refused before any test run
    Given a fresh TDD session
    When a refactor is attempted
    Then the refactor is refused with a message containing "current phase: START"

  Scenario: A passing run after the refactor returns to GREEN
    Given a fresh TDD session
    When a passing test run is recorded
    And a refactor is started with note "cleanup"
    And a passing test run is recorded
    Then the phase is "GREEN"
