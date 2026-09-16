# Executable spec for feature-file discovery and parsed reads — the
# behavior of `bdd feature list` and `bdd feature show`.
Feature: Feature file reading
  As a developer or agent working from the spec
  I want feature files discovered and parsed into a plain structure
  So that scenarios and their requirement tags are visible without
  reading raw Gherkin

  Background:
    Given a project feature file "features/calc.feature" containing:
      """
      @kata
      Feature: String calculator

        @REQ-001
        Scenario: Empty string returns zero
          Given a calculator
          When add is called with ""
          Then the result is 0

        @REQ-002
        Scenario: Single number returns its value
          Given a calculator
          When add is called with "7"
          Then the result is 7
      """

  Scenario: Listing summarizes every feature file
    When the features are listed
    Then 1 feature is listed
    And the listing shows "features/calc.feature" named "String calculator" with 2 scenarios

  Scenario: Reading returns tags and rendered steps
    When the feature "features/calc.feature" is read
    Then the feature is tagged "@kata"
    And scenario "Empty string returns zero" is tagged "@REQ-001"
    And scenario "Empty string returns zero" has step "When add is called with """
    And the feature carries the tags "@kata, @REQ-001, @REQ-002"

  Scenario: Reading a missing file names the recovery command
    When reading the feature "features/nope.feature" fails
    Then the feature error is "features/nope.feature: no such feature file. Call feature list to see valid paths."

  Scenario: Invalid Gherkin is reported with the file name
    Given a project feature file "features/broken.feature" containing:
      """
      this is not gherkin
      """
    When listing the features fails
    Then the feature error contains "features/broken.feature: not valid Gherkin -"
