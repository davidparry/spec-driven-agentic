# Executable spec for GUID-identified sessions: every run belongs to a
# session recorded in .spec/state.json and .spec/memory.json, bare `spec`
# creates one (or resumes the one named with --session), startup lists
# every session on record without asking anything, and a file written
# before sessions existed is adopted rather than discarded.
Feature: Sessions
  As a developer returning to a project
  I want each run of the harness to belong to a GUID-identified session that I can resume by id
  So that the TDD state and project memory I pick up are the ones I left

  @HARNESS-019
  Scenario: The first start creates a new session with a GUID
    Given a project with no sessions on record
    When the shell session starts
    Then the session shown is "new"
    And the session id is a GUID of 36 characters
    And the session line names the session id

  @HARNESS-019
  Scenario: A recorded session is resumed by its id with its phase
    Given a session recorded on GREEN
    When the shell session starts with that session id
    Then the session shown is "resumed"
    And the session shown has phase "GREEN"

  @HARNESS-019
  Scenario: Startup lists every session on record and asks nothing
    Given 2 sessions that recorded work
    When the shell session starts
    Then the session list names 3 sessions
    And the current session is marked "*" in the list
    And the newest session is listed first
    And 0 questions are asked at session start
    And the session start names the resume command "spec --session"

  @HARNESS-019
  Scenario: A start that recorded nothing is continued rather than multiplied
    Given a project with no sessions on record
    When the shell session starts 3 times
    Then the session list names 1 session
    And the session shown is "resumed"

  @HARNESS-019
  Scenario: A start after recorded work begins a session and keeps the old one
    Given a session recorded on GREEN
    When the shell session starts
    Then the session list names 2 sessions
    And the session shown is "new"

  @HARNESS-019
  Scenario: A state file written before sessions existed is adopted
    Given a state file written before sessions existed holding 2 entries
    When the shell session starts
    Then the adopted session keeps 2 entries
    And the session shown is "resumed"
    And the session list names 1 session

  @HARNESS-019
  Scenario: An unknown session id is refused naming both files
    Given a session id that is not on record
    When the shell session starts with that session id
    Then the start is refused naming ".spec/state.json"
    And the start is refused naming ".spec/memory.json"

  @HARNESS-019
  Scenario: A malformed session id is refused
    Given a malformed session id "not-a-guid"
    When the shell session starts with that session id
    Then the start is refused naming "not a GUID"

  @HARNESS-019
  Scenario: One-shot commands keep following the current session
    Given a current session on RED
    When the TDD state is read in a fresh invocation without a session id
    Then the persisted phase is "RED"

  @HARNESS-019
  Scenario: A new session's memory carries the module root forward
    Given a current session remembering module root "kata"
    When a new shell session starts
    Then the new session's memory names module root "kata"
    And the new session's state log holds 0 entries
