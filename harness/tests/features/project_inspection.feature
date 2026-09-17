# Executable spec for project inspection — language detection from
# marker files and runtime gating (the behavior of `spec inspect`).
Feature: Project inspection
  As a developer pointing the harness at a project
  I want its ecosystems detected and each runtime's presence reported
  So that execution is only attempted where a runtime exists, and
  nothing is ever installed for me

  Scenario: A Maven project is Java with Cucumber-JVM
    Given the project contains "pom.xml"
    When the project is inspected
    Then the language "Java" is detected with framework "Cucumber-JVM" and runtime "mvn"

  Scenario: A Gradle project is also Java
    Given the project contains "build.gradle"
    When the project is inspected
    Then the language "Java" is detected with framework "Cucumber-JVM" and runtime "gradle"

  Scenario: package.json alone is JavaScript with Cucumber-JS
    Given the project contains "package.json"
    When the project is inspected
    Then the language "JavaScript" is detected with framework "Cucumber-JS" and runtime "node"

  Scenario: package.json with tsconfig.json is TypeScript, not JavaScript
    Given the project contains "package.json"
    And the project contains "tsconfig.json"
    When the project is inspected
    Then the language "TypeScript" is detected with framework "Cucumber-JS" and runtime "node"
    And exactly 1 language is detected

  Scenario: A csproj file is .NET with Reqnroll
    Given the project contains a file with extension "csproj"
    When the project is inspected
    Then the language ".NET" is detected with framework "Reqnroll" and runtime "dotnet"

  Scenario: Cargo.toml is Rust with cucumber-rs
    Given the project contains "Cargo.toml"
    When the project is inspected
    Then the language "Rust" is detected with framework "cucumber-rs" and runtime "cargo"

  Scenario: A polyglot project reports every ecosystem
    Given the project contains "pom.xml"
    And the project contains "package.json"
    And the project contains a file with extension "csproj"
    And the project contains "Cargo.toml"
    When the project is inspected
    Then exactly 4 languages are detected

  Scenario: A present runtime is reported with its version
    Given the project contains "Cargo.toml"
    And the runtime "cargo" is installed with version "cargo 1.97.0"
    When the project is inspected
    Then the runtime for "Rust" is present with version "cargo 1.97.0"
    And the next step says all runtimes are present

  Scenario: A missing runtime disables execution but not authoring
    Given the project contains a file with extension "csproj"
    When the project is inspected
    Then the runtime for ".NET" is missing
    And the note for ".NET" contains "runtime_missing"
    And the note for ".NET" contains "The harness never installs runtimes."
    And the next step says some runtimes are missing

  Scenario: An empty directory lists the supported ecosystems
    When the project is inspected
    Then no languages are detected
    And the next step lists "Java (Cucumber-JVM)"
    And the next step lists "JavaScript (Cucumber-JS)"
    And the next step lists ".NET (Reqnroll)"
    And the next step lists "Rust (cucumber-rs)"
