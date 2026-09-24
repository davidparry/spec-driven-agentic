Feature: Deliver mode
  spec deliver is the top-level orchestrator. It resolves what to work on -
  one requirement id, the requirements a plain-words description is split
  into, or the whole pending backlog - then drives each one through
  scenario, steps, unit test, RED, implement, GREEN, refactor, and
  mark-implemented. Every step is verified against the asset survey before
  the next one starts, and the report says which requirements landed and
  which did not, so an unfinished plan can never read as a success.

  A run never stops to ask. Every proposal is accepted and every gate
  approved, and anything that genuinely needs an answer no default can
  supply is refused up front with the reason - so a run either completes
  or says why not, and never waits at a prompt.

  Scenario: A named requirement is taken through to implemented
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the delivery planned "REQ-001"
    And the delivery delivered "REQ-001"
    And the working tree file "features/kata.feature" contains "@REQ-001"
    And the working tree file "requirements/requirements.json" contains "implemented"
    And the developer was told a finding containing "[1 of 1] REQ-001"
    And the developer was told a finding containing "Saving status - working ..."
    And the delivery next step contains "All 1 planned requirement(s) are implemented"

  Scenario: A lower-case id still names the requirement
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      """
    When the delivery runs for "req-001"
    Then the delivery completes
    And the delivery planned "REQ-001"

  Scenario: An id that is not in the catalog is refused before any work starts
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    When the delivery runs for "REQ-404"
    Then the delivery error contains "No requirement with id REQ-404"
    And the delivery error contains "spec list"
    And the working tree file "features/kata.feature" does not exist

  Scenario: A requirement that is already implemented is reported, not redone
    Given a Java project marker
    And a working spec with the implemented requirement "REQ-001"
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the delivery delivered "REQ-001"
    And the developer was told a finding containing "REQ-001 is already implemented - nothing to do."

  Scenario: No argument plans every pending requirement in catalog order
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001, REQ-002"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      1 tests and 0 failures
      """
    When the delivery runs
    Then the delivery completes
    And the delivery planned "REQ-001, REQ-002"
    And the delivery delivered "REQ-001, REQ-002"
    And the developer was told a finding containing "Plan: 2 requirement(s) - REQ-001, REQ-002."
    And the developer was told a finding containing "[2 of 2] REQ-002"
    And the working tree file "features/kata.feature" contains "@REQ-002"

  Scenario: An exhausted backlog is a completed run with nothing to do
    Given a Java project marker
    And a working spec with the implemented requirement "REQ-001"
    When the delivery runs
    Then the delivery completes
    And the delivery delivered nothing
    And the delivery next step contains "Nothing is pending."

  Scenario: A description is broken down and the requirements it holds are delivered
    Given a Java project marker
    And an empty working spec
    And a model is resolved
    And the model will reply:
      """
      [{"title": "Empty string returns zero", "story": "As a calculator user, I want an empty string to return 0 so that no input is a safe default.", "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]
      """
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      """
    When the delivery runs for "empty input means zero"
    Then the delivery completes
    And the delivery planned "REQ-001"
    And the developer was told a finding containing "The description holds 1 requirement(s):"
    And the developer was told a finding containing "REQ-001 title [Empty string returns zero] (Enter keeps it): kept"
    And the working tree file "features/empty-string-returns-zero.feature" contains "@REQ-001"
    And the working tree file "requirements/requirements.json" contains "implemented"

  Scenario: Every requirement a description holds is planned, not only the first
    Given a Java project marker
    And an empty working spec
    And a model is resolved
    And the model will reply:
      """
      [{"title": "Empty string returns zero", "story": "As a calculator user, I want an empty string to return 0 so that no input is a safe default.", "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}, {"title": "Blank input is rejected", "story": "As a calculator user, I want blank input rejected so that mistakes surface early.", "acceptanceCriteria": ["Given a blank string \" \", when add is called, then an error is raised"]}]
      """
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      1 tests and 0 failures
      """
    When the delivery runs for "calculator basics"
    Then the delivery completes
    And the delivery planned "REQ-001, REQ-002"
    And the delivery delivered "REQ-001, REQ-002"
    And the developer was told a finding containing "REQ-001, REQ-002 committed to the spec."

  Scenario: Every gate is approved rather than asked about
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the delivery delivered "REQ-001"
    And the developer was not asked anything containing "Commit the generated"

  Scenario: Every proposal a description is broken into is taken as offered
    Given a Java project marker
    And an empty working spec
    And a model is resolved
    And the model will reply:
      """
      [{"title": "Empty string returns zero", "story": "As a calculator user, I want an empty string to return 0 so that no input is a safe default.", "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}]
      """
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      """
    When the delivery runs for "empty input means zero"
    Then the delivery completes
    And the delivery delivered "REQ-001"
    And the developer was told a finding containing "Stage this requirement? yes (spec deliver never stops to ask)"
    And the working tree file "requirements/requirements.json" contains "Empty string returns zero"

  Scenario: An empty directory is refused rather than asked which language to scaffold
    When the delivery runs
    Then the delivery error contains "No project was detected here"
    And the delivery error contains "spec init --language"
    And the delivery error contains "spec greenfield"
    And the developer was not asked anything containing "Language for the new project"

  Scenario: An empty catalog with nothing described is refused, not drafted from nothing
    Given a Java project marker
    And an empty working spec
    When the delivery runs
    Then the delivery error contains "nothing was described"
    And the delivery error contains "spec greenfield"
    And nothing is staged at the spec path

  Scenario: A description with no model to break it down is handed back
    Given a Java project marker
    And an empty working spec
    When the delivery runs for "empty input means zero"
    Then the delivery error contains "needs a model, and none is resolved"
    And the delivery error contains "spec model use"
    And nothing is staged at the spec path

  Scenario: One word reaching for a requirement id is refused, not drafted
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    When the delivery runs for "R-003"
    Then the delivery error contains "R-003 is not a requirement id"
    And the delivery error contains "spec list"
    And nothing is staged at the spec path

  Scenario: A RED bar with no model stops the requirement and says who must implement it
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "no model is resolved"
    And the developer was told a finding containing "Req001Test: TODO: assert"

  Scenario: The model drives a red bar to green within its budget
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 0; } }"}]
      """
    And the delivery skips the refactor
    And the delivery budget is 3 attempts
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the developer was told a finding containing "Attempt 1 of 3."
    And the developer was told a finding containing "Generating an implementation attempt - working ..."
    And the working tree file "src/main/java/Kata.java" contains "public class Kata"

  Scenario: A budget that runs out leaves the requirement outstanding with its phase
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 1; } }"}]
      """
    And the delivery skips the refactor
    And the delivery budget is 2 attempts
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 1 failures detailed "Req001Test: expected 0 but was 1"
      1 tests and 1 failures detailed "Req001Test: expected 0 but was 1"
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "still RED after 2 attempt(s)"
    And the developer was told a finding containing "Attempt 2 of 2."
    And the working tree file "requirements/requirements.json" contains "pending"
    And the delivery next step contains "spec deliver REQ-001 again"

  Scenario: A runtime that disappears mid-loop stops the requirement
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]
      """
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      runtime "JDK" missing with hint "Install a JDK 17+ and Maven."
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "runtime disappeared mid-loop"

  Scenario: A model outage spends the attempt and hands the work back
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the model is resolved but every call fails with "the endpoint refused the connection"
    And the delivery skips the refactor
    And the delivery budget is 1 attempt
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the developer was told a finding containing "Implement by hand instead."
    And the delivery leaves "REQ-001" outstanding because "still RED after 1 attempt(s)"

  Scenario: A build that cannot run at all is an error, not an outcome
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      failed "mvn exited 1: dependency resolution failed"
      """
    When the delivery runs for "REQ-001"
    Then the delivery error contains "dependency resolution failed"

  Scenario: A failure carries on to the next requirement by default
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001, REQ-002"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 0 failures
      """
    When the delivery runs
    Then the delivery is not completed
    And the delivery delivered "REQ-002"
    And the delivery leaves "REQ-001" outstanding because "no model is resolved"
    And the developer was told a finding containing "[2 of 2] REQ-002"
    And the delivery next step contains "1 of 2 delivered"

  Scenario: Fail-fast leaves the rest of the plan untouched
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001, REQ-002"
    And the delivery stops at the first failure
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      """
    When the delivery runs
    Then the delivery is not completed
    And the delivery delivered nothing
    And the developer was told a finding containing "Stopping here - the rest of the plan is untouched (--fail-fast)."
    And the delivery next step contains "REQ-001, REQ-002"

  Scenario: A missing runtime leaves the authoring in place and says so
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      runtime "JDK" missing with hint "Install a JDK 17+ and Maven."
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "the language runtime is missing"
    And the developer was told a finding containing "Runtime missing (JDK): Install a JDK 17+ and Maven."
    And the working tree file "features/kata.feature" contains "@REQ-001"

  Scenario: No detectable build tool leaves the authoring in place and says so
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And no test runner is detectable because "no supported build tool found"
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "no supported build tool found"
    And the working tree file "features/kata.feature" contains "@REQ-001"

  Scenario: Work left in staging from an earlier session is the author's to settle
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And a staged feature file "features/kata.feature" named "Kata"
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "already staged"

  Scenario: Scenarios and tests that are already in place are not written twice
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the developer was told a finding containing "REQ-001 is already implemented - nothing to do."

  Scenario: On a green bar the refactor step runs and the loop still closes
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a project source file "src/main/java/Kata.java" containing:
      """
      public class Kata {
          int add(String input) { if (input.isEmpty()) { return 0; } return 0; }
      }
      """
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata {\n    int add(String input) {\n        return 0;\n    }\n}\n"}]
      """
    And the test runs will report:
      """
      1 tests and 0 failures
      1 tests and 0 failures
      1 tests and 0 failures
      1 tests and 0 failures
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the developer was not asked anything containing "Run the refactor step"
    And the developer was told a finding containing "Refactored src/main/java/Kata.java."
    And the working tree file "src/main/java/Kata.java" contains "int add(String input) {"
    And the working tree file "requirements/requirements.json" contains "implemented"

  Scenario: --no-refactor is how the refactor step is skipped
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]
      """
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the developer was not asked anything containing "Run the refactor step"
    And the working tree file "requirements/requirements.json" contains "implemented"

  Scenario: Without a model there is no refactor step to run
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And the test runs will report:
      """
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the developer was not asked anything containing "Run the refactor step"

  Scenario: A refactor with nothing to refactor is skipped, not fatal
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]
      """
    And the test runs will report:
      """
      1 tests and 0 failures
      1 tests and 0 failures
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the developer was told a finding containing "Skipping the refactor."
    And the working tree file "requirements/requirements.json" contains "implemented"

  Scenario: A requirement whose criteria are not Given/When/Then cannot be delivered
    Given a Java project marker
    And a working spec with the pending requirement "REQ-001" whose criteria are unshaped
    And the delivery skips the refactor
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "is Given/When/Then shaped"
    And the developer was told a finding containing "Skipping criterion"
    And nothing is staged at the spec path
