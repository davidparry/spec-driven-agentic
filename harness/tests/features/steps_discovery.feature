Feature: Step-definition discovery
  The harness parses every feature file, scans the project's step-definition
  sources for the detected framework, and reports the steps that have no
  matching definition. Those are exactly the steps bdd steps generate
  will scaffold; nothing is ever executed or installed to find them.

  Background:
    Given a project feature file "features/calc.feature" containing:
      """
      Feature: String calculator

        Scenario: Adds two numbers
          Given a calculator
          When add is called with "1,2"
          Then the result is 3
      """

  Scenario: Undefined steps are reported with the framework
    Given a Java project marker
    And a project source file "src/test/java/Steps.java" containing:
      """
      public class Steps {
          @Given("a calculator")
          public void aCalculator() {}
      }
      """
    When missing steps are reported
    Then the missing report names language "Java" and framework "Cucumber-JVM"
    And 2 steps are missing
    And a missing "When" step is "add is called with \"1,2\""
    And a missing "Then" step is "the result is 3"
    And the missing next step mentions "bdd steps generate"

  Scenario: Cucumber expressions in definitions match concrete steps
    Given a Java project marker
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
    When missing steps are reported
    Then no steps are missing
    And the missing next step mentions "bdd test"
