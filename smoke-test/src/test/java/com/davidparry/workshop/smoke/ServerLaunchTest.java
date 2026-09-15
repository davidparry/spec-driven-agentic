package com.davidparry.workshop.smoke;

import io.modelcontextprotocol.client.transport.ServerParameters;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.file.Path;
import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class ServerLaunchTest {

    @Test
    @DisplayName("bdd launch is mcp serve --root <absolute workshop root>")
    void bddLaunch() {
        Path root = Path.of(".").toAbsolutePath().normalize();
        Path binary = Path.of("/usr/bin/bdd");
        ServerLaunch launch = ServerLaunch.bdd(root, binary);
        assertThat(launch.program()).isEqualTo("/usr/bin/bdd");
        assertThat(launch.args()).containsExactly("mcp", "serve", "--root", root.toString());
    }

    @Test
    @DisplayName("null workshop root is refused")
    void nullRootIsRefused() {
        assertThatThrownBy(() -> ServerLaunch.bdd(null, Path.of("bdd")))
                .isInstanceOf(NullPointerException.class);
    }

    @Test
    @DisplayName("null binary is refused")
    void nullBinaryIsRefused() {
        assertThatThrownBy(() -> ServerLaunch.bdd(Path.of("."), null))
                .isInstanceOf(NullPointerException.class);
    }

    @Test
    @DisplayName("toParameters carries the program and args")
    void toParameters() {
        ServerLaunch launch = new ServerLaunch("bdd", List.of("mcp", "serve"));
        ServerParameters params = launch.toParameters();
        assertThat(params).isNotNull();
    }
}
