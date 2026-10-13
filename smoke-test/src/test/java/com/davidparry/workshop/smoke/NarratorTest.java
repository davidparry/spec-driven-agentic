package com.davidparry.workshop.smoke;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;

class NarratorTest {

    private final List<String> captured = new ArrayList<>();
    private final Narrator narrator = new Narrator(captured::add);

    @Test
    @DisplayName("banner frames the title between rules")
    void bannerFramesTitle() {
        narrator.banner("STEP 1");
        assertThat(captured).containsExactly("", "=".repeat(72), "  STEP 1", "=".repeat(72));
    }

    @Test
    @DisplayName("say emits the message as one line")
    void sayEmitsLine() {
        narrator.say("hello");
        assertThat(captured).containsExactly("hello");
    }

    @Test
    @DisplayName("indent prefixes every line with four spaces")
    void indentPrefixesLines() {
        assertThat(Narrator.indent("a\nb")).isEqualTo("    a\n    b");
    }

    @Test
    @DisplayName("indenting empty text yields empty text")
    void indentEmpty() {
        assertThat(Narrator.indent("")).isEmpty();
    }
}
