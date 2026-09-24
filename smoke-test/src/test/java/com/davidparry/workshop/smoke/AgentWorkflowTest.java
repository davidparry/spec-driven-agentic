package com.davidparry.workshop.smoke;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

class AgentWorkflowTest {

    @Test
    @DisplayName("firstSentence truncates at the first sentence break")
    void firstSentenceTruncates() {
        assertThat(AgentWorkflow.firstSentence("Run the suite. Updates the bar."))
                .isEqualTo("Run the suite.");
    }

    @Test
    @DisplayName("firstSentence keeps a description without a sentence break unchanged")
    void firstSentenceNoBreak() {
        assertThat(AgentWorkflow.firstSentence("Get the current phase"))
                .isEqualTo("Get the current phase");
    }

    @Test
    @DisplayName("firstSentence keeps a description starting with a sentence break unchanged")
    void firstSentenceLeadingBreak() {
        assertThat(AgentWorkflow.firstSentence(". odd but possible"))
                .isEqualTo(". odd but possible");
    }

    @Test
    @DisplayName("firstSentence of a missing description is empty")
    void firstSentenceNull() {
        assertThat(AgentWorkflow.firstSentence(null)).isEmpty();
    }
}
