package com.davidparry.workshop.smoke;

import java.nio.file.Path;
import java.util.Arrays;

/**
 * The workshop's smoke test of {@code bdd mcp serve}: launches the server
 * as a child process (stdio transport), discovers its tools, and walks a
 * smoke pass of the agentic spec-to-green loop — narrating discovery and
 * tool calls so you can see what an IDE or LLM host does under the hood.
 * The walkthrough does not call {@code initialize}; STEP 1 is
 * {@code tools/list}. Default smoke is read-only plus baseline
 * {@code run_tests}; mutating tools stay behind
 * {@code --sweep --include-mutating}.
 *
 * <p>This class is the composition root and nothing else. All walkthrough
 * logic lives in {@link AgentWorkflow}; the 23-tool sweep lives in
 * {@link ToolSweep}. Both are covered at 100% against a scripted fake.
 *
 * <p>Run it from the repo root after {@code bdd} is installed and on
 * {@code PATH} ({@code bdd --version} succeeds in the same shell):
 * <pre>{@code java -jar smoke-test/target/smoke-test.jar}</pre>
 */
public final class TddAgent {

    private TddAgent() {
    }

    public static void main(String[] args) {
        Path root = resolveWorkshopRoot();
        Path bdd = BddBinary.onPath();
        if (bdd == null) {
            System.out.println("bdd was not found on PATH.");
            System.out.println("Install it: cargo install --path cli");
            System.out.println("Or add the directory that contains bdd to PATH.");
            System.exit(1);
        }

        Narrator narrator = new Narrator(System.out::println);
        narrator.banner("STEP 0 — Launch the server");
        narrator.say("The client starts bdd mcp serve as a child process and talks JSON-RPC 2.0 over stdin/stdout.");
        narrator.say("Command: " + bdd + " mcp serve --root " + root);

        try (SdkToolClient client = new SdkToolClient(root, bdd)) {
            if (isSweep(args)) {
                boolean mutating = Arrays.asList(args).contains("--include-mutating")
                        || "true".equalsIgnoreCase(System.getProperty("workshop.mutating", "false"));
                ToolSweep.SweepReport report = new ToolSweep().run(client, narrator, mutating);
                narrator.say("Sweep called " + report.called().size() + " tool(s).");
                if (!report.failures().isEmpty() || !report.missing().isEmpty() || !report.unexpected().isEmpty()) {
                    System.exit(1);
                }
            } else {
                new AgentWorkflow(client, narrator).run();
            }
        }
    }

    private static boolean isSweep(String[] args) {
        if (Arrays.asList(args).contains("--sweep")) {
            return true;
        }
        return "sweep".equalsIgnoreCase(System.getProperty("workshop.mode", "walkthrough"));
    }

    static Path resolveWorkshopRoot() {
        String configured = System.getProperty("workshop.root", System.getenv("WORKSHOP_ROOT"));
        Path root = configured != null
                ? Path.of(configured)
                : Path.of("").toAbsolutePath();
        return root.toAbsolutePath().normalize();
    }
}
