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
    @DisplayName("spec launch is mcp serve --root <absolute workshop root>")
    void specLaunch() {
        Path root = Path.of(".").toAbsolutePath().normalize();
        Path binary = Path.of("/usr/bin/spec");
        ServerLaunch launch = ServerLaunch.spec(root, binary);
        assertThat(launch.program()).isEqualTo("/usr/bin/spec");
        assertThat(launch.args()).containsExactly("mcp", "serve", "--root", root.toString());
    }

    @Test
    @DisplayName("null workshop root is refused")
    void nullRootIsRefused() {
        assertThatThrownBy(() -> ServerLaunch.spec(null, Path.of("spec")))
                .isInstanceOf(NullPointerException.class);
    }

    @Test
    @DisplayName("null binary is refused")
    void nullBinaryIsRefused() {
        assertThatThrownBy(() -> ServerLaunch.spec(Path.of("."), null))
                .isInstanceOf(NullPointerException.class);
    }

    @Test
    @DisplayName("toParameters carries the program and args")
    void toParameters() {
        ServerLaunch launch = new ServerLaunch("spec", List.of("mcp", "serve"));
        ServerParameters params = launch.toParameters();
        assertThat(params).isNotNull();
    }
}
