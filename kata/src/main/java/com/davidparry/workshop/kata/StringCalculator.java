package com.davidparry.workshop.kata;

/**
 * The String Calculator kata — the code under test for this workshop.
 *
 * <p>Requirements REQ-001 and REQ-002 are already implemented so the project
 * builds green out of the box. The remaining requirements (see
 * {@code requirements/requirements.json}, the SDD spec) are driven during the
 * workshop: an agent turns each requirement's acceptance criteria into an
 * executable Gherkin scenario (BDD) and failing tests (TDD) — RED — you
 * implement the behavior (GREEN), then clean up (REFACTOR).
 */
public class StringCalculator {

    /**
     * Adds the numbers in a delimited string.
     *
     * @param input a string of numbers, e.g. "" or "1" or "1,2"
     * @return the sum of the numbers; 0 for an empty string
     */
    public int add(String input) {
        // REQ-001: empty string returns 0
        if (input == null || input.isBlank()) {
            return 0;
        }
        // REQ-002: a single number returns its value
        // REQ-003+ are implemented live during the workshop.
        return Integer.parseInt(input.trim());
    }
}
