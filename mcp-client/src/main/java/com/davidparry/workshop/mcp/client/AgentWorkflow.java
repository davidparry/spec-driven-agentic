package com.davidparry.workshop.mcp.client;

import tools.jackson.databind.JsonNode;
import tools.jackson.databind.ObjectMapper;

import java.util.Map;

/**
 * The narrated walkthrough of one agentic spec-to-green pass: handshake,
 * discovery, workflow state, requirement selection, baseline test run, and
 * the handoff to a real LLM agent. Talks to the server only through the
 * {@link McpToolClient} port, so the whole walkthrough runs against a
 * scripted fake in tests.
 */
public class AgentWorkflow {

    private final ObjectMapper json = new ObjectMapper();
    private final McpToolClient client;
    private final Narrator out;

    public AgentWorkflow(McpToolClient client, Narrator out) {
        this.client = client;
        this.out = out;
    }

    /** Runs the narrated steps and always closes the connection. */
    public void run() {
        try {
            String nextId = walkThrough();
            handOff(nextId);
        } finally {
            client.close();
        }
    }

    private String walkThrough() {
        out.banner("STEP 1 — initialize (the MCP handshake)");
        out.say("JSON-RPC request: method=\"initialize\" — protocol version + capability negotiation.");
        ServerIdentity identity = client.initialize();
        out.say("Server identified itself as: " + identity.name() + " v" + identity.version());
        if (identity.instructions() != null) {
            out.say("Server instructions for the agent:\n" + Narrator.indent(identity.instructions()));
        }

        out.banner("STEP 2 — tools/list (discovery)");
        out.say("JSON-RPC request: method=\"tools/list\" — the agent asks what it is allowed to do.");
        for (DiscoveredTool tool : client.listTools()) {
            out.say("- " + tool.name() + ": " + firstSentence(tool.description()));
        }

        out.banner("STEP 3 — tools/call get_tdd_state (where are we?)");
        callTool("get_tdd_state", Map.of());

        out.banner("STEP 4 — tools/call list_requirements (what should we build?)");
        String requirements = callTool("list_requirements", Map.of());
        String nextId = firstPendingRequirement(requirements);

        out.banner("STEP 5 — tools/call run_tests (establish the baseline)");
        out.say("This runs `mvn -f kata/pom.xml test` inside the server and parses the Surefire reports.");
        String runResult = callTool("run_tests", Map.of());
        out.say(">>> The bar is " + field(runResult, "phase") + ".");

        if (nextId != null) {
            out.banner("STEP 6 — tools/call get_requirement (" + nextId + ")");
            out.say("An agent would now iterate the spec with validate_spec and refine_requirement,");
            out.say("then turn these acceptance criteria into an executable Gherkin scenario (BDD)");
            out.say("plus failing unit tests (TDD) — the spec (SDD) stays the source of truth.");
            callTool("get_requirement", Map.of("id", nextId));
        }
        return nextId;
    }

    private void handOff(String nextId) {
        out.banner("What happens next (your turn)");
        out.say("""
                1. Using the tdd-workflow tools, add a Gherkin scenario for {req}
                   with scenario_add (tag @{tag}), add missing steps with
                   step_definition_create, and a unit test with unit_test_create.
                2. Review the staged files with changes_show, then changes_commit.
                3. Call run_tests            -> expect RED
                4. Implement the behavior in StringCalculator.add
                5. Call run_tests            -> expect GREEN
                6. Call start_refactor if you agree, then requirement_mark_implemented
                7. Repeat for the next pending requirement.
                """
                .replace("{req}", nextId == null ? "the next requirement" : nextId)
                .replace("{tag}", nextId == null ? "REQ-XXX" : nextId));
        out.say("Connect this same server to Cursor or Claude Desktop using .cursor/mcp.json "
                + "and drive the loop with a real LLM agent.");
    }

    private String callTool(String name, Map<String, Object> arguments) {
        out.say("JSON-RPC request: method=\"tools/call\" params={\"name\":\"" + name + "\",\"arguments\":"
                + json.writeValueAsString(arguments) + "}");
        ToolResponse response = client.callTool(name, arguments);
        out.say("Result" + (response.error() ? " (isError=true)" : "") + ":\n"
                + Narrator.indent(response.text()));
        return response.text();
    }

    private String field(String text, String name) {
        JsonNode node = json.readTree(text).path(name);
        return node.isMissingNode() ? null : node.asString();
    }

    private String firstPendingRequirement(String text) {
        for (JsonNode requirement : json.readTree(text).path("requirements")) {
            if ("pending".equalsIgnoreCase(requirement.path("status").asString())) {
                return requirement.path("id").asString();
            }
        }
        return null;
    }

    static String firstSentence(String text) {
        if (text == null) {
            return "";
        }
        int dot = text.indexOf(". ");
        return dot > 0 ? text.substring(0, dot + 1) : text;
    }
}
