package com.davidparry.workshop.mcp.client;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** One tool the server advertised during discovery. */
public record DiscoveredTool(String name, String description, Map<String, Object> inputSchema) {

    public DiscoveredTool {
        name = name == null ? "" : name;
        description = description == null ? "" : description;
        inputSchema = inputSchema == null ? Map.of() : Map.copyOf(inputSchema);
    }

    public DiscoveredTool(String name, String description) {
        this(name, description, Map.of());
    }

    /** Required argument names from the JSON Schema {@code required} array. */
    public List<String> requiredArguments() {
        Object required = inputSchema.get("required");
        if (!(required instanceof List<?> list)) {
            return List.of();
        }
        List<String> names = new ArrayList<>();
        for (Object item : list) {
            if (item instanceof String value) {
                names.add(value);
            }
        }
        return List.copyOf(names);
    }
}
