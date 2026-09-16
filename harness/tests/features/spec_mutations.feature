# Executable spec for controlled spec authoring — the behavior of
# `bdd spec draft`, `bdd spec mark-implemented`, `bdd feature create`,
# and `bdd scenario add|update|delete`. All mutations land in staging.
Feature: Spec mutations
  As a developer who owns the spec wording
  I want drafting, status flips, and scenario edits staged and gated
  So that the spec stays the reviewed source of truth

  Background:
    Given a working spec with the pending requirement "REQ-001"

  Scenario: A clean draft is staged under the next free id
    Given the developer will answer:
      """
      Comma sums
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      """
    When a requirement is drafted
    Then the draft is staged as "REQ-002"
    And the staged spec has 2 requirements

  Scenario: Findings drive rewording until the draft is clean
    Given the developer will answer:
      """
      Comma sums
      the calculator should handle commas quickly
      the result is 3
      <empty>
      Comma sums
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      """
    When a requirement is drafted
    Then the draft is staged as "REQ-002"
    And the developer was told a finding containing "must be phrased Given/When/Then"
    And the developer was told a finding containing "'should' is ambiguous"
    And the developer was told a finding containing "try: rephrase as: Given <starting state>, when <action>, then <exact result>"
    And the developer was told a finding containing "try: replace the vague word with the exact observable behavior"

  Scenario: Rewording prompts show the id with prior answers that Enter keeps
    Given the developer will answer:
      """
      Comma sums
      the calculator should handle commas quickly
      Given the input "1,2", when add is called, then the result is 3
      Given an empty string "", when add is called, then the result is 0
      <empty>
      <empty>
      As a user, I want comma sums so that totals come from one input.
      <empty>
      <empty>
      <empty>
      y
      """
    When a requirement is drafted
    Then the draft is staged as "REQ-002"
    And the developer was asked "REQ-002 title [Comma sums] (Enter keeps it):"
    And the developer was asked "REQ-002 criterion 1 [Given the input "1,2", when add is called, then the result is 3] (Enter keeps it, '-' drops it):"
    And the developer was asked "REQ-002 criterion 3 (leave blank to finish the criteria):"
    And the staged spec has 2 requirements

  Scenario: A described feature is split into proposals the wizard walks through
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"title": "Empty string returns zero",
        "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
        "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string, empty input means zero
      <empty>
      2
      <empty>
      <empty>
      <empty>
      <empty>
      y
      """
    When a requirement is drafted with the model's help
    Then the draft is staged as "REQ-003"
    And the developer was told a finding containing "Accept all these requirements to refine, or enter comma-separated numbers of the ones to accept."
    And the developer was told a finding containing "The description holds 2 requirement(s):"
    And the developer was told a finding containing "2. Empty string returns zero"
    And the developer was asked "Accept [Enter for all, or comma-separated numbers]:"
    And the developer was told a finding containing "Accepted requirements are now stored in requirements/requirements.json as pending:"
    And the developer was told a finding containing "REQ-002 Comma separated numbers are summed"
    And the developer was told a finding containing "REQ-003 Empty string returns zero"
    And the developer was asked "Which requirement first to review and refine? [1-2, Enter for 1]:"
    And the developer was asked "REQ-003 title [Empty string returns zero] (Enter keeps it):"
    And the developer was asked "REQ-003 criterion 1 [Given an empty string "", when add is called, then the result is 0] (Enter keeps it, '-' drops it):"
    And the working spec has 3 requirements
    And the staged spec has 3 requirements

  Scenario: An unusable model reply falls back to manual drafting
    Given the model will reply:
      """
      Sure! Here are the requirements you asked for.
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string
      Comma sums
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      """
    When a requirement is drafted with the model's help
    Then the draft is staged as "REQ-002"
    And the developer was told a finding containing "asking again (2 of 3)"
    And the developer was told a finding containing "The description gave no complete requirement - drafting manually."
    And the developer was asked "REQ-002 title:"

  Scenario: Findings send the draft to the model whose rewording seeds the prompts
    Given the model will reply:
      """
      {"title": "Comma separated numbers are summed",
       "story": "As a user, I want comma sums so that totals come from one input.",
       "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3",
                              "Given an empty string \"\", when add is called, then the result is 0"]}
      """
    And the developer will answer:
      """
      <empty>
      Comma sums
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      y
      """
    When a requirement is drafted with the model's help
    Then the draft is staged as "REQ-002"
    And the developer was told a finding containing "only happy paths"
    And the developer was told a finding containing "Asking scripted-model to address finding 1 of 1 - working ..."
    And the developer was told a finding containing "The model reworded the draft"
    And the developer was asked "REQ-002 title [Comma separated numbers are summed] (Enter keeps it):"
    And the developer was asked "REQ-002 criterion 2 [Given an empty string "", when add is called, then the result is 0] (Enter keeps it, '-' drops it):"
    And the staged spec has 2 requirements

  Scenario: Each finding is its own model call, chained on the previous fix
    Given the model will reply:
      """
      {"title": "Comma separated numbers are summed",
       "story": "As a user, I want comma sums so that totals come from one input.",
       "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3",
                              "Given an empty string \"\", when add is called, then the result is 0"]}
      """
    And the developer will answer:
      """
      <empty>
      Comma sums
      The user gets comma sums so that totals come from one input
      Given the input "1,2", when add is called, then the result is 3
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      y
      """
    When a requirement is drafted with the model's help
    Then the draft is staged as "REQ-002"
    And the developer was told a finding containing "Asking scripted-model to address finding 1 of 2 - working ..."
    And the developer was told a finding containing "Asking scripted-model to address finding 2 of 2 - working ..."

  Scenario: A second rewording pass carries the wording the review already rejected
    Given the model will reply:
      """
      {"title": "Comma separated numbers are summed",
       "story": "As a user, I want comma sums so that totals come from one input.",
       "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}
      """
    And the developer will answer:
      """
      <empty>
      Comma sums
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      """
    When a requirement is drafted with the model's help
    Then the draft is staged as "REQ-002"
    And the developer was told a finding containing "Asking scripted-model to address finding 1 of 1 - working ..." 2 times

  Scenario: An unusable rewording still lets the developer fix the draft by hand
    Given the model will reply:
      """
      Sure! A better wording would be hard to say.
      """
    And the developer will answer:
      """
      <empty>
      Comma sums
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      <empty>
      <empty>
      <empty>
      <empty>
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      """
    When a requirement is drafted with the model's help
    Then the draft is staged as "REQ-002"
    And the developer was told a finding containing "The model's rewording for finding 1 was unusable"
    And the developer was told a finding containing "Reword the requirement to address each finding."

  Scenario: A declined draft stages nothing
    Given the developer will answer:
      """
      Comma sums
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      Given an empty string "", when add is called, then the result is 0
      <empty>
      n
      """
    When a requirement is drafted
    Then the draft is not staged
    And nothing is staged at the spec path

  Scenario: Marking implemented on GREEN stages the flip and records the feature file
    Given the persisted TDD phase is "GREEN"
    And a project feature file "features/calc.feature" containing:
      """
      Feature: Calc

        @REQ-001
        Scenario: Adds
          Given a calculator
      """
    When requirement "REQ-001" is marked implemented
    Then the staged spec shows "REQ-001" as "implemented"
    And the staged spec names "features/calc.feature" as the feature file of "REQ-001"

  Scenario: Marking implemented is refused off GREEN
    Given the persisted TDD phase is "RED"
    When marking requirement "REQ-001" implemented fails
    Then the mutation error is "Requirements are only marked implemented on GREEN (current phase: RED). Run the tests and make them pass first."

  Scenario: Marking implemented without a tagged scenario names the recovery commands
    Given the persisted TDD phase is "GREEN"
    When marking requirement "REQ-001" implemented fails
    Then the mutation error is "No scenario is tagged @REQ-001 - implemented requirements need an executable scenario. Add one with bdd scenario add, apply it with bdd changes commit, then mark REQ-001 implemented."

  Scenario: Creating a feature stages a bare feature file
    When the feature "features/calc.feature" named "Calc" is created
    Then staged content at "features/calc.feature" equals:
      """
      Feature: Calc
      """

  Scenario: Adding a scenario stages it tagged with the requirement
    Given the feature file "features/calc.feature" is created named "Calc" via staging
    When scenario "Empty string" for "REQ-001" is added to "features/calc.feature" with steps:
      """
      Given a calculator
      When add is called with ""
      Then the result is 0
      """
    Then the staged feature "features/calc.feature" has scenario "Empty string" tagged "@REQ-001"

  Scenario: A step without a Gherkin keyword is refused
    Given the feature file "features/calc.feature" is created named "Calc" via staging
    When adding scenario "Bad" for "REQ-001" to "features/calc.feature" fails with steps:
      """
      the result is 0
      """
    Then the mutation error is "step \"the result is 0\" must start with Given, When, Then, And, or But"

  Scenario: Updating a scenario replaces its steps in staging
    Given the feature file "features/calc.feature" is created named "Calc" via staging
    And scenario "Empty string" for "REQ-001" is added to "features/calc.feature" with steps:
      """
      Given a calculator
      Then the result is 0
      """
    When scenario "Empty string" in "features/calc.feature" is updated with steps:
      """
      Given a calculator
      When add is called with ""
      Then the result is 0
      """
    Then the staged feature "features/calc.feature" scenario "Empty string" has 3 steps

  Scenario: Deleting a scenario removes it from staging
    Given the feature file "features/calc.feature" is created named "Calc" via staging
    And scenario "Empty string" for "REQ-001" is added to "features/calc.feature" with steps:
      """
      Given a calculator
      """
    When scenario "Empty string" is deleted from "features/calc.feature"
    Then the staged feature "features/calc.feature" has 0 scenarios

  Scenario: Adding to a missing feature names the recovery command
    When adding scenario "S" for "REQ-001" to "features/nope.feature" fails with steps:
      """
      Given a calculator
      """
    Then the mutation error is "features/nope.feature: no such feature file. Create it first with feature create."
