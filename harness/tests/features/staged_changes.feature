# Executable spec for the staging area — the behavior of
# `bdd changes show|commit|discard` and `bdd validate`.
Feature: Staged changes
  As a developer keeping control of every edit
  I want authored changes held in a staging area
  So that nothing touches the working tree until I have reviewed,
  validated, and committed it

  Scenario: An empty staging area points at the authoring commands
    When the staged changes are shown
    Then 0 staged changes are reported
    And the changes next step starts with "Nothing is staged."

  Scenario: Staged edits never touch the working tree
    Given the feature file "features/calc.feature" is created named "Calc" via staging
    Then the working tree file "features/calc.feature" does not exist
    When the staged changes are shown
    Then 1 staged change is reported
    And a staged "create" of "features/calc.feature" is listed
    And the changes next step starts with "Review the staged files"

  Scenario: Commit applies everything and clears the area
    Given the feature file "features/calc.feature" is created named "Calc" via staging
    When the staged changes are committed
    Then the working tree file "features/calc.feature" contains "Feature: Calc"
    And the changes next step starts with "Staged changes applied"
    When the staged changes are shown
    Then 0 staged changes are reported

  Scenario: Discard drops everything without applying it
    Given the feature file "features/calc.feature" is created named "Calc" via staging
    When the staged changes are discarded
    Then the working tree file "features/calc.feature" does not exist
    And the changes next step starts with "Staged changes dropped."
    When the staged changes are shown
    Then 0 staged changes are reported

  Scenario: Validate parses staged Gherkin
    Given raw content is staged at "features/broken.feature":
      """
      this is not gherkin
      """
    When the staged changes are validated
    Then the staged validation is invalid
    And a staged validation issue contains "features/broken.feature: not valid Gherkin -"

  Scenario: Validate checks the staged spec against staged features
    Given a working spec whose requirement "REQ-001" is "implemented" with feature file "features/calc.feature"
    And raw content is staged at "features/calc.feature":
      """
      Feature: Calc

        @REQ-001
        Scenario: Empty string
          Given a calculator
      """
    When the staged changes are validated
    Then the staged validation is valid
    And the staged validation next step starts with "Spec and staged Gherkin are valid."

  Scenario: Validate reports a missing tagged scenario before commit
    Given a working spec whose requirement "REQ-001" is "implemented" with feature file "features/calc.feature"
    And raw content is staged at "features/calc.feature":
      """
      Feature: Calc

        Scenario: Untagged
          Given a calculator
      """
    When the staged changes are validated
    Then the staged validation is invalid
    And a staged validation issue contains "no scenario tagged @REQ-001"
