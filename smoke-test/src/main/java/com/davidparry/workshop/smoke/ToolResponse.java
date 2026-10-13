package com.davidparry.workshop.smoke;

/** The text of a tool call result and whether the server flagged it as an error. */
public record ToolResponse(String text, boolean error) {
}
