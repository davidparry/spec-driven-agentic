# The executable behavior spec for where a run's diagnostics are kept.
Feature: Diagnostics kept per session

  As a developer reading back what a run did
  I want each session's diagnostics in a directory of its own
  So that one session's lines are never interleaved with another's

  @HARNESS-023
  Scenario: A run's diagnostics are filed under the session it belongs to
    Given a session recorded on GREEN
    When a one-shot command runs in that project
    Then the rolling log file is written under that session's directory
    And it holds the diagnostics of that run

  @HARNESS-023
  Scenario: A run naming a session is filed under the one it named
    Given a session recorded on GREEN and another started since
    When a one-shot command runs naming the recorded session
    Then the rolling log file is written under that session's directory
    And the session started since has no diagnostics of its own

  @HARNESS-023
  Scenario: A project with no session on record keeps the flat log directory
    Given a project with no sessions on record
    When a one-shot command runs in that project
    Then the rolling log file is written in the log directory itself
