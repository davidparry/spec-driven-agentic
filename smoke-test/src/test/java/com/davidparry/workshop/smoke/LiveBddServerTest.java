package com.davidparry.workshop.smoke;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIfSystemProperty;
import org.junit.jupiter.api.io.TempDir;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;

import static org.assertj.core.api.Assertions.assertThat;

@EnabledIfSystemProperty(named = "bdd.binary", matches = ".+")
class LiveBddServerTest {

    @TempDir
    Path project;

    @Test
    @DisplayName("the live bdd binary serves exactly the 25 planned tools")
    void liveSweep() throws IOException {
        seedProject(project);
        Path bdd = Path.of(System.getProperty("bdd.binary"));
        try (SdkToolClient client = new SdkToolClient(project, bdd)) {
            ToolSweep.SweepReport report = new ToolSweep().run(client, new Narrator(line -> {
            }), false);
            assertThat(report.discovered()).hasSize(25);
            assertThat(report.missing()).isEmpty();
            assertThat(report.unexpected()).isEmpty();
            assertThat(report.failures()).isEmpty();
            assertThat(report.called()).isNotEmpty();
        }
    }

    static void seedProject(Path root) throws IOException {
        Files.createDirectories(root.resolve("requirements"));
        Files.createDirectories(root.resolve("features"));
        Files.writeString(
                root.resolve("requirements/requirements.json"),
                """
                {
                  "project": "Sweep",
                  "requirements": [
                    {
                      "id": "REQ-001",
                      "title": "Adds two numbers",
                      "status": "pending",
                      "story": "As a user, I want sums so that I can add.",
                      "acceptanceCriteria": [
                        "Given the input \\"1,2\\", when add is called, then the result is 3"
                      ],
                      "featureFile": "features/calc.feature"
                    }
                  ]
                }
                """);
        Files.writeString(
                root.resolve("features/calc.feature"),
                """
                @REQ-001
                Feature: Calc

                  Scenario: Adds
                    Given a calculator
                    When add is called with "1,2"
                    Then the result is 3
                """);
        Files.writeString(root.resolve("pom.xml"), "<project/>");
    }
}
