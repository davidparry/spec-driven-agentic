# Executable spec for the branch gate — the one stop a generating run
# makes before it starts writing the project's files.
Feature: Branch gate
  As a developer about to let a run author real files
  I want to be offered a branch of my own once, up front
  So that the whole run is one thing I can keep, merge, or throw away

  Scenario: A repository is offered a branch, and the typed name is created
    Given the project is a git repository on "main"
    When the branch gate runs and the developer answers "newline support"
    Then the branch "spec/newline-support" is created
    And the developer was told the run writes the project's files directly

  Scenario: Pressing Enter takes the generated name
    Given the project is a git repository on "main"
    When the branch gate runs and the developer answers ""
    Then a branch starting with "spec/" is created

  Scenario: Declining leaves the run where it stands
    Given the project is a git repository on "main"
    When the branch gate runs and the developer answers "n"
    Then no branch is created

  # The flag exists so a project that is deliberately not under git, or
  # a CI job managing its own refs, is never asked and never probed.
  Scenario: --no-branch asks nothing and consults no git
    Given the project is a git repository on "main"
    When the branch gate runs with --no-branch
    Then no branch is created
    And the developer was asked nothing

  Scenario: Outside a repository the developer is told there is no undo
    Given the project is not a git repository
    When the branch gate runs and the developer answers "newlines"
    Then no branch is created
    And the developer was warned that there is no undo

  # A branch made over uncommitted work carries that work onto it,
  # which is the thing that decides whether you want one.
  Scenario: Uncommitted work is called out before the question
    Given the project is a git repository on "main" with uncommitted work
    When the branch gate runs and the developer answers ""
    Then the developer was warned about uncommitted work

  Scenario: A name git would refuse is refused here, and the run goes on
    Given the project is a git repository on "main"
    When the branch gate runs and the developer answers "bad..name"
    Then no branch is created
    And the developer was warned that the name is not usable

  Scenario: git refusing the branch is a warning, not the end of the run
    Given the project is a git repository on "main" where creating a branch fails
    When the branch gate runs and the developer answers "newlines"
    Then no branch is created
    And the developer was told the run continues on "main"

  # Piped into a script with nothing left to read: staying put is the
  # answer that changes nothing about where the run writes.
  Scenario: With no one to answer, no branch is created
    Given the project is a git repository on "main"
    When the branch gate runs with no one to answer
    Then no branch is created
