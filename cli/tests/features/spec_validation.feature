# Executable spec for structural validation of the requirements spec —
# the behavior of the validate_spec tool.
Feature: Spec validation
  As a developer driving work from a requirements spec
  I want structural problems reported with exact, actionable issues
  So that only a usable spec is turned into scenarios and code

  Scenario: A well-formed spec is valid
    Given a valid pending requirement "REQ-001"
    When the spec is validated
    Then the spec is valid
    And the next step advises writing the Gherkin scenario

  Scenario: A criterion must be phrased Given/When/Then
    Given a pending requirement "REQ-007" with criterion "the result should be 6 for 1\n2,3"
    When the spec is validated
    Then the spec is invalid
    And an issue is "REQ-007: criterion "the result should be 6 for 1\n2,3" must be phrased Given/When/Then"
    And the next step advises requirement_reword and re-validating

  Scenario: Requirement ids must follow the REQ-007 shape
    Given a valid pending requirement "req-1"
    When the spec is validated
    Then the spec is invalid
    And an issue is "req-1: id must look like REQ-007 (uppercase prefix, dash, number)"

  Scenario: Duplicate requirement ids are rejected
    Given a valid pending requirement "REQ-001"
    And another valid pending requirement with the same id "REQ-001"
    When the spec is validated
    Then the spec is invalid
    And an issue is "REQ-001: duplicate id - every requirement needs its own"

  Scenario: A pending requirement may name a missing feature file
    Given a valid pending requirement "REQ-006" whose feature file is missing from disk
    When the spec is validated
    Then the spec is valid

  Scenario: An implemented requirement's named feature file must exist
    Given an implemented requirement "REQ-006" whose feature file is missing from disk
    When the spec is validated
    Then the spec is invalid
    And an issue is "REQ-006: featureFile features/x.feature does not exist"

  Scenario: An implemented requirement needs a tagged scenario
    Given an implemented requirement "REQ-006" with no scenario tagged in its feature file
    When the spec is validated
    Then the spec is invalid
    And an issue is "REQ-006: no scenario tagged @REQ-006 in features/x.feature - implemented requirements need executable scenarios"

  Scenario: An implemented requirement with a tagged scenario is valid
    Given an implemented requirement "REQ-006" with a scenario tagged in its feature file
    When the spec is validated
    Then the spec is valid
