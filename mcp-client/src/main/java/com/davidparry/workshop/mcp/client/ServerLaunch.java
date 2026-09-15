package com.davidparry.workshop.mcp.client;

import io.modelcontextprotocol.client.transport.ServerParameters;

import java.nio.file.Path;
import java.util.List;
import java.util.Objects;

/**
 * How the client starts the workflow server. The only supported server
 * is {@code bdd mcp serve}; the launch decision lives here so it can be
 * unit-tested instead of hiding inside the JaCoCo-excluded SDK glue.
 */
public record ServerLaunch(String program, List<String> args) {

    public ServerLaunch {
        Objects.requireNonNull(program, "program");
        args = List.copyOf(Objects.requireNonNull(args, "args"));
    }

    public static ServerLaunch bdd(Path workshopRoot, Path bddBinary) {
        Objects.requireNonNull(workshopRoot, "workshopRoot");
        Objects.requireNonNull(bddBinary, "bddBinary");
        return new ServerLaunch(
                bddBinary.toString(),
                List.of(
                        "mcp",
                        "serve",
                        "--root",
                        workshopRoot.toAbsolutePath().normalize().toString()));
    }

    public ServerParameters toParameters() {
        return ServerParameters.builder(program)
                .args(args.toArray(String[]::new))
                .build();
    }
}
