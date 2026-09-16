# Executable spec for wording-quality review — the behavior of the
# refine_requirement tool, one rung above structural validation.
Feature: Requirement refinement
  As a developer agreeing on a spec with an agent
  I want vague wording called out with specific findings
  So that valid-but-poor requirements are reworded before any code exists

  Scenario: Clean wording has no findings
    Given a requirement "REQ-007" with story "As a calculator user, I want newlines to separate numbers in addition to commas so that multi-line input just works."
    And the requirement has criterion "Given the input "1\n2,3", when add is called, then the result is 6"
    And the requirement has criterion "Given an empty string "", when add is called, then the result is 0"
    When the requirement "REQ-007" is refined
    Then the requirement is clean
    And the next step advises confirming the wording with the developer

  Scenario: The workshop demo story earns exactly five findings
    Given a requirement "REQ-007" with story "the calculator should handle newlines quickly"
    And the requirement has criterion "Given the input "1\n2,3", when add is called, then the result is 6"
    And the requirement has criterion "Given an empty string "", when add is called, then the result is 0"
    When the requirement "REQ-007" is refined
    Then the requirement is not clean
    And there are 5 findings
    And a finding is "story: missing the actor - start with 'As a ...' so we know who this is for"
    And a finding is "story: missing the why - finish with 'so that ...' so the value is explicit"
    And a finding is "story: 'should' is ambiguous - describe the observable behavior instead"
    And a finding is "story: 'handle' is ambiguous - describe the observable behavior instead"
    And a finding is "story: 'quickly' is ambiguous - describe the observable behavior instead"
    And the next step advises requirement_reword and iterating

  Scenario: Happy-path-only criteria earn the coverage finding
    Given a requirement "REQ-007" with story "As a user, I want newlines to separate numbers so that multi-line input works."
    And the requirement has criterion "Given the input "1\n2,3", when add is called, then the result is 6"
    When the requirement "REQ-007" is refined
    Then the requirement is not clean
    And a finding is "criteria: only happy paths - add at least one edge case (empty, invalid, or error input)"

  Scenario: A criterion covering two actions is split out
    Given a requirement "REQ-007" with story "As a user, I want sums so that errors are visible."
    And the requirement has criterion "Given a calculator, when I add and when I subtract, then 4"
    When the requirement "REQ-007" is refined
    Then the requirement is not clean
    And a finding is "criterion "Given a calculator, when I add and when I subtract, then 4": covers more than one action - split it so each criterion has a single When"

  Scenario: Deterministic sentinel and error outcomes are concrete
    Given a requirement "REQ-007" with story "As a user, I want single integers parsed so that values are usable."
    And the requirement has criterion "Given no prior input, when I enter "42", then the calculator returns 42"
    And the requirement has criterion "Given no prior input, when I enter "abc", then the calculator returns NaN"
    When the requirement "REQ-007" is refined
    Then the requirement is clean
    And the next step advises confirming the wording with the developer

  Scenario: A bare empty-string literal counts as edge-case coverage
    Given a requirement "REQ-007" with story "As a user, I want sums so that totals come from one input."
    And the requirement has criterion "Given the input "1,2", when add is called, then the result is 3"
    And the requirement has criterion "Given the input strings "" and "0", when the add function is called, then the result is "0""
    When the requirement "REQ-007" is refined
    Then the requirement is clean
    And the next step advises confirming the wording with the developer

  Scenario: A validation verdict is as concrete as a number
    Given a requirement "REQ-007" with story "As a user, I want my phone number input validated so that only numbers in the format XXX-XXX-XXXX or XXXXXXXXXX are accepted"
    And the requirement has criterion "Given the user enters '123-456-7890', when the system validates it, then the result is 'valid'"
    And the requirement has criterion "Given the user enters '123-45-6789', when the system validates it, then the result is 'invalid'"
    When the requirement "REQ-007" is refined
    Then the requirement is clean
    And the next step advises confirming the wording with the developer

  Scenario: A lone apostrophe is punctuation, not a quoted value
    Given a requirement "REQ-007" with story "As a user, I want entries read back so that storage is trustworthy."
    And the requirement has criterion "Given a stored entry, when it is read, then the user's entry is unchanged"
    When the requirement "REQ-007" is refined
    Then the requirement is not clean
    And a finding is "criterion "Given a stored entry, when it is read, then the user's entry is unchanged": the outcome is not concrete - state the exact expected value after 'then'"

  Scenario: An outcome without a concrete value is flagged
    Given a requirement "REQ-007" with story "As a user, I want sums so that errors are visible."
    And the requirement has criterion "Given a calculator, when I add, then it works"
    When the requirement "REQ-007" is refined
    Then the requirement is not clean
    And a finding is "criterion "Given a calculator, when I add, then it works": the outcome is not concrete - state the exact expected value after 'then'"
