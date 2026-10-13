Feature: Connecting a requirement to the production file its work belongs in

  Nothing on disk always says where a requirement's code goes. Its
  scenarios may have been generated minutes ago with their steps still
  placeholders, reaching no production code at all, and any project
  past a kata has several files the work could land in. The loop puts
  that choice to the model instead of printing advice an unattended run
  has nobody to read it.

  Background:
    Given a Java project marker
    And a working spec whose requirement "REQ-001" is "pending" with feature file "features/calc.feature"
    And a project source file "src/main/java/Alpha.java" containing:
      """
      public class Alpha {}
      """
    And a project source file "src/main/java/Beta.java" containing:
      """
      public class Beta {}
      """

  @HARNESS-024
  Scenario: The model is asked which of the candidate files the work belongs in
    Given the model will reply:
      """
      {"productionFile": "src/main/java/Beta.java"}
      """
    When the delivery resolves where "REQ-001" belongs
    Then the work is connected to "src/main/java/Beta.java" by the model
    And the attempt is pointed at "src/main/java/Beta.java"

  @HARNESS-024
  Scenario: With no model the conventional file stands and the way to override it is named
    When the delivery resolves where "REQ-001" belongs without a model
    Then the work falls back to "src/main/java/Kata.java" by convention
    And the developer was told a finding containing "spec set-production REQ-001 --file"

  @HARNESS-024
  Scenario: An invalid choice naming a file nobody offered is refused
    Given the model will reply:
      """
      {"productionFile": "src/main/java/Invented.java"}
      """
    When the delivery resolves where "REQ-001" belongs
    Then the work falls back to "src/main/java/Kata.java" by convention
    And the developer was told a finding containing "did not choose one of the candidates"
