// Generated from HARNESS-023: Explain specific command by name

use spec_harness::explain_specific_command_by_name::{CommandEntry, CommandTree};

#[test]
fn given_a_valid_command_name_run_exists_in_the_command_tree_when_the_explain_command_is_called_with_run_then_the_output_displays_the_command_s_description_its_options_and_the_list_of_tools_it_calls() {
    // Given a valid command name "run" exists in the command tree, when the explain command is called with "run", then the output displays the command's description, its options, and the list of tools it calls
    let mut tree = CommandTree::new();
    tree.register(CommandEntry {
        name: "run".into(),
        description: "Execute the test harness".into(),
        options: vec!["--config".into(), "--verbose".into()],
        tools: vec!["cargo".into(), "java".into()],
    });

    let output = tree
        .explain("run")
        .expect("explain should succeed for a known command");

    assert!(
        output.contains("Execute the test harness"),
        "output must contain the command description"
    );
    assert!(
        output.contains("--config"),
        "output must list the --config option"
    );
    assert!(
        output.contains("--verbose"),
        "output must list the --verbose option"
    );
    assert!(
        output.contains("cargo"),
        "output must list the cargo tool dependency"
    );
    assert!(
        output.contains("java"),
        "output must list the java tool dependency"
    );
}

#[test]
fn given_the_command_name_unknown_cmd_does_not_exist_in_the_command_tree_when_the_explain_command_is_called_with_unknown_cmd_then_an_error_message_unknown_command_is_returned() {
    // Given the command name "unknown-cmd" does not exist in the command tree, when the explain command is called with "unknown-cmd", then an error message "Unknown command" is returned
    let tree = CommandTree::new();

    let error = tree
        .explain("unknown-cmd")
        .expect_err("explain should fail for an unknown command");

    assert_eq!(
        error.to_string(),
        "Unknown command",
        "error message must be exactly 'Unknown command'"
    );
}
