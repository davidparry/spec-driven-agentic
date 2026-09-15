package com.davidparry.workshop.mcp.client;

import io.modelcontextprotocol.spec.McpSchema;
import io.modelcontextprotocol.spec.McpSchema.CallToolRequest;
import io.modelcontextprotocol.spec.McpSchema.CallToolResult;
import io.modelcontextprotocol.spec.McpSchema.InitializeResult;
import io.modelcontextprotocol.spec.McpSchema.ListToolsResult;

import java.util.List;
import java.util.Map;

/**
 * Pure mappings between MCP SDK schema types and the agent's own small
 * records. All the logic of the SDK adapter lives here, fully unit-tested;
 * {@link SdkToolClient} is only the delegation glue around these functions.
 */
public final class SdkMappers {

    private SdkMappers() {
    }

    public static ServerIdentity toIdentity(InitializeResult result) {
        return new ServerIdentity(
                result.serverInfo().name(),
                result.serverInfo().version(),
                result.instructions());
    }

    public static List<DiscoveredTool> toTools(ListToolsResult result) {
        return result.tools().stream()
                .map(tool -> new DiscoveredTool(tool.name(), tool.description(), tool.inputSchema()))
                .toList();
    }

    public static CallToolRequest toRequest(String name, Map<String, Object> arguments) {
        return CallToolRequest.builder(name).arguments(arguments).build();
    }

    public static ToolResponse toResponse(CallToolResult result) {
        StringBuilder text = new StringBuilder();
        for (McpSchema.Content content : result.content()) {
            if (content instanceof McpSchema.TextContent textContent) {
                text.append(textContent.text());
            }
        }
        return new ToolResponse(text.toString(), Boolean.TRUE.equals(result.isError()));
    }
}
