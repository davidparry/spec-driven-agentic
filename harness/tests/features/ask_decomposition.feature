# Executable spec for the ask workflow - a request preserved in the
# user's own words as an ask_requirement in requirements.json, broken
# down into actionable_requirement entries that point back at it, and
# ready for implementation only when every one of them is accepted.
# Who accepted (the rules or a person) is recorded; a change of wording
# withdraws it; running the same thing twice changes nothing.
Feature: Ask decomposition
  As a developer whose request becomes several requirements
  I want the request kept, the breakdown decided, and readiness derived
  So that nobody implements a requirement the breakdown never accepted

  Background:
    Given a working spec with the pending requirement "REQ-001"

  # --- the default run: no prompts -------------------------------------

  @HARNESS-020 @HARNESS-021
  Scenario: A default run accepts what the rules pass and asks nobody
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"title": "Empty string returns zero",
        "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
        "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]
      """
    When the request "sum numbers from a comma separated string, empty input means zero" is broken down without prompts
    Then nobody was asked anything
    And the ask "ASK-001" preserves the request "sum numbers from a comma separated string, empty input means zero"
    And the ask "ASK-001" derives "REQ-002, REQ-003"
    And "REQ-002" is accepted under "ASK-001" automatically
    And "REQ-003" is accepted under "ASK-001" automatically
    And the ask reports "REQ-002, REQ-003" accepted
    And the ask "ASK-001" is complete
    And the ask "ASK-001" is ready
    And the written spec has 4 requirements
    And the written spec validates clean

  @HARNESS-021
  Scenario: A default run records a proposal the rules cannot pass instead of guessing
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"title": "Empty string returns zero",
        "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
        "acceptanceCriteria": ["the calculator should handle empty input quickly"]}]
      """
    When the request "sum numbers from a comma separated string, empty input means zero" is broken down without prompts
    Then nobody was asked anything
    And the ask "ASK-001" derives "REQ-002"
    And the ask reports proposal "2" as pending
    And the ask "ASK-001" is not ready
    And an ask reason contains "proposal 2 "Empty string returns zero" is still pending"
    And the written spec has 3 requirements

  # The deterministic split reads structure, not meaning: each listed
  # item becomes a titled proposal that waits for a story and criteria
  # someone actually words. Wording one under that title fills the row.
  @HARNESS-020
  Scenario: Without a model the rules split only what the request enumerates, and invent no wording
    When the request "1. sum numbers from a comma separated string; 2. an empty string sums to zero" is broken down without prompts
    Then nobody was asked anything
    And the ask "ASK-001" preserves the request "1. sum numbers from a comma separated string; 2. an empty string sums to zero"
    And the ask "ASK-001" derives ""
    And the ask reports proposals "1, 2" as pending
    And an ask reason contains "proposal 1 "Sum numbers from a comma separated string" is still pending"
    And the ask "ASK-001" is not ready
    When a requirement titled "Sum numbers from a comma separated string" is created under "ASK-001" by the user with:
      """
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      """
    And a requirement titled "An empty string sums to zero" is created under "ASK-001" by the user with:
      """
      As a user, I want empty input to be 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      """
    Then the ask "ASK-001" derives "REQ-002, REQ-003"
    And the ask reports proposals "" as pending
    And the ask "ASK-001" is ready
    When the ask "ASK-001" is run again without prompts
    Then the ask reports proposals "" as pending
    And the ask "ASK-001" is ready

  @HARNESS-020
  Scenario: Without a model a request that enumerates nothing is preserved with a question, not invented
    When the request "make the calculator nicer" is broken down without prompts
    Then nobody was asked anything
    And the ask "ASK-001" preserves the request "make the calculator nicer"
    And the ask "ASK-001" derives ""
    And the ask "ASK-001" has an open blocking question containing "nothing was split"
    And the ask "ASK-001" is not ready
    And the written spec has 2 requirements

  # --- the interactive run -----------------------------------------------

  @HARNESS-021
  Scenario: Revising a proposal rewrites it in the user's words before it is approved
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string
      v
      <empty>
      <empty>
      <empty>
      Given an empty string "", when add is called, then the result is 0
      <empty>
      a
      y
      """
    When a requirement is drafted with the model's help
    Then the developer was asked "Proposal 1 - [a]pprove, [r]eject, re[v]ise, or e[x]clude from scope [a/r/v/x]:"
    And the developer was asked "proposal 1 title [Comma separated numbers are summed] (Enter keeps it):"
    And the developer was told a finding containing "REQ-002 accepted by you."
    And the written requirement "REQ-002" has 2 criteria
    And "REQ-002" is accepted under "ASK-001" by the user
    And the ask "ASK-001" is complete and confirmed
    And the ask "ASK-001" is ready

  @HARNESS-022
  Scenario: Excluding a proposal from scope records why and does not hold the ask back
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"title": "Empty string returns zero",
        "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
        "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string, empty input means zero
      a
      x
      out of scope for the first release
      y
      """
    When a requirement is drafted with the model's help
    Then the ask "ASK-001" derives "REQ-002"
    And the ask reports proposal "2" as excluded
    And the ask "ASK-001" is ready

  @HARNESS-021
  Scenario: An empty answer is not a vote and leaving coverage unconfirmed leaves the ask open
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string
      <empty>
      a
      n
      <empty>
      """
    When a requirement is drafted with the model's help
    Then the developer was told a finding containing "An empty answer is not a vote."
    And "REQ-002" is accepted under "ASK-001" by the user
    And the ask "ASK-001" is open
    And the ask "ASK-001" is not ready
    And an ask reason contains "the user has not confirmed"

  # An answer is worth another split: the model is briefed with what it
  # did not know the first time rather than being asked the same thing.
  @HARNESS-020
  Scenario: An answered question splits the request again with the answer in the brief
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"question": "Should a sum be rounded?", "blocking": true}]
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string
      a
      round results to two decimals
      y
      """
    When a requirement is drafted with the model's help
    Then the developer was asked "Question 1 (blocking): Should a sum be rounded? (Enter leaves it open):"
    And the developer was told a finding containing "Splitting again with your answers in the brief."
    And the ask "ASK-001" has no open question
    And the ask "ASK-001" derives "REQ-002"
    And the ask "ASK-001" is complete and confirmed
    And the ask "ASK-001" is ready

  # Nothing was accepted, so there is no coverage to put to the user:
  # asking whether nothing covers the request is not a question worth
  # answering.
  @HARNESS-021
  Scenario: Rejecting the only proposal leaves no coverage to confirm
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string
      r
      already covered by REQ-001
      """
    When a requirement is drafted with the model's help
    Then the developer was told a finding containing "Nothing is accepted yet, so there is no coverage to confirm."
    And the developer was not asked anything containing "cover the whole request"
    And the ask "ASK-001" derives ""
    And the ask reports proposal "1" as rejected
    And the ask "ASK-001" is not ready

  # What is missing becomes part of the brief and buys another round, so
  # the gap the user named is what the next split is told about.
  @HARNESS-022
  Scenario: Answering what is missing is recorded and splits the request again
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the developer will answer:
      """
      sum numbers from a comma separated string
      a
      n
      negative numbers must be refused
      y
      """
    When a requirement is drafted with the model's help
    Then the developer was asked "What is missing? (added to the brief; Enter stops here):"
    And the ask "ASK-001" has a question containing "What is still missing" answered "negative numbers must be refused"
    And the ask "ASK-001" has no open question
    And the ask "ASK-001" is complete and confirmed
    And the ask "ASK-001" is ready

  # Interactive, but with no model to re-split with: an unconfirmed
  # coverage is the end of the run, and the advice names what can still
  # be done by hand rather than asking for a round that cannot happen.
  @HARNESS-021
  Scenario: Without a model an unconfirmed coverage leaves the decomposition open and says so
    Given the developer will answer:
      """
      v
      <empty>
      As a user, I want comma sums so that totals come from one input.
      Given the input "1,2", when add is called, then the result is 3
      <empty>
      a
      x
      out of scope for the first release
      n
      """
    When the request "1. sum numbers from a comma separated string; 2. an empty string sums to zero" is broken down with the developer and no model
    Then the developer was asked "proposal 1 story (As a ..., I want ..., so that ...):"
    And the developer was told a finding containing "The decomposition stays open."
    And the developer was not asked anything containing "What is missing?"
    And the ask "ASK-001" derives "REQ-002"
    And the ask reports proposal "2" as excluded
    And the ask "ASK-001" is open
    And the ask "ASK-001" is not ready

  # --- decisions recorded without a wizard -------------------------------

  @HARNESS-021
  Scenario: A decision by number settles what the rules left pending, and only a person may confirm
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"title": "Empty string returns zero",
        "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
        "acceptanceCriteria": ["the calculator should handle empty input quickly"]}]
      """
    And the request "sum numbers from a comma separated string, empty input means zero" is broken down without prompts
    When decisions on "ASK-001" accept proposal "2" by the review
    Then the mutation error contains "cannot be accepted automatically"
    When decisions on "ASK-001" confirm coverage automatically
    Then the mutation error contains "user's decision"
    When decisions on "ASK-001" accept proposal "2" by the user
    Then the ask "ASK-001" derives "REQ-002, REQ-003"
    And "REQ-003" is accepted under "ASK-001" by the user
    And the ask "ASK-001" is ready

  @HARNESS-022
  Scenario: A blocking question keeps the ask from being ready until it is answered
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"question": "Which delimiters besides the comma count?", "blocking": true}]
      """
    When the request "sum numbers from a delimited string" is broken down without prompts
    Then "REQ-002" is accepted under "ASK-001" automatically
    And the ask "ASK-001" has an open blocking question containing "Which delimiters"
    And the ask "ASK-001" is not ready
    And an ask reason contains "question 1 is blocking and unanswered"
    When decisions on "ASK-001" answer question 1 with "only the comma"
    Then the ask "ASK-001" has no open question
    And the ask "ASK-001" is ready

  # --- parent and child on disk, and how they come apart ------------------

  @HARNESS-022
  Scenario: A requirement created under an ask joins it and a deleted one reopens it
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the request "sum numbers from a comma separated string" is broken down without prompts
    When a requirement titled "Empty string returns zero" is created under "ASK-001" by the user with:
      """
      As a user, I want empty input to be 0 so that no input is a safe default.
      Given an empty string "", when add is called, then the result is 0
      """
    Then the ask "ASK-001" derives "REQ-002, REQ-003"
    And "REQ-003" is accepted under "ASK-001" by the user
    And the ask "ASK-001" is ready
    When requirement "REQ-003" is deleted
    Then the written requirement "REQ-003" is absent
    And the ask "ASK-001" derives "REQ-002"
    And the ask reports proposal "2" as pending
    And the ask "ASK-001" is not ready

  @HARNESS-022
  Scenario: Deleting an ask that still derives requirements needs cascade
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the request "sum numbers from a comma separated string" is broken down without prompts
    When requirement "ASK-001" is deleted
    Then the mutation error contains "ASK-001 derives REQ-002"
    When requirement "ASK-001" is deleted with cascade
    Then the written requirement "ASK-001" is absent
    And the written requirement "REQ-002" is absent
    And the written spec has 1 requirement

  @HARNESS-020
  Scenario: Unlinking a requirement takes it out from under its ask
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the request "sum numbers from a comma separated string" is broken down without prompts
    When requirement "REQ-002" is unlinked from its ask
    Then "REQ-002" stands on its own
    And the ask "ASK-001" derives ""
    And the ask reports proposal "1" as excluded

  # --- acceptance follows the wording ------------------------------------

  @HARNESS-021
  Scenario: Rewording a derived requirement in meaning withdraws its acceptance
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the request "sum numbers from a comma separated string" is broken down without prompts
    And the ask "ASK-001" is ready
    When requirement "REQ-002" is retitled "Sums numbers separated by commas or newlines"
    Then "REQ-002" is not accepted
    And the ask "ASK-001" derives "REQ-002"
    And the ask "ASK-001" is not ready
    And an ask reason contains "proposal 1 "Sums numbers separated by commas or newlines" is still pending"
    When decisions on "ASK-001" accept proposal "1" by the user
    Then "REQ-002" is accepted under "ASK-001" by the user
    And the ask "ASK-001" is ready

  @HARNESS-021
  Scenario: A whitespace-only edit keeps the acceptance
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the request "sum numbers from a comma separated string" is broken down without prompts
    When requirement "REQ-002" is retitled "Comma   separated numbers are summed"
    Then "REQ-002" is accepted under "ASK-001" automatically
    When the ask "ASK-001" is run again without prompts
    Then the ask "ASK-001" is ready

  @HARNESS-020
  Scenario: Rewording the request reopens the ask and keeps what was derived
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the request "sum numbers from a comma separated string" is broken down without prompts
    When the request of "ASK-001" is reworded to "sum numbers from a comma separated string, and an empty string sums to zero"
    Then the ask "ASK-001" preserves the request "sum numbers from a comma separated string, and an empty string sums to zero"
    And the ask "ASK-001" is open
    And "REQ-002" is accepted under "ASK-001" automatically
    When the ask "ASK-001" is run again without prompts
    Then the ask "ASK-001" derives "REQ-002"
    And the ask "ASK-001" is ready

  # --- repeated execution and compatibility -------------------------------

  @HARNESS-020
  Scenario: Running the same request again adds nothing and rewrites nothing
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]},
       {"title": "Empty string returns zero",
        "story": "As a user, I want empty input to be 0 so that no input is a safe default.",
        "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]
      """
    And the request "sum numbers from a comma separated string, empty input means zero" is broken down without prompts
    And the written spec is remembered
    When the request "sum numbers from a comma separated string, empty input means zero" is broken down without prompts
    Then the written spec is byte for byte what was remembered
    And the written spec has 4 requirements
    When the ask "ASK-001" is run again without prompts
    Then the written spec is byte for byte what was remembered

  @HARNESS-020
  Scenario: A requirement that was never part of an ask is written back exactly as it was
    Given the model will reply:
      """
      [{"title": "Comma separated numbers are summed",
        "story": "As a user, I want comma sums so that totals come from one input.",
        "acceptanceCriteria": ["Given the input \"1,2\", when add is called, then the result is 3"]}]
      """
    And the request "sum numbers from a comma separated string" is broken down without prompts
    Then the written requirement "REQ-001" carries no ask field
    And "REQ-001" stands on its own
    And the written spec validates clean
