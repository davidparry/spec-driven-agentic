package com.davidparry.workshop.smoke;

import io.modelcontextprotocol.spec.McpSchema;
import io.modelcontextprotocol.spec.McpSchema.CallToolRequest;
import io.modelcontextprotocol.spec.McpSchema.CallToolResult;
import io.modelcontextprotocol.spec.McpSchema.ListToolsResult;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class SdkMappersTest {

    @Test
    @DisplayName("the discovery result maps to name/description pairs")
    void mapsTools() {
        ListToolsResult result = new ListToolsResult(List.of(
                McpSchema.Tool.builder("run_tests", Map.of("type", "object"))
                        .description("Run the kata test suite.")
                        .build()), null);
        assertThat(SdkMappers.toTools(result))
                .containsExactly(new DiscoveredTool(
                        "run_tests",
                        "Run the kata test suite.",
                        Map.of("type", "object")));
    }

    @Test
    @DisplayName("a tool call maps to a CallToolRequest with its arguments")
    void mapsRequest() {
        CallToolRequest request = SdkMappers.toRequest("get_requirement", Map.of("id", "REQ-003"));
        assertThat(request.name()).isEqualTo("get_requirement");
        assertThat(request.arguments()).containsEntry("id", "REQ-003");
    }

    @Test
    @DisplayName("text content is concatenated and non-text content is ignored")
    void mapsResponseText() {
        CallToolResult result = CallToolResult.builder()
                .content(List.of(
                        new McpSchema.TextContent("{\"phase\":"),
                        new McpSchema.ImageContent(null, "aGk=", "image/png"),
                        new McpSchema.TextContent("\"GREEN\"}")))
                .isError(false)
                .build();
        ToolResponse response = SdkMappers.toResponse(result);
        assertThat(response.text()).isEqualTo("{\"phase\":\"GREEN\"}");
        assertThat(response.error()).isFalse();
    }

    @Test
    @DisplayName("an error result maps to an error response")
    void mapsErrorResponse() {
        CallToolResult result = CallToolResult.builder()
                .content(List.of(new McpSchema.TextContent("No requirement with id 'REQ-404'.")))
                .isError(true)
                .build();
        ToolResponse response = SdkMappers.toResponse(result);
        assertThat(response.text()).contains("REQ-404");
        assertThat(response.error()).isTrue();
    }
}
