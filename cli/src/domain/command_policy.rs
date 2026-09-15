//! The guardrail policy for the `command_run` MCP tool. Pure argv
//! validation: the program must come from a fixed allowlist of dev
//! tools, no argument may name an absolute path or step outside the
//! project root with `..`, and known eval-escape flags of the allowed
//! tools are refused. Commands are executed as argv directly — never
//! through a shell — so `;`, `&&`, `|`, globs, and redirection are
//! inert text and need no policy here.
//!
//! This is policy-level guardrailing, not an OS sandbox: an allowed
//! build tool can still run build scripts. What the policy makes
//! unexpressible is naming anything outside the root and running
//! destructive binaries (`rm`, `sudo`, `sh`, ...).

/// The dev tools an agent may run during the implementation phase.
pub const ALLOWED_PROGRAMS: &[&str] = &[
    "cargo", "mvn", "npm", "npx", "node", "dotnet", "java", "javac", "tsc",
];

/// A refused command: the violated rule, spelled out for the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRefusal(pub String);

impl std::fmt::Display for CommandRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CommandRefusal {}

/// Flags and subcommands that turn an allowed tool into an arbitrary
/// code or shell escape, refused per program.
fn eval_escapes(program: &str) -> &'static [&'static str] {
    match program {
        "node" => &["-e", "--eval", "-p", "--print"],
        "npx" => &["-c", "--call"],
        "npm" => &["exec", "x"],
        _ => &[],
    }
}

/// Validate one argv against the guardrails. `Ok` means the command is
/// safe to spawn with the project root as its working directory.
pub fn validate(argv: &[String]) -> Result<(), CommandRefusal> {
    let Some(program) = argv.first() else {
        return Err(CommandRefusal("The command is empty.".into()));
    };
    if program.contains('/') || program.contains('\\') {
        return Err(CommandRefusal(format!(
            "'{program}' is refused: the program must be a bare name resolved \
             via PATH, never a path.",
        )));
    }
    if !ALLOWED_PROGRAMS.contains(&program.as_str()) {
        return Err(CommandRefusal(format!(
            "'{program}' is not on the allowlist. Allowed programs: {}.",
            ALLOWED_PROGRAMS.join(", "),
        )));
    }
    let escapes = eval_escapes(program);
    for argument in &argv[1..] {
        if escapes.contains(&argument.as_str()) {
            return Err(CommandRefusal(format!(
                "'{program} {argument}' is refused: it can execute arbitrary code \
                 outside the guardrails.",
            )));
        }
        if program == "mvn" && argument.starts_with("exec:") {
            return Err(CommandRefusal(format!(
                "'{argument}' is refused: the Maven exec plugin can run arbitrary \
                 programs outside the guardrails.",
            )));
        }
        // Path jail: the process runs inside the project root, and no
        // argument may point outside it. `=`-joined flag values count.
        // `foo...bar` is a Normal path component, not a parent hop.
        for piece in argument.split('=') {
            if piece.is_empty() {
                continue;
            }
            if let Err(reason) = crate::domain::paths::confine(piece) {
                return Err(CommandRefusal(format!("'{argument}' is refused: {reason}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn ordinary_build_and_test_commands_are_allowed() {
        for command in [
            vec!["cargo", "test"],
            vec!["cargo", "build", "--release"],
            vec!["mvn", "-q", "-B", "compile"],
            vec!["npm", "install"],
            vec!["npx", "cucumber-js", "--format", "json"],
            vec!["node", "--version"],
            vec!["dotnet", "test"],
            vec!["javac", "src/main/java/Kata.java"],
            vec!["tsc", "--noEmit"],
        ] {
            assert_eq!(validate(&argv(&command)), Ok(()), "refused: {command:?}");
        }
    }

    #[test]
    fn an_empty_command_is_refused() {
        assert_eq!(
            validate(&[]).unwrap_err(),
            CommandRefusal("The command is empty.".into())
        );
    }

    #[test]
    fn programs_off_the_allowlist_are_refused() {
        for program in ["rm", "sudo", "sh", "bash", "curl", "git", "python"] {
            let error = validate(&argv(&[program, "-rf", "."])).unwrap_err();
            assert!(
                error.0.contains("not on the allowlist"),
                "{program}: {}",
                error.0
            );
            assert!(error.0.contains("cargo"), "the refusal names the allowlist");
        }
    }

    #[test]
    fn a_path_qualified_program_is_refused() {
        for program in ["./script.sh", "/bin/rm", "bin/cargo", r"tools\evil.exe"] {
            let error = validate(&argv(&[program])).unwrap_err();
            assert!(
                error.0.contains("bare name"),
                "{program} should be refused as a path: {}",
                error.0
            );
        }
    }

    #[test]
    fn eval_escape_flags_are_refused_per_program() {
        for command in [
            vec!["node", "-e", "require('fs').rmSync('.', {recursive: true})"],
            vec!["node", "--eval", "1"],
            vec!["node", "-p", "1"],
            vec!["node", "--print", "1"],
            vec!["npx", "-c", "rm -rf ."],
            vec!["npx", "--call", "rm -rf ."],
            vec!["npm", "exec", "rimraf", "."],
            vec!["npm", "x", "rimraf"],
        ] {
            let error = validate(&argv(&command)).unwrap_err();
            assert!(
                error.0.contains("arbitrary code"),
                "{command:?}: {}",
                error.0
            );
        }
    }

    #[test]
    fn maven_exec_goals_are_refused() {
        let error = validate(&argv(&["mvn", "exec:exec", "-Dexec.executable=rm"])).unwrap_err();
        assert!(error.0.contains("Maven exec plugin"), "got: {}", error.0);
        let error = validate(&argv(&["mvn", "exec:java"])).unwrap_err();
        assert!(error.0.contains("Maven exec plugin"), "got: {}", error.0);
    }

    #[test]
    fn absolute_path_arguments_are_refused() {
        for command in [
            vec!["cargo", "build", "--manifest-path", "/etc/Cargo.toml"],
            vec!["cargo", "build", "--manifest-path=/etc/Cargo.toml"],
            vec!["javac", "/etc/passwd"],
            vec!["dotnet", "build", r"C:\proj\app.csproj"],
            vec!["mvn", "-f", "C:/other/pom.xml"],
        ] {
            let error = validate(&argv(&command)).unwrap_err();
            assert!(
                error.0.contains("absolute paths") || error.0.contains("home-directory"),
                "{command:?}: {}",
                error.0
            );
        }
        let home = validate(&argv(&["tsc", "~/code/index.ts"])).unwrap_err();
        assert!(
            home.0.contains("home-directory"),
            "tilde home path: {}",
            home.0
        );
    }

    #[test]
    fn parent_directory_traversal_is_refused() {
        for command in [
            vec!["cargo", "build", "--manifest-path", "../other/Cargo.toml"],
            vec!["cargo", "build", "--manifest-path=.."],
            vec!["javac", "..\\..\\evil.java"],
        ] {
            let error = validate(&argv(&command)).unwrap_err();
            assert!(error.0.contains(".."), "{command:?}: {}", error.0);
        }
    }

    #[test]
    fn shell_metacharacters_are_inert_text_not_a_policy_matter() {
        // No shell ever interprets the argv, so an argument like
        // "; rm -rf /" reaches the allowed program as a literal string.
        // The policy only refuses what could still act: the path jail.
        assert_eq!(validate(&argv(&["cargo", "test", "a;b|c&&d"])), Ok(()));
        assert_eq!(
            validate(&argv(&["cargo", "test", "foo...bar"])),
            Ok(()),
            "ellipsis is not a parent-directory hop"
        );
    }
}
