package com.davidparry.workshop.mcp.server.tdd;

import com.davidparry.workshop.mcp.server.McpServerFactory;
import com.davidparry.workshop.mcp.server.WorkflowToolHandlers;
import com.davidparry.workshop.mcp.server.requirements.RequirementRefiner;
import com.davidparry.workshop.mcp.server.requirements.RequirementsRepository;
import com.davidparry.workshop.mcp.server.requirements.SpecValidator;

import io.cucumber.java.After;
import io.cucumber.java.Before;
import io.cucumber.java.en.Given;
import io.cucumber.java.en.Then;
import io.cucumber.java.en.When;

import io.modelcontextprotocol.server.McpServerFeatures.SyncToolSpecification;
import io.modelcontextprotocol.server.McpSyncServer;
import io.modelcontextprotocol.spec.McpSchema;
import io.modelcontextprotocol.spec.McpSchema.CallToolRequest;
import io.modelcontextprotocol.spec.McpSchema.CallToolResult;
import io.modelcontextprotocol.spec.McpServerSession;
import io.modelcontextprotocol.spec.McpServerTransportProvider;

import reactor.core.publisher.Mono;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicReference;
import java.util.stream.Stream;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * Step definitions for {@code tdd_workflow_server.feature}. Tools are
 * invoked through the same {@link SyncToolSpecification} wiring an MCP
 * client calls, with the kata suite replaced by a swappable stub (and, for
 * the build-failure scenario, a real {@link MavenTestRunner} on a fake
 * command).
 */
public class TddWorkflowServerSteps {

    private Path dir;
    private TddStateMachine tdd;
    private AtomicReference<TestRunner> suite;
    private WorkflowToolHandlers handlers;
    private List<SyncToolSpecification> specs;
    private CallToolResult last;
    private McpSyncServer server;

    private static final class FakeTransportProvider implements McpServerTransportProvider {
        @Override
        public void setSessionFactory(McpServerSession.Factory sessionFactory) {
            // no transport in a behavior test
        }

        @Override
        public Mono<Void> notifyClients(String method, Object params) {
            return Mono.empty();
        }

        @Override
        public Mono<Void> closeGracefully() {
            return Mono.empty();
        }
    }

    @Before
    public void createWorkspace() throws IOException {
        dir = Files.createTempDirectory("mcp-server-bdd");
    }

    @After
    public void cleanUp() throws IOException {
        if (server != null) {
            server.close();
        }
        try (Stream<Path> files = Files.walk(dir)) {
            files.sorted(Comparator.reverseOrder()).forEach(p -> p.toFile().delete());
        }
    }

    @Given("a workflow server backed by a spec with an implemented {string} and a pending {string}")
    public void aWorkflowServer(String implementedId, String pendingId) throws IOException {
        Path file = specFile();
        Files.writeString(file, """
                {
                  "project": "Server Spec Kata",
                  "requirements": [
                    {
                      "id": "%s",
                      "title": "First",
                      "status": "implemented",
                      "story": "As a user...",
                      "acceptanceCriteria": ["Given x, then y"]
                    },
                    {
                      "id": "%s",
                      "title": "Second",
                      "status": "pending",
                      "story": "As a user...",
                      "acceptanceCriteria": ["Given a, then b"],
                      "featureFile": "kata/src/test/resources/features/string_calculator.feature"
                    }
                  ]
                }
                """.formatted(implementedId, pendingId));
        tdd = new TddStateMachine();
        suite = new AtomicReference<>(TestRunSummary::empty);
        handlers = new WorkflowToolHandlers(
                () -> RequirementsRepository.load(file),
                new SpecValidator(file, dir),
                new RequirementRefiner(),
                () -> suite.get().runKataTests(),
                tdd);
        specs = McpServerFactory.toolSpecifications(handlers);
    }

    @Given("the spec on disk is rewritten to be valid")
    public void specRewrittenValid() throws IOException {
        Path feature = dir.resolve("features").resolve("calc.feature");
        Files.createDirectories(feature.getParent());
        Files.writeString(feature, "@REQ-001\nScenario: done");
        Files.writeString(specFile(), """
                {
                  "project": "Server Spec Kata",
                  "requirements": [
                    {
                      "id": "REQ-001",
                      "title": "First",
                      "status": "implemented",
                      "story": "As a user...",
                      "acceptanceCriteria": ["Given x, when y, then z"],
                      "featureFile": "features/calc.feature"
                    }
                  ]
                }
                """);
    }

