package com.davidparry.workshop.mcp.server.requirements;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;

class RequirementRefinerTest {

    private final RequirementRefiner refiner = new RequirementRefiner();

    private static Requirement requirement(String story, List<String> criteria) {
        return new Requirement("REQ-001", "Title", "pending", story, criteria, null);
    }

    @Test
    @DisplayName("a well-worded requirement has no findings")
    void cleanRequirement() {
        Requirement r = requirement(
                "As a calculator user, I want commas to separate numbers so that lists just work.",
                List.of(
                        "Given the input \"1,2\", when add is called, then the result is 3",
                        "Given an empty string \"\", when add is called, then the result is 0"));
        assertThat(refiner.review(r)).isEmpty();
    }

    @Test
    @DisplayName("a story without an actor or a why is flagged")
    void storyShape() {
        Requirement r = requirement(
                "The calculator adds numbers.",
                List.of("Given an empty string \"\", when add is called, then the result is 0"));
        assertThat(refiner.review(r))
                .anyMatch(f -> f.contains("missing the actor"))
                .anyMatch(f -> f.contains("missing the why"));
    }

    @Test
    @DisplayName("ambiguous words in the story are flagged once each")
    void ambiguousStory() {
        Requirement r = requirement(
                "As a user, the calculator should handle input quickly and should be robust so that it works.",
                List.of("Given an empty string \"\", when add is called, then the result is 0"));
        List<String> findings = refiner.review(r);
        assertThat(findings)
                .anyMatch(f -> f.contains("story: 'should' is ambiguous"))
                .anyMatch(f -> f.contains("story: 'handle' is ambiguous"))
                .anyMatch(f -> f.contains("story: 'quickly' is ambiguous"))
                .anyMatch(f -> f.contains("story: 'robust' is ambiguous"));
        assertThat(findings.stream().filter(f -> f.contains("'should'")).count()).isEqualTo(1);
    }

    @Test
    @DisplayName("a null story is flagged rather than crashing")
    void nullStory() {
        Requirement r = requirement(null,
                List.of("Given an empty string \"\", when add is called, then the result is 0"));
        assertThat(refiner.review(r))
                .anyMatch(f -> f.contains("missing the actor"))
                .anyMatch(f -> f.contains("missing the why"));
    }

    @Test
    @DisplayName("a criterion with more than one When is told to split")
    void multiActionCriterion() {
        Requirement r = requirement(
                "As a user, I want addition so that sums are easy to get.",
                List.of("Given an empty calculator, when add is called and when reset is called, then the result is 0"));
        assertThat(refiner.review(r)).anyMatch(f -> f.contains("covers more than one action"));
    }

