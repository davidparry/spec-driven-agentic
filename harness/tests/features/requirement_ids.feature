# Executable spec for requirement id allocation. Nothing configures the
# prefix: it is read back off the catalog being drafted into, so a spec
# numbering HARNESS-014 keeps numbering HARNESS, and the same binary
# drafting into the kata keeps numbering REQ. The prefix a catalog uses
# is a property of the catalog, never of the tool.
Feature: Requirement ids follow the catalog
  As a developer drafting into a spec that already has a convention
  I want new requirement ids to carry the prefix the catalog uses
  So that one tool serves every spec without being told how each numbers

  @HARNESS-015
  Scenario: A drafted id carries the prefix the catalog already uses
    Given a working spec with the pending requirement "HARNESS-014"
    When a requirement titled "Criteria coverage" is drafted with:
      """
      As a developer closing out a requirement, I want each criterion reported as covered so that I can see what is unproven.
      Given a requirement whose criteria are all matched by an asserting test, when coverage is requested for its id, then the verdict is "covered"
      Given a requirement carrying 0 acceptance criteria, when coverage is requested for its id, then the verdict is "uncovered"
      """
    Then the draft is staged as "HARNESS-015"

  @HARNESS-015
  Scenario: An empty catalog starts at the default prefix
    Given an empty working spec
    When a requirement titled "Empty string returns zero" is drafted with:
      """
      As a calculator user, I want an empty string to total zero so that no input is a safe call.
      Given the input "", when add is called, then the result is 0
      Given the input "1", when add is called, then the result is 1
      """
    Then the draft is staged as "REQ-001"

  @HARNESS-015
  Scenario: Numbering follows the width the catalog already uses
    Given a working spec with the pending requirement "REQ-0007"
    When a requirement titled "Newlines as delimiters" is drafted with:
      """
      As a user, I want newlines to work as delimiters so that multi-line input is supported.
      Given the input "1\n2,3", when add is called, then the result is 6
      Given an empty string "", when add is called, then the result is 0
      """
    Then the draft is staged as "REQ-0008"

  # A prefix the catalog does not use must not raise its number, or one
  # borrowed id would push every later id past a gap nobody created.
  @HARNESS-015
  Scenario: A foreign prefix in the catalog does not raise the number
    Given a working spec with the pending requirement "HARNESS-001"
    And the spec file "requirements/requirements.json" lists the include "cli.json"
    And the spec file "requirements/cli.json" holds the pending requirement "CLI-099"
    When a requirement titled "Criteria coverage" is drafted with:
      """
      As a developer closing out a requirement, I want each criterion reported as covered so that I can see what is unproven.
      Given a requirement whose criteria are all matched by an asserting test, when coverage is requested for its id, then the verdict is "covered"
      Given a requirement carrying 0 acceptance criteria, when coverage is requested for its id, then the verdict is "uncovered"
      """
    Then the draft is staged as "HARNESS-002"

  @HARNESS-015
  Scenario: The next id is reported so an agent reads the shape rather than guessing it
    Given a Java project marker
    And a working spec with the pending requirement "HARNESS-014"
    When the project status is checked
    Then the status next id is "HARNESS-015"

  # `spec deliver` used to recognise only REQ-, which made the harness's
  # own backlog undeliverable by the binary it builds.
  @HARNESS-015
  Scenario: A requirement carrying the catalog's prefix is a delivery target
    Given a working spec with the pending requirement "HARNESS-014"
    When the delivery target "harness-014" is read
    Then the delivery target is the requirement "HARNESS-014"

  @HARNESS-015
  Scenario: Prose is still a description rather than an id
    Given a working spec with the pending requirement "HARNESS-014"
    When the delivery target "a custom delimiter on the first line" is read
    Then the delivery target is a description