    @Given("the spec on disk is rewritten with polished wording on {string}")
    public void specRewrittenPolished(String id) throws IOException {
        Files.writeString(specFile(), """
                {
                  "project": "Server Spec Kata",
                  "requirements": [
                    {
                      "id": "%s",
                      "title": "Commas",
                      "status": "pending",
                      "story": "As a calculator user, I want commas to separate numbers so that lists just work.",
                      "acceptanceCriteria": [
                        "Given the input \\"1,2\\", when add is called, then the result is 3",
                        "Given an empty string \\"\\", when add is called, then the result is 0"
                      ]
                    }
                  ]
                }
                """.formatted(id));
    }

    @Given("the spec on disk is rewritten with a validation verdict on {string}")
    public void specRewrittenWithVerdict(String id) throws IOException {
        Files.writeString(specFile(), """
                {
                  "project": "Server Spec Kata",
                  "requirements": [
                    {
                      "id": "%s",
                      "title": "Phone number format",
                      "status": "pending",
                      "story": "As a user, I want my phone number input validated so that only numbers in the format XXX-XXX-XXXX or XXXXXXXXXX are accepted",
                      "acceptanceCriteria": [
                        "Given the user enters '123-456-7890', when the system validates it, then the result is 'valid'",
                        "Given the user enters '123-45-6789', when the system validates it, then the result is 'invalid'"
                      ]
                    }
                  ]
                }
                """.formatted(id));
    }

    @Given("the spec on disk is rewritten with vague wording on {string}")
    public void specRewrittenVague(String id) throws IOException {
        Files.writeString(specFile(), """
                {
                  "project": "Server Spec Kata",
                  "requirements": [
                    {
                      "id": "%s",
                      "title": "Commas",
                      "status": "pending",
                      "story": "The calculator should handle commas quickly.",
                      "acceptanceCriteria": [
                        "Given some input, when add is called, then it should work properly"
                      ]
                    }
                  ]
                }
                """.formatted(id));
    }

    @Given("the spec on disk is split into a root that includes {string} holding {string}")
    public void specSplitWithInclude(String includeFile, String childId) throws IOException {
        Files.writeString(specFile(), """
                {
                  "project": "Server Spec Kata",
                  "includes": ["%s"],
                  "requirements": [
                    {
                      "id": "REQ-001",
                      "title": "First",
                      "status": "pending",
                      "story": "As a user, I want sums so that I can add.",
                      "acceptanceCriteria": ["Given x, when y, then z"]
                    }
                  ]
                }
                """.formatted(includeFile));
        Files.writeString(dir.resolve(includeFile), """
                {
                  "requirements": [
                    {
                      "id": "%s",
                      "title": "From the included file",
                      "status": "pending",
                      "story": "As a user, I want the included backlog so that the spec scales.",
                      "acceptanceCriteria": ["Given a, when b, then c"]
                    }
                  ]
                }
                """.formatted(childId));
    }

    @Given("the spec file {string} is rewritten to include {string}")
    public void specFileRewrittenToInclude(String file, String include) throws IOException {
        Files.writeString(dir.resolve(file), """
                {
                  "includes": ["%s"],
                  "requirements": []
                }
                """.formatted(include));
    }

    @Given("the spec on disk is rewritten with a requirement missing its acceptance criteria")
    public void specRewrittenInvalid() throws IOException {
        Files.writeString(specFile(), """
                {
                  "project": "Server Spec Kata",
                  "requirements": [
                    {
                      "id": "REQ-001",
                      "title": "First",
                      "status": "pending",
                      "story": "As a user...",
                      "acceptanceCriteria": []
                    }
                  ]
                }
                """);
    }

    @Given("the kata suite passes with {int} tests")
    public void kataSuitePasses(int tests) {
        suite.set(() -> new TestRunSummary(tests, 0, 0, 0, List.of()));
    }

    @Given("the kata suite fails with {string}")
    public void kataSuiteFails(String detail) {
        suite.set(() -> new TestRunSummary(5, 1, 0, 0, List.of(detail)));
    }

    @Given("the kata build fails before tests can run")
    public void kataBuildFails() {
        suite.set(new MavenTestRunner(
                dir,
                List.of("sh", "-c", "echo 'COMPILATION ERROR'; exit 1"),
                Duration.ofSeconds(30)));
    }

