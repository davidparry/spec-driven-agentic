# Executable spec for finding the project to work on, and the file
# inside it to draft into. Both answers come from where the command was
# run: the nearest enclosing project wins, and inside that project the
# catalog file covering the working directory takes the draft. Anything
# typed on the command line outranks both.
Feature: The project and the spec file are found from where you are
  As a developer running spec from inside a repository
  I want the nearest enclosing project and the catalog file covering me
  So that the command works on what I am looking at without flags

  @HARNESS-016
  Scenario: A command run in a subdirectory finds the project above it
    Given a project holding a spec catalog
    And the working directory is "src/domain" inside it
    When the project root is discovered
    Then the project root is the project directory

  @HARNESS-016
  Scenario: The nearest project wins over the one enclosing it
    Given a project holding a spec catalog
    And a nested project "harness" holding its own spec catalog
    And the working directory is "harness/src" inside it
    When the project root is discovered
    Then the project root is the nested project directory

  # A project configured but not yet drafted into is still a project, so
  # spec init followed by cd src keeps working.
  @HARNESS-016
  Scenario: A spec home marks a project before any catalog exists
    Given a project holding only a spec home
    And the working directory is "src" inside it
    When the project root is discovered
    Then the project root is the project directory

  @HARNESS-016
  Scenario: At the project root there is nothing to discover
    Given a project holding a spec catalog
    And the working directory is "" inside it
    When the project root is discovered
    Then no project root is discovered

  @HARNESS-016
  Scenario: Outside any project there is nothing to discover
    Given a directory belonging to no project
    When the project root is discovered
    Then no project root is discovered

  @HARNESS-017
  Scenario: An undirected draft lands in the catalog file covering the working directory
    Given a working spec with the pending requirement "REQ-001"
    And the spec file "requirements/requirements.json" lists the include "core/math.json"
    And the spec file "requirements/core/math.json" holds the pending requirement "MATH-001"
    And the command is run in "core"
    When a requirement titled "Newlines as delimiters" is drafted with:
      """
      As a user, I want newlines to work as delimiters so that multi-line input is supported.
      Given the input "1\n2,3", when add is called, then the result is 6
      Given an empty string "", when add is called, then the result is 0
      """
    Then the draft is staged as "MATH-002"
    And the staged spec file "requirements/core/math.json" has 2 requirements

  @HARNESS-017
  Scenario: A working directory no catalog file covers drafts into the root
    Given a working spec with the pending requirement "REQ-001"
    And the spec file "requirements/requirements.json" lists the include "core/math.json"
    And the spec file "requirements/core/math.json" holds the pending requirement "MATH-001"
    And the command is run in "src/domain"
    When a requirement titled "Newlines as delimiters" is drafted with:
      """
      As a user, I want newlines to work as delimiters so that multi-line input is supported.
      Given the input "1\n2,3", when add is called, then the result is 6
      Given an empty string "", when add is called, then the result is 0
      """
    Then the draft is staged as "REQ-002"
    And the staged spec file "requirements/requirements.json" has 2 requirements

  @HARNESS-017
  Scenario: A named file outranks the working directory
    Given a working spec with the pending requirement "REQ-001"
    And the spec file "requirements/requirements.json" lists the include "core/math.json"
    And the spec file "requirements/core/math.json" holds the pending requirement "MATH-001"
    And the command is run in "core"
    When a requirement titled "Newlines as delimiters" is drafted into "requirements/requirements.json" with:
      """
      As a user, I want newlines to work as delimiters so that multi-line input is supported.
      Given the input "1\n2,3", when add is called, then the result is 6
      Given an empty string "", when add is called, then the result is 0
      """
    Then the draft is staged as "REQ-002"
    And the staged spec file "requirements/requirements.json" has 2 requirements
