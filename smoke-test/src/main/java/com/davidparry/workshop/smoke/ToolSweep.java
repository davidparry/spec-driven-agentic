package com.davidparry.workshop.smoke;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;
import java.util.stream.Collectors;

/**
 * Lists the server's tools, diffs them against {@link ToolPlan}, and
 * calls every row the current mode allows. Default mode is read-only;
 * {@code includeMutating} opts into staging and gated tools.
 */
public final class ToolSweep {

    public record SweepReport(
            List<String> discovered,
            List<String> called,
            List<String> unexpected,
            List<String> missing,
            List<String> failures) {
        public SweepReport {
            discovered = List.copyOf(discovered);
            called = List.copyOf(called);
            unexpected = List.copyOf(unexpected);
            missing = List.copyOf(missing);
            failures = List.copyOf(failures);
        }
    }

    public SweepReport run(McpToolClient client, Narrator narrator, boolean includeMutating) {
        List<DiscoveredTool> tools = client.listTools();
        List<String> discovered = tools.stream().map(DiscoveredTool::name).toList();
        Set<String> discoveredSet = new LinkedHashSet<>(discovered);
        Set<String> planned = ToolPlan.names();

        List<String> unexpected = discoveredSet.stream()
                .filter(name -> !planned.contains(name))
                .toList();
        List<String> missing = planned.stream()
                .filter(name -> !discoveredSet.contains(name))
                .toList();

        if (narrator != null) {
            narrator.say("Discovered " + discovered.size() + " tool(s); plan has " + ToolPlan.size() + ".");
            if (!unexpected.isEmpty()) {
                narrator.say("Unexpected: " + String.join(", ", unexpected));
            }
            if (!missing.isEmpty()) {
                narrator.say("Missing: " + String.join(", ", missing));
            }
        }

        List<String> called = new ArrayList<>();
        List<String> failures = new ArrayList<>();
        Set<String> present = discoveredSet.stream()
                .filter(planned::contains)
                .collect(Collectors.toCollection(LinkedHashSet::new));
        for (ToolPlan.PlannedTool row : ToolPlan.all()) {
            if (!present.contains(row.name())) {
                continue;
            }
            if (row.kind() != ToolPlan.Kind.READ && !includeMutating) {
                continue;
            }
            if (narrator != null) {
                narrator.say("Calling " + row.name());
            }
            try {
                ToolResponse response = client.callTool(row.name(), row.arguments());
                called.add(row.name());
                if (response.error() && row.kind() != ToolPlan.Kind.GATED) {
                    failures.add(row.name() + ": " + response.text());
                }
            } catch (RuntimeException error) {
                failures.add(row.name() + ": " + error.getMessage());
            }
        }
        return new SweepReport(discovered, called, unexpected, missing, failures);
    }
}
