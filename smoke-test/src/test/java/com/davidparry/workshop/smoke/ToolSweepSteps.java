package com.davidparry.workshop.smoke;

import io.cucumber.java.en.Given;
import io.cucumber.java.en.Then;
import io.cucumber.java.en.When;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

public class ToolSweepSteps {

    private ServerLaunch launch;
    private DiscoveredTool discovered;
    private final Scripted server = new Scripted();
    private ToolSweep.SweepReport report;

    @When("a bdd launch is built for root {string} and binary {string}")
    public void bddLaunchIsBuilt(String root, String binary) {
        launch = ServerLaunch.bdd(Path.of(root), Path.of(binary));
    }

    @Then("the launch program is {string}")
    public void launchProgramIs(String program) {
        assertThat(launch.program()).isEqualTo(program);
    }

    @Then("the launch args are {string}, {string}, {string}, {string}")
    public void launchArgsAre(String a, String b, String c, String d) {
        assertThat(launch.args()).containsExactly(a, b, c, d);
    }

    @Given("a discovered tool {string} whose schema requires {string}")
    public void discoveredToolRequires(String name, String required) {
        discovered = new DiscoveredTool(
                name,
                name,
                Map.of("type", "object", "required", List.of(required)));
    }

    @Then("the required arguments are {string}")
    public void requiredArgumentsAre(String required) {
        assertThat(discovered.requiredArguments()).containsExactly(required);
    }

    @Then("the tool plan names exactly {int} tools")
    public void toolPlanSize(int count) {
        assertThat(ToolPlan.size()).isEqualTo(count);
    }

    @Then("the tool plan includes {string}")
    public void toolPlanIncludes(String name) {
        assertThat(ToolPlan.names()).contains(name);
    }

    @Given("a server that exposes every planned tool")
    public void serverExposesEveryPlannedTool() {
        ToolPlan.all().forEach(row -> server.tools.add(new DiscoveredTool(row.name(), row.name())));
    }

    @Given("a server that exposes the planned tools plus {string}")
    public void serverExposesPlannedPlus(String extra) {
        serverExposesEveryPlannedTool();
        server.tools.add(new DiscoveredTool(extra, extra));
    }

    @Given("the gated tool {string} answers with an error {string}")
    public void gatedToolAnswersWithError(String name, String message) {
        server.errors.put(name, message);
    }

    @When("a read-only sweep runs")
    public void readOnlySweepRuns() {
        report = new ToolSweep().run(server, new Narrator(line -> {
        }), false);
    }

    @When("a mutating sweep runs")
    public void mutatingSweepRuns() {
        report = new ToolSweep().run(server, new Narrator(line -> {
        }), true);
    }

    @Then("the sweep reports {string} as unexpected")
    public void sweepReportsUnexpected(String name) {
        assertThat(report.unexpected()).contains(name);
    }

    @Then("the sweep called {string}")
    public void sweepCalled(String name) {
        assertThat(report.called()).contains(name);
    }

    @Then("the sweep did not call {string}")
    public void sweepDidNotCall(String name) {
        assertThat(report.called()).doesNotContain(name);
    }

    @Then("the sweep has no failures")
    public void sweepHasNoFailures() {
        assertThat(report.failures()).isEmpty();
    }

    private static final class Scripted implements McpToolClient {
        private final List<DiscoveredTool> tools = new ArrayList<>();
        private final Map<String, String> errors = new HashMap<>();

        @Override
        public List<DiscoveredTool> listTools() {
            return List.copyOf(tools);
        }

        @Override
        public ToolResponse callTool(String name, Map<String, Object> arguments) {
            String error = errors.get(name);
            if (error != null) {
                return new ToolResponse(error, true);
            }
            return new ToolResponse("{}", false);
        }

        @Override
        public void close() {
        }
    }
}
