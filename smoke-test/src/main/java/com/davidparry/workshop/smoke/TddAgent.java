package com.davidparry.workshop.smoke;

import java.nio.file.Files;
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
 * <p>Run it from the repo root after the {@code bdd} binary is on PATH
 * (or {@code -Dbdd.binary=…}):
 * <pre>{@code java -jar smoke-test/target/smoke-test.jar}</pre>
 */
public final class TddAgent {

    private TddAgent() {
    }

    public static void main(String[] args) {
        Path root = resolveWorkshopRoot();
        Path bdd = resolveBddBinary(root);
        if (bdd == null || !Files.isRegularFile(bdd)) {
            System.out.println("bdd binary not found.");
            System.out.println("Install it: cargo install --path cli");
            System.out.println("Or pass -Dbdd.binary=/path/to/bdd");
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

    static Path resolveBddBinary(Path root) {
        String configured = System.getProperty("bdd.binary");
        if (configured != null && !configured.isBlank()) {
            return Path.of(configured);
        }
        Path release = root.resolve("cli/target/release/bdd");
        if (Files.isRegularFile(release)) {
            return release;
        }
        Path debug = root.resolve("cli/target/debug/bdd");
        if (Files.isRegularFile(debug)) {
            return debug;
        }
        return null;
    }
}
