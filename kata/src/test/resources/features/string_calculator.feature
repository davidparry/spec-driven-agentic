# The executable behavior spec (BDD level) for the String Calculator kata.
#
# Each scenario is tagged with the requirement it verifies. Scenarios for
# REQ-001 and REQ-002 ship with the workshop as the worked example; during
# the workshop the agent appends scenarios for REQ-003+ generated from the
# acceptance criteria in requirements/requirements.json — the human reviews
# the spec, then the loop runs RED -> GREEN -> REFACTOR.
Feature: String Calculator addition
  As a user of the calculator
  I want delimited number strings to be summed safely
  So that any input produces a predictable result

  @REQ-001
  Scenario: An empty string returns zero
    Given a string calculator
    When I add ""
    Then the result is 0

  @REQ-002
  Scenario: A single number returns its value
    Given a string calculator
    When I add "7"
    Then the result is 7

  @REQ-002
  Scenario: Another single number returns its value
    Given a string calculator
    When I add "42"
    Then the result is 42

  # REQ-003+: scenarios are written live during the workshop from the
  # acceptance criteria. Ask the agent to call get_requirement("REQ-003").
