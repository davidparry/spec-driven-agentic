# Executable spec for reading the requirements spec — the list every
# workflow starts from and the enriched single-requirement view.
Feature: Spec reading
  As an agent driving the workflow from the requirements spec
  I want to list the requirements and read one enriched with locations
  So that every next action starts from the spec, not from guesswork

  Scenario: Listing requirements shows ids, titles, and statuses
    Given a valid pending requirement "REQ-001"
    And an implemented requirement "REQ-002" with a scenario tagged in its feature file
    When the requirements are listed
    Then 2 requirements are listed
    And the listing has "REQ-001" titled "A title" with status "pending"
    And the listing has "REQ-002" titled "A title" with status "implemented"

  Scenario: Showing a requirement enriches it with locations and a workflow hint
    Given a valid pending requirement "REQ-001"
    When the requirement "REQ-001" is shown
    Then the shown requirement has id "REQ-001" and status "pending"
    And the shown requirement points at steps "steps/Steps.java", tests "tests/Test.java", and production "src/Prod.java"
    And the shown feature location is "features/x.feature"
    And the shown workflow hint mentions "@REQ-001"

  Scenario: Showing an unknown id points to list_requirements
    Given a valid pending requirement "REQ-001"
    When showing the requirement "REQ-999" fails
    Then the spec reading error is "No requirement with id 'REQ-999'. Call list_requirements to see valid ids."
