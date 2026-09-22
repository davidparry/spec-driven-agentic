Feature: Project configuration dump
  As a developer checking how the harness is set up
  I want `spec config` to list every key and whether it is a default or came from the file
  So that I can see what `.spec/config.toml` actually overrides

  Scenario: No config file reports every key as default
    When the configuration is listed
    Then the config file status is "(none)"
    And the config value "llm.model" is "(unset)" from default
    And the config value "llm.endpoint" is "http://localhost:11434" from default
    And the config value "tools.max_rounds" is "12" from default
    And the config value "tools.profiles.implement" is from default

  Scenario: Values present in the file are attributed to that file
    Given the config file contains:
      """
      [llm]
      model = "mine"
      timeout_seconds = 900
      [tools]
      max_rounds = 4
      [tools.profiles]
      status = ["get_tdd_state"]
      [tools.enabled]
      implement = ["feature_list"]
      """
    When the configuration is listed
    Then the config file status contains ".spec/config.toml"
    And the config value "llm.model" is "mine" from the config file
    And the config value "llm.endpoint" is "http://localhost:11434" from default
    And the config value "llm.timeout_seconds" is "900" from the config file
    And the config value "tools.max_rounds" is "4" from the config file
    And the config value "tools.profiles.status" is "get_tdd_state" from the config file
    And the config value "tools.profiles.implement" is from default
    And the config value "tools.enabled.implement" is "feature_list" from the config file

  Scenario: Invalid TOML is flagged and every key stays the default
    Given the config file contains:
      """
      not = = toml
      """
    When the configuration is listed
    Then the config file status contains "invalid TOML"
    And the config value "llm.model" is "(unset)" from default
