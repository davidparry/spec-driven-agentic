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
    And the working tree file "src/test/resources/features/empty-string-returns-zero.feature" contains "@REQ-001"
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
    And the developer was told a finding containing "Write this requirement? yes (spec deliver never stops to ask)"
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
    And the spec file is unchanged

  Scenario: A description with no model to break it down is handed back
    Given a Java project marker
    And an empty working spec
    When the delivery runs for "empty input means zero"
    Then the delivery error contains "needs a model, and none is resolved"
    And the delivery error contains "spec model use"
    And the spec file is unchanged

  Scenario: One word reaching for a requirement id is refused, not drafted
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    When the delivery runs for "req7"
    Then the delivery error contains "req7 is not a requirement id"
    And the delivery error contains "spec list"
    And the spec file is unchanged

  # "R-003" reads as an id - an uppercase prefix, a dash, a number - so
  # the catalog is what decides, not the shape. A prefix the catalog does
  # not use is reported by name rather than drafted from as prose.
  Scenario: An id shaped correctly but naming nothing is reported by name
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    When the delivery runs for "R-003"
    Then the delivery error contains "No requirement with id R-003"
    And the delivery error contains "spec list"
    And the spec file is unchanged

  # The prefix belongs to the catalog: this crate's own spec numbers
  # HARNESS-014, and the same command has to reach it.
  Scenario: A requirement carrying the catalog's own prefix is delivered
    Given a Java project marker
    And a working spec with the pending requirements "HARNESS-001"
    And the delivery skips the refactor
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Harness001Test: TODO: assert"
      """
    When the delivery runs for "HARNESS-001"
    Then the delivery is not completed
    And the delivery planned "HARNESS-001"

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
    And the model will also fill in the generated unit test
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

  # The generated unit test ships with a `TODO: assert` placeholder on
  # purpose - writing the assertion is the point of the exercise. An
  # attempt that writes production code and leaves the placeholder
  # standing has written nothing the suite can fail on, and six measured
  # attempts did exactly that while the failure count climbed.
  Scenario: An attempt that leaves the generated placeholder standing is refused
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 0; } }"}]
      """
    And the delivery skips the refactor
    And the delivery budget is 1 attempts
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the developer was told a finding containing "still carries the generated placeholder"

  # The file a delivery wrote is recorded on the requirement, so the next
  # attempt writes where the last one did instead of inferring the path
  # again from step definitions that may not name it.
  Scenario: A delivery records where it wrote the production code
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 0; } }"}]
      """
    And the model will also fill in the generated unit test
    And the delivery skips the refactor
    And the delivery budget is 3 attempts
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the working tree file "requirements/requirements.json" contains "productionFiles"
    And the working tree file "requirements/requirements.json" contains "src/main/java/Kata.java"

  # A run nobody watched can still be read for what the decision plane
  # made of each gated stage. The judgments ride in the report per stage,
  # and the run says them as it goes, the same way `spec refine` does.
  Scenario: A delivery reports what the decision model made of each gated stage
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And a decision model that answers every question favourably is configured for the project
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 0; } }"}]
      """
    And the model will also fill in the generated unit test
    And the delivery skips the refactor
    And the delivery budget is 3 attempts
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the delivery report carries a "drive_to_green" judgment from the gate "UNIT_TEST_ASSERTS"
    And the delivery report carries a "drive_to_green" judgment from the gate "IMPLEMENTATION_COMPLETE"
    And every delivery judgment reads "HOLDS"
    And the developer was told a finding containing "A second review (IMPLEMENTATION_COMPLETE, implementation_complete/v1) judged 1 file:"

  # The same run with no decision model named reports exactly what it
  # reported before there was a plane to consult.
  Scenario: A delivery without a decision model carries no judgments
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add(String input) { return 0; } }"}]
      """
    And the model will also fill in the generated unit test
    And the delivery skips the refactor
    And the delivery budget is 3 attempts
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the delivery report carries no judgments

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

  # A build error and a failing test are both red bars to the state
  # machine, and the implement loop can only move one of them. Spending
  # the budget writing production code against a compiler error is how a
  # run burns three model calls and finishes further back than it began.
  Scenario: A bar that is a build error stops the requirement instead of spending attempts
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]
      """
    And the delivery skips the refactor
    And the delivery budget is 3 attempts
    And the test runs will report:
      """
      the build failed with "Req001Test.java:12: cannot find symbol"
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "before any test ran"
    And the developer was told a finding containing "generated unit test is the usual cause"
    And the developer was not told a finding containing "Attempt 1 of 3."

  # "Templates are the fallback" is the run's second line. The polish
  # pass checks a unit test's shape and never whether it compiles, so a
  # model that adds a helper the file cannot build is the one break the
  # run can mend by itself: the template it polished compiles.
  Scenario: A unit test the model broke is rewritten from the template before the run stops
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]
      """
    And the model will polish the unit test into:
      """
      import org.junit.jupiter.api.Test;

      class Req001Test {
          @Test
          void addsNumbers() { Helper.notDeclaredAnywhere(); }
      }
      """
    And the model will also fill in the generated unit test
    And the delivery skips the refactor
    And the delivery budget is 1 attempt
    And the test runs will report:
      """
      the build failed with "Req001Test.java:5: cannot find symbol Helper"
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      1 tests and 0 failures
      """
    When the delivery runs for "REQ-001"
    Then the delivery completes
    And the delivery delivered "REQ-001"
    And the developer was told a finding containing "rewritten from the template"
    And the developer was told a finding containing "Attempt 1 of 1."
    And the working tree file "src/test/java/Req001Test.java" does not contain "Helper.notDeclaredAnywhere"

  # The same break with the template already in place is somebody
  # else's: there is nothing to fall back to, so the stop stands.
  Scenario: A build broken by something other than the model's unit test still stops the requirement
    Given a Java project marker
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]
      """
    And the model will polish the unit test into:
      """
      import org.junit.jupiter.api.Test;

      class Req001Test {
          @Test
          void addsNumbers() { Helper.notDeclaredAnywhere(); }
      }
      """
    And the delivery skips the refactor
    And the delivery budget is 1 attempt
    And the test runs will report:
      """
      the build failed with "Req001Test.java:5: cannot find symbol Helper"
      the build failed with "Kata.java:1: class Kata is public, should be declared in a file named Kata.java"
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the developer was told a finding containing "rewritten from the template"
    And the delivery leaves "REQ-001" outstanding because "before any test ran"
    And the developer was not told a finding containing "Attempt 1 of 1."

  # The attempt that breaks the build is worse than the one before it:
  # it briefs the next attempt with a compiler error instead of a failing
  # test, and leaves the break on disk when the budget runs out.
  Scenario: An attempt that stops the code compiling is put back
    Given a Java project marker
    And a project source file "src/main/java/Kata.java" containing:
      """
      public class Kata { int add(String input) { return 0; } }
      """
    And a working spec with the pending requirements "REQ-001"
    And a model is resolved
    And the model will reply:
      """
      [{"path": "src/main/java/Kata.java", "content": "public class Kata { int add( }"}]
      """
    And the model will also fill in the generated unit test
    And the delivery skips the refactor
    And the delivery budget is 1 attempt
    And the test runs will report:
      """
      1 tests and 1 failures detailed "Req001Test: TODO: assert"
      the build failed with "Kata.java:1: ')' expected"
      """
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the developer was told a finding containing "Attempt 1 left the build not compiling"
    And the developer was told a finding containing "src/main/java/Kata.java"
    And the working tree file "src/main/java/Kata.java" contains "return 0;"

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

  # The failure this guards against: deliver wrote the Gherkin to
  # features/<slug>.feature while the catalog read the project's own
  # features root, so a scenario could be written and then read back as
  # missing. A drafted requirement carries no featureFile, which is the
  # only case where deliver chooses the path itself.
  Scenario: A drafted requirement's scenarios land in the project's own features root
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
    And the working tree file "src/test/resources/features/empty-string-returns-zero.feature" contains "@REQ-001"
    And the working tree file "features/empty-string-returns-zero.feature" does not exist
    And the developer was not told a finding containing "can be read back"

  Scenario: A requirement whose criteria are not Given/When/Then cannot be delivered
    Given a Java project marker
    And a working spec with the pending requirement "REQ-001" whose criteria are unshaped
    And the delivery skips the refactor
    When the delivery runs for "REQ-001"
    Then the delivery is not completed
    And the delivery leaves "REQ-001" outstanding because "is Given/When/Then shaped"
    And the developer was told a finding containing "Skipping criterion"
    And the spec file is unchanged
