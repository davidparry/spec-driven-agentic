# Executable spec for LLM model resolution — flag over configuration over
# discovery. Discovery never persists anything: an installed model is
# only a session default until the user explicitly picks one.
Feature: LLM model selection
  As a harness user generating scenarios and tests with a local LLM
  I want the model resolved from flag, configuration, or Ollama discovery
  So that generation uses the model I chose and never installs anything

  Scenario: The command-line flag wins over everything
    Given the configured model is "configured-model"
    And Ollama has models "alpha, beta"
    When the model is resolved with flag "flagged-model"
    Then the model resolves to "flagged-model" from the flag

  Scenario: The configured model wins over discovery
    Given the configured model is "configured-model"
    And Ollama has models "alpha, beta"
    When the model is resolved without a flag
    Then the model resolves to "configured-model" from configuration

  Scenario: A single installed model is used automatically
    Given Ollama has models "solo"
    When the model is resolved without a flag
    Then the model resolves to "solo" as the only installed model
    And no model choice is persisted

  Scenario: With several models and no configuration the first is the session default
    Given Ollama has models "alpha, beta, gamma"
    When the model is resolved without a flag
    Then the model resolves to "alpha" as the session default
    And no model choice is persisted

  Scenario: No installed models means generation is unavailable
    Given Ollama has no models
    When the model is resolved without a flag
    Then resolution is unavailable with a message containing "no models installed"

  Scenario: An unreachable Ollama means generation is unavailable
    Given Ollama is unreachable
    When the model is resolved without a flag
    Then resolution is unavailable with a message containing "cannot reach the model provider"

  Scenario: Session startup announces the configured model
    Given the configured model is "configured-model"
    And Ollama has models "alpha, beta"
    When the session model status is checked
    Then the session is ready with model "configured-model"

  Scenario: Session startup borrows the first installed model without saving it
    Given Ollama has models "alpha, beta"
    When the session model status is checked
    Then the session is ready with model "alpha"
    And no model choice is persisted

  Scenario: Session startup with no models installed says to pull one
    Given Ollama has no models
    When the session model status is checked
    Then the session reports that no models are installed

  Scenario: Session startup with Ollama unreachable says to install it
    Given Ollama is unreachable
    When the session model status is checked
    Then the session reports the provider is down with "connection refused"

  Scenario: Listing models returns every installed model
    Given Ollama has models "alpha, beta"
    When the models are listed
    Then 2 models are listed
    And a listed model is "alpha"
    And a listed model is "beta"

  Scenario: Listing models with an empty catalog returns none
    Given Ollama has no models
    When the models are listed
    Then 0 models are listed

  Scenario: Choosing a model that is not installed is rejected
    Given Ollama has models "alpha, beta"
    When the model "zzz" is chosen
    Then the choice is rejected with a message containing "available: alpha, beta"

  Scenario: Choosing an installed model persists it
    Given Ollama has models "alpha, beta"
    When the model "beta" is chosen
    Then the persisted model is "beta"
