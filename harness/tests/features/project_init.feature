# Executable spec for `spec init` — the per-language scaffold that is
# safe to re-run because existing files are skipped, never overwritten.
Feature: Project initialization
  As a developer starting a spec-driven project in an empty directory
  I want one command to lay down the build file, Cucumber runner, and empty spec
  So that the first requirement can be drafted immediately

  Scenario: A fresh directory gets the Java scaffold
    When the project is initialized for "java" named "String Calculator"
    Then the init report shows language "Java" with framework "Cucumber-JVM"
    And 6 scaffold files are created and 0 are skipped
    And the working tree file "pom.xml" contains "string-calculator"
    And the working tree file "requirements/requirements.json" contains "String Calculator"
    And the working tree file ".spec.toml" contains "http://localhost:11434"
    And the working tree file ".spec.toml" contains "[tools.profiles]"
    And the working tree file ".spec.toml" contains "implement = ["
    And the working tree file ".spec.toml" contains "command_run"
    And the init next step mentions "spec draft"

  Scenario: A fresh directory gets the Rust scaffold
    When the project is initialized for "rust" named "String Calculator"
    Then the init report shows language "Rust" with framework "cucumber-rs"
    And 7 scaffold files are created and 0 are skipped
    And the working tree file "Cargo.toml" contains "cucumber"
    And the working tree file "tests/cucumber.rs" contains "World::run"

  Scenario: Existing files are skipped, never overwritten
    Given the working tree file "pom.xml" already contains "my hand-written pom"
    When the project is initialized for "java" named "Calc"
    Then 5 scaffold files are created and 1 is skipped
    And a skipped file is "pom.xml"
    And the working tree file "pom.xml" contains "my hand-written pom"
