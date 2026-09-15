package com.davidparry.workshop.smoke;

import io.cucumber.java.en.Given;
import io.cucumber.java.en.Then;
import io.cucumber.java.en.When;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.StringJoiner;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

/**
 * Step definitions for {@code tdd_workshop_agent.feature}. The walkthrough
 * runs against a scripted {@link McpToolClient}, and every assertion reads
 * the captured narration — the same output an attendee sees on the terminal.
 */
public class TddWorkshopAgentSteps {

    private final ScriptedToolClient server = new ScriptedToolClient();
    private final StringJoiner captured = new StringJoiner("\n");

    @Given("the server exposes a tool {string} described as {string}")
    public void serverExposesTool(String name, String description) {
        server.tools.add(new DiscoveredTool(name, description));
    }

    @Given("the tool {string} answers with phase {string}")
    public void toolAnswersWithPhase(String name, String phase) {
        server.responses.put(name, new ToolResponse("{\"phase\":\"" + phase + "\"}", false));
    }

    @Given("the backlog lists {string} as {string} and {string} as {string}")
    public void backlogLists(String firstId, String firstStatus, String secondId, String secondStatus) {
        server.responses.put("list_requirements", new ToolResponse("""
                {"requirements":[
                  {"id":"%s","status":"%s"},
                  {"id":"%s","status":"%s"}
                ]}""".formatted(firstId, firstStatus, secondId, secondStatus), false));
    }

    @Given("the tool {string} answers with an error {string}")
    public void toolAnswersWithError(String name, String message) {
        server.responses.put(name, new ToolResponse(message, true));
    }

    @Given("the tool {string} fails catastrophically")
    public void toolFailsCatastrophically(String name) {
        server.failures.put(name, new IllegalStateException("transport blew up"));
    }

    @When("the walkthrough runs")
    public void walkthroughRuns() {
        workflow().run();
    }

    @When("the walkthrough runs and fails")
    public void walkthroughRunsAndFails() {
        assertThatThrownBy(() -> workflow().run()).isInstanceOf(IllegalStateException.class);
    }

    @Then("the narration contains {string}")
    public void narrationContains(String expected) {
        assertThat(narration()).contains(expected);
    }

    @Then("the narration does not contain {string}")
    public void narrationDoesNotContain(String unexpected) {
        assertThat(narration()).doesNotContain(unexpected);
    }

    @Then("the server connection has been closed")
    public void connectionClosed() {
        assertThat(server.closed).isTrue();
    }

    private AgentWorkflow workflow() {
        return new AgentWorkflow(server, new Narrator(captured::add));
    }

    private String narration() {
        return captured.toString();
    }

    /** A scripted server: canned tools and per-tool responses. */
    private static final class ScriptedToolClient implements McpToolClient {

        private final List<DiscoveredTool> tools = new ArrayList<>();
        private final Map<String, ToolResponse> responses = new HashMap<>();
        private final Map<String, RuntimeException> failures = new HashMap<>();
        private boolean closed;

        @Override
        public List<DiscoveredTool> listTools() {
            return List.copyOf(tools);
        }

        @Override
        public ToolResponse callTool(String name, Map<String, Object> arguments) {
            RuntimeException failure = failures.get(name);
            if (failure != null) {
                throw failure;
            }
            return responses.getOrDefault(name, new ToolResponse("{}", false));
        }

        @Override
        public void close() {
            closed = true;
        }
    }
}