    @When("the agent calls list_requirements")
    public void callsListRequirements() {
        last = call("list_requirements", Map.of());
    }

    @When("the agent calls get_requirement for {string}")
    public void callsGetRequirement(String id) {
        last = call("get_requirement", Map.of("id", id));
    }

    @When("the agent calls validate_spec")
    public void callsValidateSpec() {
        last = call("validate_spec", Map.of());
    }

    @When("the agent calls refine_requirement for {string}")
    public void callsRefineRequirement(String id) {
        last = call("refine_requirement", Map.of("id", id));
    }

    @When("the agent calls run_tests")
    public void callsRunTests() {
        last = call("run_tests", Map.of());
    }

    @When("the agent calls get_tdd_state")
    public void callsGetTddState() {
        last = call("get_tdd_state", Map.of());
    }

    @When("the agent calls start_refactor with note {string}")
    public void callsStartRefactor(String note) {
        last = call("start_refactor", Map.of("note", note));
    }

    @When("the server is assembled")
    public void serverIsAssembled() {
        server = McpServerFactory.create(new FakeTransportProvider(), handlers);
    }

    @Then("the call succeeds")
    public void callSucceeds() {
        assertThat(last.isError()).isFalse();
    }

    @Then("the call fails as a tool error")
    public void callFailsAsToolError() {
        assertThat(last.isError()).isTrue();
    }

    @Then("the result mentions {string} with status {string}")
    public void resultMentionsWithStatus(String id, String status) {
        assertThat(text()).contains(id).contains(status);
    }

    @Then("the result names the project")
    public void resultNamesProject() {
        assertThat(text()).contains("Server Spec Kata");
    }

    @Then("the result contains the acceptance criteria")
    public void resultContainsAcceptanceCriteria() {
        assertThat(text()).contains("Given a, then b");
    }

    @Then("the result contains a workflow hint naming the tag {string}")
    public void resultContainsWorkflowHint(String tag) {
        assertThat(text()).contains("workflowHint").contains(tag);
    }

    @Then("the error points the agent at {string}")
    public void errorPointsAt(String hint) {
        assertThat(text()).contains(hint);
    }

    @Then("the error names the {string} rule")
    public void errorNamesRule(String rule) {
        assertThat(text()).contains(rule);
    }

    @Then("the reported phase is {string}")
    public void reportedPhaseIs(String phase) {
        assertThat(text()).contains("\"phase\" : \"" + phase + "\"");
    }

    @Then("the result contains {string}")
    public void resultContains(String expected) {
        assertThat(text()).contains(expected);
    }

    @Then("the result contains a suggested next step")
    public void resultContainsNextStep() {
        assertThat(text()).contains("nextStep");
    }

    @Then("the spec is reported valid")
    public void specReportedValid() {
        assertThat(text()).contains("\"valid\" : true");
    }

    @Then("the spec is reported invalid")
    public void specReportedInvalid() {
        assertThat(text()).contains("\"valid\" : false");
    }

    @Then("the wording is reported clean")
    public void wordingReportedClean() {
        assertThat(text()).contains("\"clean\" : true");
    }

    @Then("the wording is reported unclean")
    public void wordingReportedUnclean() {
        assertThat(text()).contains("\"clean\" : false");
    }

    @Then("the refactor log records {string}")
    public void refactorLogRecords(String note) {
        assertThat(tdd.refactorLog()).contains(note);
    }

    @Then("it identifies as {string} version {string}")
    public void identifiesAs(String name, String version) {
        assertThat(server.getServerInfo().name()).isEqualTo(name);
        assertThat(server.getServerInfo().version()).isEqualTo(version);
    }

    @Then("it exposes exactly the tools {string}")
    public void exposesExactlyTheTools(String expected) {
        assertThat(specs.stream().map(s -> s.tool().name()).toList())
                .containsExactly(expected.split(", "));
    }

    private Path specFile() {
        return dir.resolve("requirements.json");
    }

    private CallToolResult call(String tool, Map<String, Object> args) {
        SyncToolSpecification spec = specs.stream()
                .filter(s -> s.tool().name().equals(tool))
                .findFirst()
                .orElseThrow();
        return spec.callHandler().apply(null, new CallToolRequest(tool, args));
    }

    private String text() {
        return ((McpSchema.TextContent) last.content().get(0)).text();
    }
}
