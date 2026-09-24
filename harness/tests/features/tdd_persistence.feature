# Executable spec for the persistent TDD state machine — `spec test`,
# `spec state`, and `spec refactor` sharing one machine across invocations
# through .spec/state.json, with replies matching the Java server.
Feature: Persistent TDD state
  As a developer running the loop from a short-lived harness
  I want the RED/GREEN/REFACTOR phase persisted between invocations
  So that the harness enforces the discipline the long-running server does

  Scenario: Before any run the phase is START with the baseline hint
    When the TDD state is read in a fresh invocation
    Then the persisted phase is "START"
    And the state next step is "No tests have been run yet. Call run_tests to establish a baseline."

  Scenario: A failing run moves to RED and survives the process
    Given the test suite will report 8 tests with 2 failures
    When the tests are run
    Then the test reply phase is "RED"
    And the test reply next step starts with "Tests are failing."
    When the TDD state is read in a fresh invocation
    Then the persisted phase is "RED"
    And the persisted last run counts 8 tests and 2 failures

  Scenario: A passing run moves to GREEN
    Given the test suite will report 8 tests with 0 failures
    When the tests are run
    Then the test reply phase is "GREEN"
    And the test reply next step starts with "All tests pass."

  Scenario: Refactoring is allowed on GREEN and the note is logged
    Given the test suite will report 8 tests with 0 failures
    And the tests are run
    When a persisted refactor is started with note "extract parser"
    Then the refactor reply phase is "REFACTOR"
    When the TDD state is read in a fresh invocation
    Then the persisted phase is "REFACTOR"
    And the persisted refactor log contains "extract parser"

  Scenario: Refactoring is refused on a red bar
    Given the test suite will report 8 tests with 2 failures
    And the tests are run
    When starting a refactor with note "tidy up" fails
    Then the TDD error is "Refactoring is only allowed from GREEN (current phase: RED). Never refactor on a red bar — make the tests pass first."

  Scenario: Feature and scenario filters are handed to the test runner
    Given the test suite will report 1 tests with 0 failures
    When the tests are run filtered to feature "features/calc.feature" and scenario "Adds two numbers"
    Then the runner received feature "features/calc.feature" and scenario "Adds two numbers"
    And the test reply phase is "GREEN"

  Scenario: A missing runtime never reaches the state machine
    Given the test runner reports runtime "mvn" missing with hint "Install a JDK and Apache Maven, then rerun."
    When running the tests is refused
    Then the refusal names runtime "mvn"
    When the TDD state is read in a fresh invocation
    Then the persisted phase is "START"

  Scenario: Each recorded run is a timestamped state entry
    Given the test suite will report 8 tests with 2 failures
    When the tests are run
    Then the persisted state log holds 1 entry
    And every persisted state entry has a timestamp
    And the persisted state file carries interpretation instructions

  Scenario: The agent only sees the three latest state entries
    Given the test suite will report 8 tests with 0 failures
    And the tests are run
    And the tests are run
    And the tests are run
    And the tests are run
    When the TDD state is read in a fresh invocation
    Then the state reply holds 3 entries
    And the persisted state log holds 4 entries
    And the state reply carries interpretation instructions
    And every persisted state entry has a timestamp
