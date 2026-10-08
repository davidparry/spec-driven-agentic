package com.davidparry.workshop.smoke;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIfSystemProperty;
import org.junit.jupiter.api.io.TempDir;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;

import static org.assertj.core.api.Assertions.assertThat;

@EnabledIfSystemProperty(named = "spec.binary", matches = ".+")
class LiveSpecServerTest {

    @TempDir
    Path project;

    @Test
    @DisplayName("the live spec binary serves exactly the planned tools")
    void liveSweep() throws IOException {
        seedProject(project);
        Path spec = Path.of(System.getProperty("spec.binary"));
        try (SdkToolClient client = new SdkToolClient(project, spec)) {
            ToolSweep.SweepReport report = new ToolSweep().run(client, new Narrator(line -> {
            }), false);
            assertThat(report.discovered()).hasSize(22);
            assertThat(report.missing()).isEmpty();
            assertThat(report.unexpected()).isEmpty();
            assertThat(report.failures()).isEmpty();
            assertThat(report.called()).isNotEmpty();
        }
    }

    static void seedProject(Path root) throws IOException {
        Files.createDirectories(root.resolve("requirements"));
        Files.createDirectories(root.resolve("features"));
        Files.writeString(root.resolve("requirements/requirements.json"), spec("Adds two numbers"));
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

        // A workshop project is a git checkout, and git_diff reads what
        // is uncommitted in one. Without a repository here the sweep
        // would only ever see the tool's "not a repository" refusal, and
        // the reword leaves it something real to report.
        commitEverything(root);
        Files.writeString(root.resolve("requirements/requirements.json"), spec("Adds two integers"));
    }

    private static String spec(String title) {
        return """
                {
                  "project": "Sweep",
                  "requirements": [
                    {
                      "id": "REQ-001",
                      "title": "%s",
                      "status": "pending",
                      "story": "As a user, I want sums so that I can add.",
                      "acceptanceCriteria": [
                        "Given the input \\"1,2\\", when add is called, then the result is 3"
                      ],
                      "featureFile": "features/calc.feature"
                    }
                  ]
                }
                """.formatted(title);
    }

    private static void commitEverything(Path root) throws IOException {
        String[][] commands = {
            {"git", "init", "--initial-branch=main"},
            {"git", "config", "user.email", "smoke@example.com"},
            {"git", "config", "user.name", "Smoke"},
            {"git", "add", "."},
            {"git", "commit", "-m", "seed"},
        };
        for (String[] command : commands) {
            try {
                Process git = new ProcessBuilder(command)
                        .directory(root.toFile())
                        .redirectOutput(ProcessBuilder.Redirect.DISCARD)
                        .redirectError(ProcessBuilder.Redirect.DISCARD)
                        .start();
                if (git.waitFor() != 0) {
                    throw new IOException("`" + String.join(" ", command) + "` failed in " + root);
                }
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                throw new IOException("interrupted running " + String.join(" ", command), interrupted);
            }
        }
    }
}
