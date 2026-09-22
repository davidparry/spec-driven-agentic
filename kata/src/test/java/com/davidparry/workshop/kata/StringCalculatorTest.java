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

    @Test
    @DisplayName("REQ-003: Given \"1,2\", when add is called, then the result is 3")
    void shouldSumTwoNumbersSeparatedByComma() {
        assertThat(calculator.add("1,2")).isEqualTo(3);
    }

    @Test
    @DisplayName("REQ-003: Given \"10,20\", when add is called, then the result is 30")
    void shouldSumTwoMultiDigitNumbersSeparatedByComma() {
        assertThat(calculator.add("10,20")).isEqualTo(30);
    }
}
