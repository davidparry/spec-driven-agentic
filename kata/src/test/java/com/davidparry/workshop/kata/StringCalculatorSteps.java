package com.davidparry.workshop.kata;

import io.cucumber.java.en.Given;
import io.cucumber.java.en.Then;
import io.cucumber.java.en.When;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.catchThrowable;

/**
 * Step definitions binding the Gherkin scenarios in
 * {@code src/test/resources/features} to the production code.
 *
 * <p>The steps for negative-number rejection (REQ-006) are included ahead of
 * time so the agent only has to write the scenario — mirroring how a team
 * builds up a reusable step vocabulary.
 */
public class StringCalculatorSteps {

    private StringCalculator calculator;
    private int result;
    private Throwable thrown;

    @Given("a string calculator")
    public void aStringCalculator() {
        calculator = new StringCalculator();
    }

    @When("I add {string}")
    public void iAdd(String input) {
        // Gherkin cannot express a literal newline in a quoted string, so
        // scenarios write "\n" and we unescape it here.
        String unescaped = input.replace("\\n", "\n");
        thrown = catchThrowable(() -> result = calculator.add(unescaped));
    }

    @Then("the result is {int}")
    public void theResultIs(int expected) {
        assertThat(thrown).isNull();
        assertThat(result).isEqualTo(expected);
    }

    @Then("an IllegalArgumentException is thrown with a message containing {string}")
    public void anExceptionIsThrownContaining(String fragment) {
        assertThat(thrown)
                .isInstanceOf(IllegalArgumentException.class)
                .hasMessageContaining(fragment);
    }
}
