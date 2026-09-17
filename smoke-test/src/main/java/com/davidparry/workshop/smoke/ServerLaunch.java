package com.davidparry.workshop.smoke;

import io.modelcontextprotocol.client.transport.ServerParameters;

import java.nio.file.Path;
import java.util.List;
import java.util.Objects;

/**
 * How the client starts the workflow server. The only supported server
 * is {@code spec mcp serve}; the launch decision lives here so it can be
 * unit-tested instead of hiding inside the JaCoCo-excluded SDK glue.
 */
public record ServerLaunch(String program, List<String> args) {

    public ServerLaunch {
        Objects.requireNonNull(program, "program");
        args = List.copyOf(Objects.requireNonNull(args, "args"));
    }

    public static ServerLaunch spec(Path workshopRoot, Path specBinary) {
        Objects.requireNonNull(workshopRoot, "workshopRoot");
        Objects.requireNonNull(specBinary, "specBinary");
        return new ServerLaunch(
                specBinary.toString(),
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
