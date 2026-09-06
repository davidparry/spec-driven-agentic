package com.davidparry.workshop.mcp.server.requirements;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Deterministic quality feedback on a requirement's wording — one rung above
 * {@link SpecValidator}. The validator decides whether the spec is
 * structurally usable; the refiner critiques how well it is written, so an
 * LLM can iterate: reword, validate, refine, repeat — until no findings
 * remain and the human signs off on the wording.
 */
public class RequirementRefiner {

    private static final Pattern AMBIGUOUS = Pattern.compile(
            "\\b(should|could|might|handles?|properly|appropriately|quickly|easily|robust|user-friendly|etc)\\b",
            Pattern.CASE_INSENSITIVE);
    private static final Pattern WHEN = Pattern.compile("\\bwhen\\b");
    private static final Pattern EDGE_CASE = Pattern.compile(
            "\\b(empty|blank|invalid|negative|error|missing|null|nan|exceptions?|rejected|refused|fails?|failure)\\b",
            Pattern.CASE_INSENSITIVE);
    /**
     * The outcome starts at the last word "then", never at a substring like
     * the one inside "strengthening".
     */
    private static final Pattern THEN_WORD = Pattern.compile("\\bthen\\b", Pattern.CASE_INSENSITIVE);
    private static final Pattern NUMBER = Pattern.compile("[0-9]");
    /**
     * A matched pair of delimiters, never a lone apostrophe: a single quote
     * only opens a literal when it does not sit against a letter or digit,
     * so "doesn't" and "the user's result" are not read as quoted values.
     */
    private static final Pattern QUOTED_LITERAL = Pattern.compile(
            "(?:\"[^\"]*\")|(?:^|[^A-Za-z0-9])(?:'[^']*'|`[^`]*`)(?:[^A-Za-z0-9]|$)");
    private static final Pattern SENTINEL_OUTCOME = Pattern.compile(
            "\\b(nan|null|nil|none|undefined|true|false|empty|blank|zero)\\b", Pattern.CASE_INSENSITIVE);
    private static final Pattern ERROR_OUTCOME = Pattern.compile(
            "\\b(errors?|exceptions?|invalid|rejected|refused|denied|fail(?:s|ed|ure)?|throws?|thrown|rais(?:es?|ed))\\b",
            Pattern.CASE_INSENSITIVE);
    /**
     * The affirmative half of a two-valued domain: a validation predicate
     * answers "valid" as exactly as a sum answers 3. The negative half is
     * already covered by {@link #ERROR_OUTCOME} (invalid, rejected, refused).
     */
    private static final Pattern PREDICATE_OUTCOME = Pattern.compile(
            "\\b(valid|accepted|allowed|permitted|matches|matched)\\b", Pattern.CASE_INSENSITIVE);
    /** Typed error names keep their casing: NumberFormatException, TypeError. */
    private static final Pattern TYPED_ERROR = Pattern.compile("\\b[A-Z][A-Za-z]*(?:Exception|Error)\\b");

    /** Returns an empty list when the wording is clean, otherwise every finding. */
    public List<String> review(Requirement requirement) {
        List<String> findings = new ArrayList<>();
        reviewStory(requirement.story(), findings);
        for (String criterion : requirement.acceptanceCriteria()) {
            reviewCriterion(criterion, findings);
        }
        reviewCoverage(requirement.acceptanceCriteria(), findings);
        return List.copyOf(findings);
    }

    private void reviewStory(String story, List<String> findings) {
        String lower = String.valueOf(story).toLowerCase(Locale.ROOT);
        if (!lower.contains("as a")) {
            findings.add("story: missing the actor - start with 'As a ...' so we know who this is for");
        }
        if (!lower.contains("so that")) {
            findings.add("story: missing the why - finish with 'so that ...' so the value is explicit");
        }
        for (String word : ambiguousWords(lower)) {
            findings.add("story: '" + word + "' is ambiguous - describe the observable behavior instead");
        }
    }

    private void reviewCriterion(String criterion, List<String> findings) {
        String lower = criterion.toLowerCase(Locale.ROOT);
        if (WHEN.matcher(lower).results().count() > 1) {
            findings.add("criterion \"" + criterion
                    + "\": covers more than one action - split it so each criterion has a single When");
        }
        int thenIndex = lastThenIndex(criterion);
        if (thenIndex >= 0 && !outcomeIsConcrete(criterion.substring(thenIndex))) {
            findings.add("criterion \"" + criterion
                    + "\": the outcome is not concrete - state the exact expected value after 'then'");
        }
        for (String word : ambiguousWords(lower)) {
            findings.add("criterion \"" + criterion + "\": '" + word + "' is ambiguous - state exactly what happens");
        }
    }

    private static int lastThenIndex(String criterion) {
        int index = -1;
        Matcher then = THEN_WORD.matcher(criterion);
        while (then.find()) {
            index = then.start();
        }
        return index;
    }

    /**
     * A deterministic outcome: a number, a quoted literal in double, single
     * or back quotes, a sentinel value (NaN, null, true, ...), definite
     * error wording (an error is raised, the input is rejected, ...), a
     * validation predicate (the number is valid), or a typed error name
     * like NumberFormatException.
     */
    private static boolean outcomeIsConcrete(String outcome) {
        return NUMBER.matcher(outcome).find()
                || QUOTED_LITERAL.matcher(outcome).find()
                || SENTINEL_OUTCOME.matcher(outcome).find()
                || ERROR_OUTCOME.matcher(outcome).find()
                || PREDICATE_OUTCOME.matcher(outcome).find()
                || TYPED_ERROR.matcher(outcome).find();
    }

    private void reviewCoverage(List<String> criteria, List<String> findings) {
        if (!criteria.isEmpty() && criteria.stream().noneMatch(RequirementRefiner::coversEdgeCase)) {
            findings.add("criteria: only happy paths - add at least one edge case "
                    + "(empty, invalid, or error input)");
        }
    }

    /**
     * An edge case announces itself by keyword, or by a bare "" literal -
     * the canonical empty-input case, often written without the word "empty".
     */
    private static boolean coversEdgeCase(String criterion) {
        return EDGE_CASE.matcher(criterion).find() || criterion.contains("\"\"");
    }

    private static Set<String> ambiguousWords(String lower) {
        Set<String> words = new LinkedHashSet<>();
        Matcher matcher = AMBIGUOUS.matcher(lower);
        while (matcher.find()) {
            words.add(matcher.group(1));
        }
        return words;
    }
}
