package com.davidparry.workshop.smoke;

import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.stream.Collectors;

/**
 * The complete catalog the Java smoke test is willing to call. A 26th tool
 * on the server fails the build until it is planned here.
 */
public final class ToolPlan {

    public enum Kind {
        READ,
        STAGES,
        GATED
    }

    public record PlannedTool(String name, Kind kind, Map<String, Object> arguments) {
        public PlannedTool {
            arguments = arguments == null ? Map.of() : Map.copyOf(arguments);
        }
    }

    private static final List<PlannedTool> TOOLS = List.of(
            read("list_requirements"),
            read("get_requirement", Map.of("id", "REQ-001")),
            read("validate_spec"),
            read("refine_requirement", Map.of("id", "REQ-001")),
            read("get_tdd_state"),
            stages("run_tests", Map.of()),
            gated("start_refactor", Map.of()),
            read("project_root"),
            read("project_inspect"),
            read("feature_list"),
            read("feature_read", Map.of("path", "features/calc.feature")),
            stages("feature_create", Map.of(
                    "path", "features/extra.feature",
                    "name", "Extra")),
            stages("scenario_add", Map.of(
                    "feature", "features/calc.feature",
                    "req", "REQ-001",
                    "name", "Adds from the sweep",
                    "steps", List.of(
                            "Given a calculator",
                            "When I add 1 and 2",
                            "Then the result is 3"))),
            stages("scenario_update", Map.of(
                    "feature", "features/calc.feature",
                    "name", "Adds from the sweep",
                    "req", "REQ-001")),
            stages("scenario_delete", Map.of(
                    "feature", "features/calc.feature",
                    "name", "Adds from the sweep")),
            read("changes_show"),
            read("changes_validate"),
            gated("changes_commit", Map.of()),
            stages("changes_discard", Map.of()),
            gated("command_run", Map.of("command", List.of("true"))),
            stages("requirement_reword", Map.of(
                    "id", "REQ-001",
                    "title", "Adds two numbers from the sweep")),
            gated("requirement_mark_implemented", Map.of("id", "REQ-001")),
            read("step_definitions_find"),
            stages("step_definition_create", Map.of()),
            stages("unit_test_create", Map.of("req_id", "REQ-001")));

    private ToolPlan() {
    }

    public static List<PlannedTool> all() {
        return TOOLS;
    }

    public static Set<String> names() {
        return TOOLS.stream().map(PlannedTool::name).collect(Collectors.toUnmodifiableSet());
    }

    public static int size() {
        return TOOLS.size();
    }

    private static PlannedTool read(String name) {
        return new PlannedTool(name, Kind.READ, Map.of());
    }

    private static PlannedTool read(String name, Map<String, Object> arguments) {
        return new PlannedTool(name, Kind.READ, arguments);
    }

    private static PlannedTool stages(String name, Map<String, Object> arguments) {
        return new PlannedTool(name, Kind.STAGES, arguments);
    }

    private static PlannedTool gated(String name, Map<String, Object> arguments) {
        return new PlannedTool(name, Kind.GATED, arguments);
    }
}
