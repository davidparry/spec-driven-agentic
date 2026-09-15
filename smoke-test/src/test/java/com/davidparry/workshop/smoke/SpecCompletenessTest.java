package com.davidparry.workshop.smoke;

import tools.jackson.databind.JsonNode;
import tools.jackson.databind.ObjectMapper;

import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * The executable proof that this module's SDD/BDD/TDD triangle is closed:
 * every requirement in the client's own spec is implemented, every
 * requirement has tagged Gherkin scenarios in the feature file it names,
 * and every scenario tag traces back to a spec requirement. Coverage of the
 * TDD layer is enforced separately by the JaCoCo 100% check.
 */
class SpecCompletenessTest {

    /** Surefire runs with the module directory as the working directory. */
    private static final Path MODULE_DIR = Path.of("").toAbsolutePath();
    private static final Path REPO_ROOT = MODULE_DIR.getParent();
    private static final Pattern TAG = Pattern.compile("@(CLI-\\d+)");

    private static List<JsonNode> requirements;

    @BeforeAll
    static void loadSpec() throws IOException {
        JsonNode spec = new ObjectMapper().readTree(
                Files.readString(MODULE_DIR.resolve("requirements/requirements.json")));
        requirements = new ArrayList<>();
        spec.path("requirements").forEach(requirements::add);
    }

    @Test
    @DisplayName("SDD: the client has its own spec and every requirement is implemented")
    void everyRequirementIsImplemented() {
        assertThat(requirements).isNotEmpty();
        assertThat(requirements).allSatisfy(r ->
                assertThat(r.path("status").asString()).isEqualTo("implemented"));
    }

    @Test
    @DisplayName("SDD: every requirement carries acceptance criteria and names its feature file")
    void everyRequirementHasCriteriaAndFeatureFile() {
        assertThat(requirements).allSatisfy(r -> {
            String id = r.path("id").asString();
            assertThat(r.path("acceptanceCriteria")).as("%s acceptance criteria", id).isNotEmpty();
            assertThat(r.path("featureFile").asString()).as("%s featureFile", id).isNotBlank();
            assertThat(REPO_ROOT.resolve(r.path("featureFile").asString()))
                    .as("%s feature file exists", id).exists();
        });
    }

    @Test
    @DisplayName("BDD: every requirement has at least one tagged scenario in its feature file")
    void everyRequirementHasATaggedScenario() throws IOException {
        for (JsonNode r : requirements) {
            String feature = Files.readString(REPO_ROOT.resolve(r.path("featureFile").asString()));
            assertThat(feature)
                    .as("feature file %s has a scenario tagged @%s",
                            r.path("featureFile").asString(), r.path("id").asString())
                    .contains("@" + r.path("id").asString());
        }
    }

    @Test
    @DisplayName("BDD: every scenario tag in the feature file traces back to a spec requirement")
    void everyScenarioTagTracesToTheSpec() throws IOException {
        Set<String> specIds = requirements.stream()
                .map(r -> r.path("id").asString())
                .collect(Collectors.toSet());
        Set<String> featureFiles = requirements.stream()
                .map(r -> r.path("featureFile").asString())
                .collect(Collectors.toSet());
        for (String featureFile : featureFiles) {
            String feature = Files.readString(REPO_ROOT.resolve(featureFile));
            Matcher matcher = TAG.matcher(feature);
            while (matcher.find()) {
                assertThat(specIds)
                        .as("tag @%s in %s exists in the spec", matcher.group(1), featureFile)
                        .contains(matcher.group(1));
            }
        }
    }

    @Test
    @DisplayName("BDD: the Cucumber suite actually executes the tagged feature file")
    void cucumberSuiteExecutesTheFeature() {
        List<String> featureFiles = requirements.stream()
                .map(r -> r.path("featureFile").asString())
                .distinct()
                .toList();
        assertThat(featureFiles).allSatisfy(f ->
                assertThat(f).startsWith("smoke-test/src/test/resources/features/"));
    }
}
