# Executable spec for the decision plane: bounded questions put to a
# local decision model, and the policy the harness applies to the typed
# answer. The model supplies a verdict; the harness owns the consequence.
Feature: Local decision model judgments
  As a developer agreeing a spec with an agent
  I want a bounded judgment on wording the regex rules cannot measure
  So that vagueness is caught without a model deciding what happens next

  Scenario: A criterion naming an exact value is judged measurable
    Given a decision model "nimble:test" is configured
    And the decision model answers "measurable" with the probability 0.97
    When the criterion "Given the input "1,2", when add is called, then the result is 3" is judged
    Then the judgment verdict is "measurable"
    And the judgment records the model "nimble:test"
    And the judgment records the question "measurable/v1"

  # The deterministic rules ask whether the outcome clause looks
  # concrete - a number, a quoted literal, a named error. A number
  # anywhere satisfies that, so this criterion earns no finding at all
  # while being unmeasurable: nobody measured code quality. That is the
  # whole reason the decision plane earns its place beside the rules.
  Scenario: Vagueness the deterministic rules cannot reach is still judged unmeasurable
    Given a decision model "nimble:test" is configured
    And the decision model answers "measurable" with the probability 0.038
    When the criterion "Given the refactored module, when the suite runs, then code quality is improved by at least 20%" is judged
    Then the judgment verdict is "not measurable"
    And the regex refiner reported no finding at all for "Given the refactored module, when the suite runs, then code quality is improved by at least 20%"

  Scenario: A probability inside the dead band is inconclusive rather than a verdict
    Given a decision model "nimble:test" is configured
    And the decision model answers "measurable" with the probability 0.55
    When the criterion "Given a calculator, when I add, then the result is valid" is judged
    Then the judgment verdict is "inconclusive"
    And the judgment action is "CONTINUE"

  Scenario: An advisory judgment adds a finding and changes no deterministic finding
    Given a decision model "nimble:test" is configured
    And the decision mode is "advisory"
    And the decision model answers "measurable" with the probability 0.04
    And a requirement "REQ-007" with story "As a user, I want fast replies so that the page feels alive."
    And the requirement has criterion "Given a request, when it is served, then the response completes before the user notices"
    When the requirement "REQ-007" is refined with judgment
    Then the refinement findings are unchanged by the judgment
    And the refinement carries a judgment for 1 criterion
    And the judgment action is "CONTINUE"

  Scenario: An unreachable decision model leaves the refiner's verdict exactly as it was
    Given the decision model is unreachable
    And the decision mode is "advisory"
    And a requirement "REQ-007" with story "As a user, I want sums so that totals come from one input."
    And the requirement has criterion "Given the input "1,2", when add is called, then the result is 3"
    When the requirement "REQ-007" is refined with judgment
    Then the refinement findings are unchanged by the judgment
    And the refinement carries no judgment
    And the refinement notes the decision model was unavailable

  # The one rule a judgment plane cannot be allowed to break: a request
  # that never produced an answer is not an answer, and never an approval.
  Scenario: An unreachable decision model in enforce mode refuses instead of approving
    Given the decision model is unreachable
    And the decision mode is "enforce"
    When the criterion "Given the input "1,2", when add is called, then the result is 3" is judged
    Then judging fails because the decision model was unavailable
    And no verdict is reported

  Scenario: A malformed answer is reported rather than read as a verdict
    Given a decision model "nimble:test" is configured
    And the decision model replies with the body "{"model":"nimble:test","answers":{"measurable":{"type":"noul"}},"usage":{"input_tokens":1,"output_tokens":1}}"
    When the criterion "Given the input "1,2", when add is called, then the result is 3" is judged
    Then judging fails because the reply was malformed
    And no verdict is reported

  Scenario: An answer to a question that was never asked is refused
    Given a decision model "nimble:test" is configured
    And the decision model replies with the body "{"model":"nimble:test","answers":{"urgency":{"type":"noul","noul":0.9}},"usage":{"input_tokens":1,"output_tokens":1}}"
    When the criterion "Given the input "1,2", when add is called, then the result is 3" is judged
    Then judging fails because the answers did not match the questions
    And no verdict is reported

  Scenario: A model that is not pulled names the pull command
    Given a decision model "absent:test" is configured
    And the decision endpoint reports the model is not found
    When the criterion "Given the input "1,2", when add is called, then the result is 3" is judged
    Then judging fails naming "ollama pull absent:test"

  Scenario: A generative model asked for a decision is refused with its capability named
    Given a decision model "chatter:test" is configured
    And the decision endpoint reports the model does not support decision
    When the criterion "Given the input "1,2", when add is called, then the result is 3" is judged
    Then judging fails because the model cannot make decisions
    And the failure names the decision capability

  # Ollama lists a decision model alongside the coding models, and the
  # session default used to be whichever one came back first.
  Scenario: Discovery never hands a decision-only model to generative work
    Given Ollama reports the model "nimble:test" with the capability "decision"
    And Ollama reports the model "coder:test" with the capability "completion"
    When the session model is resolved with nothing configured
    Then the resolved generative model is "coder:test"

  Scenario: A model whose capabilities cannot be read keeps the behaviour it always had
    Given Ollama reports the model "mystery:test" with no readable capabilities
    When the session model is resolved with nothing configured
    Then the resolved generative model is "mystery:test"

  Scenario: Only decision-capable models are offered for the decision role
    Given Ollama reports the model "nimble:test" with the capability "decision"
    And Ollama reports the model "coder:test" with the capability "completion"
    When the decision models are listed
    Then the decision model list is "nimble:test"

  Scenario: The decision model is configured without touching the generative model
    Given the config file contains:
      """
      [llm]
      model = "coder:test"
      """
    When the decision model "nimble:test" is persisted
    And the configuration is listed
    Then the config value "decision.model" is "nimble:test" from the config file
    And the config value "llm.model" is "coder:test" from the config file

  Scenario: Configuration reports the decision model and where it came from
    Given the config file contains:
      """
      [decision]
      model = "nimble:test"
      """
    When the configuration is listed
    Then the config value "decision.model" is "nimble:test" from the config file
    And the config value "decision.mode" is "advisory" from default
    And the config value "decision.min_confidence" is "0.8" from default

  # The decision endpoint defaults to wherever Ollama already is, rather
  # than making every project state the same host twice.
  Scenario: The decision endpoint follows the configured Ollama endpoint
    Given the config file contains:
      """
      [llm]
      endpoint = "http://ollama.internal:11434"
      """
    When the configuration is listed
    Then the config value "decision.endpoint" is "http://ollama.internal:11434" from the config file
