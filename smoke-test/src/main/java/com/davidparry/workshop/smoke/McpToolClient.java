package com.davidparry.workshop.smoke;

import java.util.List;
import java.util.Map;

/**
 * The agent's port to an MCP server: discovery, invocation, shutdown.
 * {@link AgentWorkflow} depends only on this interface, so the whole
 * walkthrough is testable with a scripted fake; the SDK adapter
 * ({@link SdkToolClient}) is wired in by the composition root.
 */
public interface McpToolClient extends AutoCloseable {

    /** Lists the tools the server exposes ({@code tools/list}). */
    List<DiscoveredTool> listTools();

    /** Calls one tool ({@code tools/call}) and returns its text result. */
    ToolResponse callTool(String name, Map<String, Object> arguments);

    /** Closes the connection to the server. */
    @Override
    void close();
}
