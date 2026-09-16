package com.davidparry.workshop.smoke;

import io.modelcontextprotocol.client.McpClient;
import io.modelcontextprotocol.client.McpSyncClient;
import io.modelcontextprotocol.client.transport.StdioClientTransport;
import io.modelcontextprotocol.json.McpJsonDefaults;
import io.modelcontextprotocol.spec.McpSchema;

import java.nio.file.Path;
import java.time.Duration;
import java.util.List;
import java.util.Map;

/**
 * {@link McpToolClient} backed by the MCP SDK's synchronous client: launches
 * {@code bdd mcp serve} as a child process over stdio — exactly what an IDE
 * host does. Pure delegation: every call forwards to the SDK and maps the
 * result through {@link SdkMappers}. Excluded from the coverage gate.
 *
 * <p>Does not call {@code initialize}. STEP 1 is {@code tools/list}. The
 * Java MCP SDK 2.0 may still open the stdio session with initialize on
 * first use; {@code bdd mcp serve} does not require that handshake.
 */
public class SdkToolClient implements McpToolClient {

    private final McpSyncClient client;

    public SdkToolClient(Path workshopRoot, Path bddBinary) {
        this(ServerLaunch.bdd(workshopRoot, bddBinary));
    }

    public SdkToolClient(ServerLaunch launch) {
        this.client = McpClient.sync(new StdioClientTransport(launch.toParameters(), McpJsonDefaults.getMapper()))
                .requestTimeout(Duration.ofMinutes(6))
                .clientInfo(new McpSchema.Implementation("tdd-workshop-agent", "1.0.0"))
                .build();
    }

    @Override
    public List<DiscoveredTool> listTools() {
        return SdkMappers.toTools(client.listTools());
    }

    @Override
    public ToolResponse callTool(String name, Map<String, Object> arguments) {
        return SdkMappers.toResponse(client.callTool(SdkMappers.toRequest(name, arguments)));
    }

    @Override
    public void close() {
        client.closeGracefully();
    }
}
