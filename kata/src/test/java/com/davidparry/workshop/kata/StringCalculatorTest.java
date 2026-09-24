package com.davidparry.workshop.kata;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * Unit-level (TDD) tests generated from requirements during the workshop.
 * The behavior-level (BDD) spec lives in
 * {@code src/test/resources/features/string_calculator.feature} and runs
 * through Cucumber via {@link RunCucumberTest}.
 *
 * <p>Convention: tests are grouped and named by requirement ID. The MCP
 * server's {@code run_tests} tool executes both suites together and reports
 * one combined RED/GREEN result back to the agent and developer.
 */
class StringCalculatorTest {

    private final StringCalculator calculator = new StringCalculator();

    @Test
    @DisplayName("REQ-001: an empty string returns 0")
    void emptyStringReturnsZero() {
        assertThat(calculator.add("")).isZero();
    }

    @Test
    @DisplayName("REQ-002: a single number returns its value")
    void singleNumberReturnsItsValue() {
        assertThat(calculator.add("7")).isEqualTo(7);
    }

    // REQ-003 .. REQ-006: tests are generated live from acceptance criteria
    // by the agent during the workshop. Ask the agent to call
    // get_requirement("REQ-003") and write the failing tests here.
}
