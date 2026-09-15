package com.davidparry.workshop.smoke;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class DiscoveredToolTest {

    @Test
    @DisplayName("a missing schema has no required arguments")
    void missingSchema() {
        assertThat(new DiscoveredTool("list_requirements", "list").requiredArguments()).isEmpty();
        assertThat(new DiscoveredTool("x", "y", null).inputSchema()).isEmpty();
        assertThat(new DiscoveredTool(null, null, Map.of()).name()).isEmpty();
        assertThat(new DiscoveredTool(null, null, Map.of()).description()).isEmpty();
    }

    @Test
    @DisplayName("required names come from the JSON Schema required array")
    void requiredFromSchema() {
        DiscoveredTool tool = new DiscoveredTool(
                "get_requirement",
                "one",
                Map.of("type", "object", "required", List.of("id", 3, "path")));
        assertThat(tool.requiredArguments()).containsExactly("id", "path");
    }

    @Test
    @DisplayName("a required field that is not a list is ignored")
    void requiredNotAList() {
        DiscoveredTool tool = new DiscoveredTool(
                "x",
                "y",
                Map.of("required", "id"));
        assertThat(tool.requiredArguments()).isEmpty();
    }
}
