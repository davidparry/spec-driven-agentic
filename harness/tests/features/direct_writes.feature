# Executable spec for the work tree — what happens to the project's
# files when the harness authors a change.
Feature: Direct writes
  As a developer whose project the harness is editing
  I want every authored change written straight to the file it belongs in
  So that my editor, my build, and git diff see it the moment it is made,
  and git is what I undo it with

  @HARNESS-004
  Scenario: An authored file lands in the project immediately
    Given the feature file "features/calc.feature" is created named "Calc"
    Then the working tree file "features/calc.feature" contains "Feature: Calc"

  Scenario: A second mutation builds on what the first one wrote
    Given the feature file "features/calc.feature" is created named "Calc"
    When a scenario "Empty string" tagged "REQ-001" is added to "features/calc.feature"
    And a scenario "Single number" tagged "REQ-002" is added to "features/calc.feature"
    Then the working tree file "features/calc.feature" contains "Scenario: Empty string"
    And the working tree file "features/calc.feature" contains "Scenario: Single number"

  Scenario: A write reports whether it created or modified the file
    When "notes.md" is written with "first"
    Then the write is reported as a "create"
    When "notes.md" is written with "second"
    Then the write is reported as a "modify"
    And the working tree file "notes.md" contains "second"

  # The path jail. Paths reach the work tree from a model's reply as
  # often as from a human, so a refusal is the expected outcome, not an
  # edge case.
  Scenario Outline: A path that escapes the project is refused
    When "<path>" is written with "x"
    Then the write is refused
    And no file was written outside the project

    Examples:
      | path             |
      | /etc/passwd      |
      | ../outside.txt   |
      | ~/escape.txt     |

  Scenario: The harness's own state directory is not writable through the work tree
    When ".spec/config.toml" is written with "x"
    Then the write is refused
    And the write refusal mentions ".spec/ holds"

  # The file is replaced by a rename from a scratch file in the same
  # directory, so a reader never sees a half-written source file.
  Scenario: A write leaves no scratch file behind
    When "notes.md" is written with "done"
    Then no scratch file is left in the project root
