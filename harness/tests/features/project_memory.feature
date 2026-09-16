# Executable spec for project memory: language, libraries, and layout
# recorded in `.bdd-memory.json` and briefed into every LLM system prompt.
Feature: Project memory
  As a developer working a spec-driven loop
  I want the project's language, libraries, and structure remembered
  So that every model call is briefed with what this project actually is

  Scenario: Refreshing a Java project records language and libraries
    Given a project source file "pom.xml" containing:
      """
      <project>
        <dependency>
          <artifactId>cucumber-java</artifactId>
          <version>7.20.1</version>
          <scope>test</scope>
        </dependency>
      </project>
      """
    And a project source file "src/main/java/App.java" containing:
      """
      class App {}
      """
    When the project memory is refreshed
    Then the working tree file ".bdd-memory.json" contains "Java"
    And the working tree file ".bdd-memory.json" contains "Cucumber-JVM"
    And the working tree file ".bdd-memory.json" contains "Maven"
    And the working tree file ".bdd-memory.json" contains "cucumber-java"
    And the working tree file ".bdd-memory.json" contains "src/main/java"

  Scenario: A chosen language is kept when other markers appear later
    Given a project source file "pom.xml" containing:
      """
      <project/>
      """
    When the project memory is refreshed for language "rust"
    Then the working tree file ".bdd-memory.json" contains "Rust"
    And the working tree file ".bdd-memory.json" contains "cucumber-rs"

  Scenario: An LLM call is briefed with project memory
    Given a project source file "pom.xml" containing:
      """
      <project>
        <dependency>
          <artifactId>cucumber-java</artifactId>
          <version>7.20.1</version>
        </dependency>
      </project>
      """
    When the project memory is refreshed
    And a model call is made with system "You implement a project."
    Then the model system prompt contains "Project memory:"
    And the model system prompt contains "Language: Java (Cucumber-JVM)"
    And the model system prompt contains "cucumber-java 7.20.1"
    And the model system prompt contains "You implement a project."