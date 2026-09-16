Feature: Greenfield mode
  bdd greenfield walks the whole loop from an empty directory: scaffold,
  spec draft, tagged scenarios, generated tests, RED, implementation,
  GREEN, refactor, mark-implemented. Exactly two gates need the human -
  the spec wording approval and the generated-test review. Execution
  happens only when the language's runtime is present; authoring always
  stands on its own.

  Scenario: The full loop from an empty directory to an implemented requirement
    Given the greenfield test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test.emptyStringReturnsZero: expected 0"
      1 tests and 0 failures
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      cobol
      java
      String Calculator
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      <empty>
      y
      tidy the names
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the working tree file "pom.xml" contains "cucumber-junit-platform-engine"
    And the working tree file ".bdd-memory.json" contains "Java"
    And the working tree file ".bdd-memory.json" contains "cucumber-java"
    And the working tree file "features/empty-string-returns-zero.feature" contains "@REQ-001"
    And the working tree file "requirements/requirements.json" contains "implemented"
    And the developer was told a finding containing "Generating the unit test for REQ-001 - working ..."
    And the developer was told a finding containing "Running the tests - working ..."
    And the developer was told a finding containing "Req001Test.emptyStringReturnsZero: expected 0"
    And the developer was told a finding containing "Saving status - working ..."
    And the developer was told a finding containing "REQ-001 is implemented. Loop closed."

  Scenario: Declining the generated-test review discards the generation
    Given a Java project marker
    And an empty working spec
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      n
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield next step starts with "Generation was discarded"
    And nothing is staged at the spec path

  Scenario: Declining to stage the drafted wording ends the run
    Given a Java project marker
    And an empty working spec
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      n
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield next step starts with "Nothing was staged"

  Scenario: A missing runtime stops execution but authoring stands
    Given a Java project marker
    And an empty working spec
    And the greenfield test runs will report:
      """
      runtime "JDK" missing with hint "Install a JDK 17+ and Maven."
      """
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield next step starts with "Authoring is complete"
    And the developer was told a finding containing "Runtime missing (JDK): Install a JDK 17+ and Maven."
    And the working tree file "features/empty-string-returns-zero.feature" contains "@REQ-001"

  Scenario: No detectable build tool stops execution but authoring stands
    Given a Java project marker
    And an empty working spec
    And no test runner is detectable because "no supported build tool found"
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield next step starts with "Authoring is complete"
    And the developer was told a finding containing "no supported build tool found"

  Scenario: Pausing on RED hands the loop back to the developer
    Given a Java project marker
    And an empty working spec
    And the greenfield test runs will report:
      """
      2 tests and 2 failures
      """
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      stop
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield phase is "RED"
    And the greenfield next step starts with "Paused on RED"

  Scenario: A runtime that disappears between reruns still leaves authoring intact
    Given a Java project marker
    And an empty working spec
    And the greenfield test runs will report:
      """
      1 tests and 1 failures
      runtime "JDK" missing with hint "The JDK was uninstalled."
      """
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      <empty>
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield next step starts with "Authoring is complete"

  Scenario: A refactor that breaks the bar pauses before mark-implemented
    Given a Java project marker
    And an empty working spec
    And the greenfield test runs will report:
      """
      1 tests and 1 failures
      1 tests and 0 failures
      1 tests and 1 failures
      """
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      <empty>
      y
      inline the parser
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield phase is "RED"
    And the greenfield next step starts with "The refactor broke the bar"
    And the working tree file "requirements/requirements.json" contains "pending"

  Scenario: A build failure surfaces as an error, not a crash
    Given a Java project marker
    And an empty working spec
    And the greenfield test runs will report:
      """
      failed "mvn exploded"
      """
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      """
    When the greenfield loop runs
    Then the greenfield error is "mvn exploded"

  Scenario: A resolved model polishes generation and unshaped criteria are skipped
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      not usable output
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 1 failures
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      sum comma separated numbers
      Comma separated numbers are summed
      As a calculator user, I want comma-separated numbers to be summed so that I can add many values.
      Given the input "1,2", when add is called, then the result is 3
      Given an empty string "" when add is called then the result is 0
      <empty>
      y
      y
      <empty>
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the developer was told a finding containing "Generation uses model scripted-model"
    And the developer was told a finding containing "Splitting the description into requirements with scripted-model - working ..."
    And the developer was told a finding containing "The description gave no complete requirement - drafting manually."
    And the developer was told a finding containing "Skipping criterion"
    And the working tree file "features/comma-separated-numbers-are-summed.feature" contains "@REQ-001"

  Scenario: A described feature drives the wizard from proposals to GREEN
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      [{"title": "Empty string returns zero", "story": "As a calculator user, I want an empty string to return 0 so that no input is a safe default.", "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 1 failures
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      empty input means zero
      <empty>
      <empty>
      <empty>
      <empty>
      y
      y
      <empty>
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the developer was told a finding containing "The description holds 1 requirement(s):"
    And the developer was asked "REQ-001 title [Empty string returns zero] (Enter keeps it):"
    And the working tree file "features/empty-string-returns-zero.feature" contains "@REQ-001"
    And the working tree file "requirements/requirements.json" contains "implemented"
    And the developer was told a finding containing "Saving status - working ..."
    And the developer was told a finding containing "REQ-001 is implemented. Loop closed."

  Scenario: A closed loop offers the next pending requirement and continues
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      [{"title": "Empty string returns zero", "story": "As a calculator user, I want an empty string to return 0 so that no input is a safe default.", "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}, {"title": "Blank input is rejected", "story": "As a calculator user, I want blank input rejected so that mistakes surface early.", "acceptanceCriteria": ["Given a blank string \" \", when add is called, then an error is raised"]}]
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 0 failures
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      calculator basics
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      y
      y
      n
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      y
      y
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the greenfield next step starts with "Every requirement is implemented"
    And the developer was told a finding containing "Still pending in the spec:"
    And the developer was told a finding containing "1. REQ-002 Blank input is rejected"
    And the developer was asked "Which pending requirement next? [1-1, Enter for 1] (n stops):"
    And the developer was told a finding containing "REQ-002 is implemented. Loop closed."
    And the working tree file "features/blank-input-is-rejected.feature" contains "@REQ-002"
    And the working tree file "requirements/requirements.json" contains "implemented"

  Scenario: Declining the pending-requirement offer ends the run with the loop closed
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      [{"title": "Empty string returns zero", "story": "As a calculator user, I want an empty string to return 0 so that no input is a safe default.", "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}, {"title": "Blank input is rejected", "story": "As a calculator user, I want blank input rejected so that mistakes surface early.", "acceptanceCriteria": ["Given a blank string \" \", when add is called, then an error is raised"]}]
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      calculator basics
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      <empty>
      y
      y
      n
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the greenfield next step starts with "The next requirement is waiting"
    And the developer was told a finding containing "Still pending in the spec:"
    And the developer was asked "Which pending requirement next? [1-1, Enter for 1] (n stops):"
    And the working tree file "requirements/requirements.json" contains "pending"

  Scenario: Pressing Enter on RED lets the model attempt the implementation
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 0; } }"}]
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test.empty_string_returns_zero: TODO: assert"
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      <empty>
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      <empty>
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the developer was asked "Press Enter to let the model attempt the implementation and rerun the tests, enter a number to attempt up to that many times without asking again, or type stop to pause here:"
    And the developer was told a finding containing "Generating an implementation attempt - working ..."
    # The narration prints the complete (absolute) path; the assertion
    # keeps only the stable, root-agnostic tail.
    And the developer was told a finding containing "src/main/java/Kata.java (llm)."
    And the developer was told a finding containing "Updated "
    And the working tree file "src/main/java/Kata.java" contains "public class Kata"
    And the persisted attempt log holds 0 attempts for "REQ-001"

  Scenario: A number at the RED prompt buys that many attempts without asking again
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 1; } }"}]
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test.empty_string_returns_zero: TODO: assert"
      1 tests and 1 failures detailed "Req001Test.empty_string_returns_zero: expected 0 but was 1"
      1 tests and 1 failures detailed "Req001Test.empty_string_returns_zero: expected 0 but was 1"
      """
    And the developer will answer:
      """
      <empty>
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      2
      stop
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield next step starts with "Paused on RED"
    And the developer was told a finding containing "Attempt 1 of 2."
    And the developer was told a finding containing "Attempt 2 of 2."
    And the persisted attempt log holds 2 attempts for "REQ-001"

  Scenario: A green bar mid-budget stops the automatic attempts early
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 0; } }"}]
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test.empty_string_returns_zero: TODO: assert"
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      <empty>
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      5
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the developer was told a finding containing "Attempt 1 of 5."
    And the persisted attempt log holds 0 attempts for "REQ-001"

  Scenario: A paused RED loop keeps the attempt so the next try is briefed with it
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 1; } }"}]
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test.empty_string_returns_zero: TODO: assert"
      1 tests and 1 failures detailed "Req001Test.empty_string_returns_zero: expected 0 but was 1"
      """
    And the developer will answer:
      """
      <empty>
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      <empty>
      stop
      """
    When the greenfield loop runs
    Then the greenfield run is not completed
    And the greenfield next step starts with "Paused on RED"
    And the persisted attempt log holds 1 attempt for "REQ-001"

  Scenario: An unusable implementation reply hands the loop back to the developer
    Given a Java project marker
    And an empty working spec
    And the model will reply:
      """
      not usable output
      """
    And a greenfield model is resolved
    And the greenfield test runs will report:
      """
      1 tests and 1 failures
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      <empty>
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      <empty>
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the developer was told a finding containing "The model's reply held no usable file update. Implement by hand instead."
    And the working tree file "src/main/java/Kata.java" does not exist

  Scenario: Steps that are already defined are not regenerated
    Given a Java project marker
    And an empty working spec
    And a project source file "src/test/java/steps/CalculatorSteps.java" containing:
      """
      package steps;
      import io.cucumber.java.en.*;
      public class CalculatorSteps {
          @Given("an empty string {string}")
          public void anEmptyString(String s) {}
          @When("add is called")
          public void addIsCalled() {}
          @Then("the result is {int}")
          public void theResultIs(int n) {}
      }
      """
    And the greenfield test runs will report:
      """
      1 tests and 1 failures
      1 tests and 0 failures
      """
    And the developer will answer:
      """
      Empty string returns zero
      As a calculator user, I want an empty string to return 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      <empty>
      y
      y
      <empty>
      n
      """
    When the greenfield loop runs
    Then the greenfield run completes with phase "GREEN"
    And the working tree file "src/test/java/GeneratedSteps.java" does not exist
