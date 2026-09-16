Feature: Hybrid generation into staging
  Step definitions and unit tests are generated from deterministic
  templates that always work. When an LLM model is resolved its output is
  preferred - but only after validation; anything unusable falls back to
  the template silently. Generated code lands in the staging area, never
  in working files, so the human reviews before anything is applied.

  Background:
    Given a Java project marker
    And a project feature file "features/calc.feature" containing:
      """
      Feature: String calculator

        Scenario: Adds two numbers
          Given a calculator
          When add is called with "1,2"
          Then the result is 3
      """

  Scenario: Without a model the template is staged
    When step definitions are generated without a model
    Then the generation is staged at "src/test/java/GeneratedSteps.java" from "template"
    And the staged file "src/test/java/GeneratedSteps.java" contains "@Given(\"a calculator\")"
    And the staged file "src/test/java/GeneratedSteps.java" contains "PendingException"
    And the working tree has no file "src/test/java/GeneratedSteps.java"

  Scenario: Steps that collapse to one expression get one definition, not duplicates
    Given a project feature file "features/more.feature" containing:
      """
      Feature: More sums

        Scenario: Bigger numbers
          Then the result is 5
      """
    When step definitions are generated without a model
    Then the staged file "src/test/java/GeneratedSteps.java" defines "@Then(\"the result is {int}\")" exactly once

  Scenario: Validated model output is preferred over the template
    Given the model will reply:
      """
      ```java
      import io.cucumber.java.en.Given;

      public class GeneratedSteps {
          @Given("a calculator") public void polished() {}
      }
      ```
      """
    When step definitions are generated with the model
    Then the generation is staged at "src/test/java/GeneratedSteps.java" from "llm"
    And the staged file "src/test/java/GeneratedSteps.java" contains "polished"

  Scenario: Unusable model output falls back to the template silently
    Given the model will reply:
      """
      I cannot help with that request.
      """
    When step definitions are generated with the model
    Then the generation is staged at "src/test/java/GeneratedSteps.java" from "template"
    And the staged file "src/test/java/GeneratedSteps.java" contains "PendingException"

  Scenario: Generating with every step already defined is refused
    Given a project source file "src/test/java/Steps.java" containing:
      """
      public class Steps {
          @Given("a calculator")
          public void aCalculator() {}
          @When("add is called with {string}")
          public void add(String input) {}
          @Then("the result is {int}")
          public void result(int value) {}
      }
      """
    When generating step definitions fails
    Then the generation error is "Every step already has a definition - nothing to generate."

  Scenario: A failing unit test is staged from a requirement's criteria
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    When a unit test is generated for "REQ-001" without a model
    Then the generation is staged at "src/test/java/Req001Test.java" from "template"
    And the staged file "src/test/java/Req001Test.java" contains "fail(\"TODO: assert - Given a, when b, then 3\")"
    And the working tree has no file "src/test/java/Req001Test.java"

  Scenario: A unit test for an unknown requirement is refused
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    When generating a unit test for "REQ-999" fails
    Then the generation error is "No requirement with id REQ-999. Call spec list to see valid ids."

  Scenario: A RED run's failures drive a model implementation attempt
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And a persisted RED run failing with "Req001Test.case: TODO: assert - Given a, when b, then 3"
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 3; } }"}]
      """
    When an implementation is generated for "REQ-001" with the model
    Then the implementation staged "src/main/java/Kata.java" from the model
    And the staged file "src/main/java/Kata.java" contains "public class Kata"
    And the working tree has no file "src/main/java/Kata.java"
    And the persisted attempt log holds 1 attempt for "REQ-001"

  Scenario: Every attempt is logged so the next one is briefed with the history
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And a persisted RED run failing with "Req001Test.case: expected 3 but was 0"
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 3; } }"}]
      """
    When an implementation is generated for "REQ-001" with the model
    And an implementation is generated for "REQ-001" with the model
    Then the persisted attempt log holds 2 attempts for "REQ-001"

  Scenario: The implement preflight names every gap and the step to take instead
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    When implement readiness is checked for "REQ-001"
    Then the implement readiness is not ready
    And a readiness finding contains "No RED test run is recorded - run bdd test first"
    And a readiness finding contains "No scenario is tagged @REQ-001"
    And a readiness finding contains "run bdd steps generate"
    And a readiness finding contains "bdd unittest generate REQ-001"
    And the readiness next step contains "bdd test"
    And the readiness asset "src/test/java/Req001Test.java" is missing
    And the readiness asset "src/main/java/Kata.java" is missing

  Scenario: The implement preflight is clean when every prerequisite exists
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And a project feature file "features/tagged.feature" containing:
      """
      Feature: Tagged calculator

        @REQ-001
        Scenario: Adds tagged numbers
          Given a calculator
          When add is called with "1,2"
          Then the result is 3
      """
    And a project source file "src/test/java/Steps.java" containing:
      """
      public class Steps {
          @Given("a calculator")
          public void aCalculator() {}
          @When("add is called with {string}")
          public void add(String input) {}
          @Then("the result is {int}")
          public void result(int value) {}
      }
      """
    And a project source file "src/test/java/Req001Test.java" containing:
      """
      public class Req001Test {}
      """
    And a persisted RED run failing with "Req001Test.case: TODO: assert - Given a, when b, then 3"
    When implement readiness is checked for "REQ-001"
    Then the implement readiness is ready
    And the readiness next step contains "bdd implement REQ-001 can run"
    And the readiness asset "features/tagged.feature" is present
    And the readiness asset "src/test/java/Req001Test.java" is present

  Scenario: A GREEN bar means there is nothing to implement
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And the persisted TDD phase is "GREEN"
    When implement readiness is checked for "REQ-001"
    Then the implement readiness is not ready
    And a readiness finding contains "The bar is GREEN - there is nothing to implement"

  Scenario: When the preflight blocks, the model advises the next step
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And the model will reply:
      """
      Not yet - run bdd test first to record the RED bar, then bdd implement REQ-001.
      """
    When the model is asked for implement advice on "REQ-001"
    Then the implement readiness is not ready
    And the implement advice is "Not yet - run bdd test first to record the RED bar, then bdd implement REQ-001."

  Scenario: Status puts staged changes before everything else
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And raw content is staged at "src/main/java/Kata.java":
      """
      public class Kata {}
      """
    When the project status is checked
    Then the status next step contains "1 staged file(s) await review"
    And the status next step contains "bdd changes commit"
    And the status lists 1 staged file and 1 requirement

  Scenario: Status on GREEN points to mark-implemented for the requirement in flight
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And a project feature file "features/tagged.feature" containing:
      """
      Feature: Tagged calculator

        @REQ-001
        Scenario: Adds tagged numbers
          Given a calculator
          When add is called with "1,2"
          Then the result is 3
      """
    And a project source file "src/test/java/Steps.java" containing:
      """
      public class Steps {
          @Given("a calculator")
          public void aCalculator() {}
          @When("add is called with {string}")
          public void add(String input) {}
          @Then("the result is {int}")
          public void result(int value) {}
      }
      """
    And a project source file "src/test/java/Req001Test.java" containing:
      """
      public class Req001Test {}
      """
    And the persisted TDD phase is "GREEN"
    When the project status is checked
    Then the status next step contains "bdd spec mark-implemented REQ-001"
    And the status of "REQ-001" holds 0 findings

  Scenario: Status names the earliest gap on the road to implemented
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    When the project status is checked
    Then the status next step contains "No scenario is tagged @REQ-001"
    And the status of "REQ-001" holds 3 findings

  Scenario: An implementation attempt without a model is refused
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    When generating an implementation for "REQ-001" without a model fails
    Then the generation error is "No model resolved - implement by hand and rerun bdd test."

  Scenario: An implementation reply outside the project is refused
    Given a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And the model will reply:
      """
      [{"path": "/etc/passwd", "content": "nope"}]
      """
    When generating an implementation for "REQ-001" with the model fails
    Then the generation error is "The model's reply held no usable file update."
