package com.davidparry.workshop.smoke;

import java.util.function.Consumer;

/**
 * Writes the walkthrough narration, one line (possibly multi-line) at a
 * time. Injected with the sink so tests can capture and assert on everything
 * the agent says; the composition root passes {@code System.out::println}.
 */
public class Narrator {

    private static final String RULE = "=".repeat(72);

    private final Consumer<String> lineSink;

    public Narrator(Consumer<String> lineSink) {
        this.lineSink = lineSink;
    }

    public void banner(String title) {
        lineSink.accept("");
        lineSink.accept(RULE);
        lineSink.accept("  " + title);
        lineSink.accept(RULE);
    }

    public void say(String message) {
        lineSink.accept(message);
    }

    /** Indents every line by four spaces, for quoted server output. */
    public static String indent(String text) {
        return text.lines().map(l -> "    " + l).reduce((a, b) -> a + "\n" + b).orElse("");
    }
}
