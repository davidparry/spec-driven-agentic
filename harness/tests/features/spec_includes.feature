# Executable spec for the requirements catalog — the root
# requirements/requirements.json is always the entry point, holds
# requirements of its own, and may include child spec files that nest
# N levels deep. The tooling merges the tree, drives the workflow from
# the merged view, and writes every mutation back to the file the
# requirement lives in.
Feature: Spec catalog includes
  As a developer growing a spec beyond one file
  I want the root requirements.json to include child spec files
  So that the backlog scales while one catalog stays the entry point

  Background:
    Given a working spec with the pending requirement "REQ-001"

  Scenario: Requirements from included files merge into one backlog
    Given the spec file "requirements/requirements.json" lists the include "core.json"
    And the spec file "requirements/core.json" holds the pending requirement "REQ-002"
    When the requirements are listed with their files
    Then 2 requirements are listed with files
    And requirement "REQ-001" is listed from "requirements/requirements.json"
    And requirement "REQ-002" is listed from "requirements/core.json"

  Scenario: Includes nest N levels deep
    Given the spec file "requirements/requirements.json" lists the include "core/math.json"
    And the spec file "requirements/core/math.json" holds the pending requirement "REQ-002"
    And the spec file "requirements/core/math.json" lists the include "edge.json"
    And the spec file "requirements/core/edge.json" holds the pending requirement "REQ-003"
    When the requirements are listed with their files
    Then 3 requirements are listed with files
    And requirement "REQ-003" is listed from "requirements/core/edge.json"

  Scenario: Adding an include stages the catalog entry and an empty child file
    When the spec file "requirements/core/math.json" is included in the catalog
    Then the include of "requirements/core/math.json" under "requirements/requirements.json" is staged as created
    And the staged spec file "requirements/core/math.json" has 0 requirements
    And the staged spec lists the include "core/math.json"

  Scenario: A staged include is immediately draftable into
    Given the spec file "requirements/core/math.json" is included in the catalog
    When a requirement titled "Newlines as delimiters" is drafted into "requirements/core/math.json" with:
      """
      As a user, I want newlines to work as delimiters so that multi-line input is supported.
      Given the input "1\n2,3", when add is called, then the result is 6
      Given an empty string "", when add is called, then the result is 0
      """
    Then the draft is staged as "REQ-002"
    And the staged spec file "requirements/core/math.json" has 1 requirements

  Scenario: Drafting into a file outside the catalog is refused
    When drafting a requirement titled "Newlines" into "requirements/other.json" fails with:
      """
      As a user, I want newlines to work as delimiters so that multi-line input is supported.
      Given the input "1\n2,3", when add is called, then the result is 6
      """
    Then the mutation error contains "not part of the spec catalog"

  Scenario: Marking implemented writes back to the file the requirement lives in
    Given the spec file "requirements/requirements.json" lists the include "core.json"
    And the spec file "requirements/core.json" holds the pending requirement "REQ-002"
    And the persisted TDD phase is "GREEN"
    And a project feature file "features/calc.feature" containing:
      """
      Feature: Calc

        @REQ-002
        Scenario: Adds
          Given a calculator
      """
    When requirement "REQ-002" is marked implemented
    Then the staged spec file "requirements/core.json" shows "REQ-002" as "implemented"
    And nothing is staged at the spec path

  Scenario: A duplicate id across files names the file that declared it first
    Given the spec file "requirements/requirements.json" lists the include "core.json"
    And the spec file "requirements/core.json" holds the pending requirement "REQ-001"
    When the staged changes are validated
    Then the staged validation is invalid
    And a staged validation issue contains "REQ-001: duplicate id - also declared in requirements.json"

  Scenario: A missing included file is a validation issue
    Given the spec file "requirements/requirements.json" lists the include "missing.json"
    When the staged changes are validated
    Then the staged validation is invalid
    And a staged validation issue contains "spec: missing.json is not readable"

  Scenario: An include cycle is reported instead of looping forever
    Given the spec file "requirements/requirements.json" lists the include "core.json"
    And the spec file "requirements/core.json" holds the pending requirement "REQ-002"
    And the spec file "requirements/core.json" lists the include "requirements.json"
    When the staged changes are validated
    Then the staged validation is invalid
    And a staged validation issue contains "spec: requirements.json is included more than once"

  Scenario: A pure catalog file with no requirements of its own is valid
    Given the spec file "requirements/requirements.json" lists the include "catalog.json"
    And the spec file "requirements/catalog.json" lists the include "core/math.json"
    And the spec file "requirements/core/math.json" holds the pending requirement "REQ-002"
    When the staged changes are validated
    Then the staged validation is valid
