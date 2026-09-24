# Executable spec for the test runners — report parsing per build tool,
# the compile-error rule, and the runtime_missing gate.
Feature: Test runners
  As a developer running the TDD loop in any supported language
  I want each build tool's reports parsed into one summary shape
  So that the RED/GREEN phase is computed the same way everywhere

  Scenario: Surefire reports are summed with failure details
    Given the Surefire report:
      """
      <?xml version="1.0"?>
      <testsuite name="Calc" tests="3" failures="1" errors="0" skipped="1">
        <testcase classname="CalcTest" name="adds">
          <failure message="expected 3 but was 4">stack</failure>
        </testcase>
      </testsuite>
      """
    Then the parsed run has 3 tests, 1 failures, 0 errors, 1 skipped
    And a parsed failure detail contains "CalcTest.adds: expected 3 but was 4"
    And a parsed failure detail contains "stack"

  Scenario: TRX counters and failed results are extracted
    Given the TRX report:
      """
      <?xml version="1.0"?>
      <TestRun>
        <Results>
          <UnitTestResult testName="Calc.Fails" outcome="Failed">
            <Output><ErrorInfo><Message>Expected 3 but was 4</Message></ErrorInfo></Output>
          </UnitTestResult>
        </Results>
        <ResultSummary><Counters total="2" passed="1" failed="1" error="0" notExecuted="0"/></ResultSummary>
      </TestRun>
      """
    Then the parsed run has 2 tests, 1 failures, 0 errors, 0 skipped
    And a parsed failure detail is "Calc.Fails: Expected 3 but was 4"

  Scenario: A cucumber-js report counts scenarios by their worst step
    Given the cucumber-js report:
      """
      [
        {"name": "Calc", "elements": [
          {"type": "scenario", "name": "adds", "steps": [{"name": "add", "result": {"status": "passed"}}]},
          {"type": "scenario", "name": "fails", "steps": [{"name": "boom", "result": {"status": "failed", "error_message": "expected 3"}}]},
          {"type": "scenario", "name": "new", "steps": [{"name": "later", "result": {"status": "undefined"}}]}
        ]}
      ]
      """
    Then the parsed run has 3 tests, 1 failures, 1 errors, 0 skipped
    And a parsed failure detail is "Calc > fails: expected 3"
    And a parsed failure detail is "Calc > new: step \"later\" is undefined"

  Scenario: Cargo test result lines are summed across binaries
    Given the cargo test output:
      """
      test domain::adds ... FAILED
      test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
      test result: ok. 3 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
      """
    Then the parsed run has 8 tests, 1 failures, 0 errors, 2 skipped
    And a parsed failure detail is "domain::adds: FAILED"

  Scenario: A build that fails before tests is one error with the output tail
    Given a Maven project whose build prints "compile error: expected ;" and fails
    When the Maven tests are run
    Then the parsed run has 0 tests, 0 failures, 1 errors, 0 skipped
    And a parsed failure detail contains "Build failed before tests could run:"
    And a parsed failure detail contains "compile error: expected ;"

  Scenario: A missing runtime is refused, never installed
    Given a Maven project on a machine without "mvn"
    When running the Maven tests is refused
    Then the refusal names runtime "mvn"
    And the refusal hint is "Install a JDK and Apache Maven, then rerun."
