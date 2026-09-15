package com.davidparry.workshop.smoke;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class ToolPlanTest {

    @Test
    @DisplayName("the plan names exactly 24 tools and no extras")
    void planIsExactlyTwentyFour() {
        assertThat(ToolPlan.size()).isEqualTo(24);
        assertThat(ToolPlan.names()).hasSize(24);
        assertThat(ToolPlan.all()).extracting(ToolPlan.PlannedTool::name).doesNotHaveDuplicates();
        assertThat(ToolPlan.names()).contains(
                "list_requirements",
                "unit_test_create",
                "command_run",
                "requirement_mark_implemented",
                "step_definition_create");
    }

    @Test
    @DisplayName("null arguments on a planned row become an empty map")
    void nullArgumentsBecomeEmpty() {
        ToolPlan.PlannedTool row = new ToolPlan.PlannedTool("x", ToolPlan.Kind.READ, null);
        assertThat(row.arguments()).isEmpty();
    }
}

class ToolSweepTest {

    @Test
    @DisplayName("read-only mode calls every READ tool that was discovered")
    void readOnlyCallsReads() {
        Scripted client = new Scripted();
        ToolPlan.all().forEach(row -> client.tools.add(new DiscoveredTool(row.name(), row.name())));
        ToolSweep.SweepReport report = new ToolSweep().run(client, null, false);
        assertThat(report.missing()).isEmpty();
        assertThat(report.unexpected()).isEmpty();
        assertThat(report.discovered()).hasSize(24);
        assertThat(report.called()).contains("list_requirements", "validate_spec", "changes_show");
        assertThat(report.called()).doesNotContain("scenario_add", "command_run", "changes_commit");
        assertThat(report.failures()).isEmpty();
    }

    @Test
    @DisplayName("mutating mode also calls staging and gated tools")
    void mutatingCallsTheRest() {
        Scripted client = new Scripted();
        ToolPlan.all().forEach(row -> client.tools.add(new DiscoveredTool(row.name(), row.name())));
        client.errors.put("start_refactor", "not GREEN");
        Narrator narrator = new Narrator(line -> {
        });
        ToolSweep.SweepReport report = new ToolSweep().run(client, narrator, true);
        assertThat(report.called()).hasSize(24);
        assertThat(report.failures()).isEmpty();
    }

    @Test
    @DisplayName("an extra discovered tool is unexpected and a missing planned tool is reported")
    void unexpectedAndMissing() {
        Scripted client = new Scripted();
        client.tools.add(new DiscoveredTool("list_requirements", "list"));
        client.tools.add(new DiscoveredTool("bonus_tool", "nope"));
        Narrator narrator = new Narrator(line -> {
        });
        ToolSweep.SweepReport report = new ToolSweep().run(client, narrator, false);
        assertThat(report.unexpected()).containsExactly("bonus_tool");
        assertThat(report.missing()).contains("get_requirement");
        assertThat(report.called()).containsExactly("list_requirements");
    }

    @Test
    @DisplayName("a non-gated tool error is a failure; a transport blow-up is too")
    void failuresAreCollected() {
        Scripted client = new Scripted();
        client.tools.add(new DiscoveredTool("list_requirements", "list"));
        client.tools.add(new DiscoveredTool("validate_spec", "v"));
        client.errors.put("list_requirements", "disk on fire");
        client.blow.put("validate_spec", new IllegalStateException("transport"));
        ToolSweep.SweepReport report = new ToolSweep().run(client, null, false);
        assertThat(report.failures()).hasSize(2);
        assertThat(report.failures().get(0)).contains("list_requirements");
        assertThat(report.failures().get(1)).contains("validate_spec");
    }

    private static final class Scripted implements McpToolClient {
        private final List<DiscoveredTool> tools = new ArrayList<>();
        private final Map<String, String> errors = new HashMap<>();
        private final Map<String, RuntimeException> blow = new HashMap<>();

        @Override
        public List<DiscoveredTool> listTools() {
            return List.copyOf(tools);
        }

        @Override
        public ToolResponse callTool(String name, Map<String, Object> arguments) {
            RuntimeException failure = blow.get(name);
            if (failure != null) {
                throw failure;
            }
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