    @Test
    @DisplayName("a Then without a concrete expected value is flagged")
    void vagueOutcome() {
        Requirement r = requirement(
                "As a user, I want addition so that sums just work.",
                List.of("Given an empty string, when add is called, then the result is correct"));
        assertThat(refiner.review(r)).anyMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("a quoted literal counts as a concrete outcome")
    void quotedOutcomeIsConcrete() {
        Requirement r = requirement(
                "As a user, I want echo so that output is visible.",
                List.of("Given an empty input, when echo is called, then the output is \"\""));
        assertThat(refiner.review(r)).noneMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("sentinel values like NaN and null are concrete outcomes")
    void sentinelOutcomesAreConcrete() {
        Requirement r = requirement(
                "As a user, I want single integers parsed so that values are usable.",
                List.of(
                        "Given no prior input, when I enter \"abc\", then the calculator returns NaN",
                        "Given a missing entry, when lookup is called, then the result is null"));
        assertThat(refiner.review(r)).noneMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("definite error outcomes and typed error names are concrete")
    void errorOutcomesAreConcrete() {
        Requirement r = requirement(
                "As a user, I want parsing so that errors are visible.",
                List.of(
                        "Given an invalid delimiter, when add is called, then an error is raised",
                        "Given a negative number, when add is called, then a NumberFormatException propagates"));
        assertThat(refiner.review(r)).noneMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("single and back quoted literals count as concrete outcomes")
    void singleAndBackQuotedOutcomesAreConcrete() {
        Requirement r = requirement(
                "As a user, I want my phone number checked so that the form is submittable.",
                List.of(
                        "Given the user enters '123-456-7890', when the number is checked, then the result is 'ok'",
                        "Given an empty string \"\", when the number is checked, then the result is `no`"));
        assertThat(refiner.review(r)).noneMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("a validation verdict counts as a concrete outcome")
    void predicateOutcomesAreConcrete() {
        Requirement r = requirement(
                "As a user, I want my phone number input validated so that only numbers "
                        + "in the format XXX-XXX-XXXX or XXXXXXXXXX are accepted",
                List.of(
                        "Given the user enters '123-456-7890', when the system validates it, then the result is 'valid'",
                        "Given the user enters '123-45-6789', when the system validates it, then the result is 'invalid'"));
        assertThat(refiner.review(r)).isEmpty();
    }

    @Test
    @DisplayName("an unquoted verdict counts as a concrete outcome")
    void unquotedVerdictIsConcrete() {
        Requirement r = requirement(
                "As a clerk, I want postal codes checked so that mail reaches its destination.",
                List.of(
                        "Given a submitted form, when the postal code is checked, then the postal code is valid",
                        "Given a form with a missing postal code, when the postal code is checked, "
                                + "then an error is raised"));
        assertThat(refiner.review(r)).noneMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("a lone apostrophe is punctuation, not a quoted literal")
    void contractionsAreNotQuotedLiterals() {
        Requirement r = requirement(
                "As a user, I want entries read back so that storage is trustworthy.",
                List.of("Given a stored entry, when it is read, then the user's entry is unchanged"));
        assertThat(refiner.review(r)).anyMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("the outcome starts at the word 'then', never at a substring")
    void thenIsWordBounded() {
        Requirement r = requirement(
                "As a user, I want sums so that errors are visible.",
                List.of("Given an empty string \"\", when add is called, then the result is 3, strengthening the total"));
        assertThat(refiner.review(r)).isEmpty();
    }

    @Test
    @DisplayName("a NaN edge case counts for coverage")
    void nanCountsAsEdgeCaseCoverage() {
        Requirement r = requirement(
                "As a user, I want single integers parsed so that values are usable.",
                List.of(
                        "Given the input \"1,2\", when add is called, then the result is 3",
                        "Given no prior input, when I enter \"abc\", then the calculator returns NaN"));
        assertThat(refiner.review(r)).isEmpty();
    }

    @Test
    @DisplayName("a bare empty-string literal counts as edge-case coverage")
    void emptyLiteralCountsAsEdgeCaseCoverage() {
        Requirement r = requirement(
                "As a user, I want sums so that totals come from one input.",
                List.of(
                        "Given the input \"1,2\", when add is called, then the result is 3",
                        "Given the input strings \"\" and \"0\", when the add function is called, then the result is \"0\""));
        assertThat(refiner.review(r)).isEmpty();
    }

    @Test
    @DisplayName("a criterion without a Then skips the concreteness check (the validator owns shape)")
    void noThenSkipsConcreteness() {
        Requirement r = requirement(
                "As a user, I want addition so that sums just work.",
                List.of("Given an empty string, when add is called"));
        assertThat(refiner.review(r)).noneMatch(f -> f.contains("the outcome is not concrete"));
    }

    @Test
    @DisplayName("ambiguous words inside a criterion are flagged")
    void ambiguousCriterion() {
        Requirement r = requirement(
                "As a user, I want addition so that sums just work.",
                List.of("Given an invalid input, when add is called, then it should work properly"));
        assertThat(refiner.review(r))
                .anyMatch(f -> f.contains("criterion") && f.contains("'should' is ambiguous"))
                .anyMatch(f -> f.contains("criterion") && f.contains("'properly' is ambiguous"));
    }

    @Test
    @DisplayName("happy-path-only criteria get an edge-case nudge")
    void happyPathOnly() {
        Requirement r = requirement(
                "As a user, I want commas to separate numbers so that lists just work.",
                List.of("Given the input \"1,2\", when add is called, then the result is 3"));
        assertThat(refiner.review(r)).anyMatch(f -> f.contains("only happy paths"));
    }

    @Test
    @DisplayName("no criteria means no edge-case nudge (the validator owns that gap)")
    void noCriteriaNoCoverageNudge() {
        Requirement r = requirement(
                "As a user, I want commas to separate numbers so that lists just work.",
                List.of());
        assertThat(refiner.review(r)).noneMatch(f -> f.contains("only happy paths"));
    }
}
