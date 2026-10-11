//! Wiring a new Rust source file into its crate.
//!
//! A Rust file under `src/` that no `mod` line names is not part of the
//! crate: `cargo` never compiles it, and the tests that call into it
//! fail with E0433 (`could not find ... in the crate root`). Measured on
//! this harness: three implementation attempts in a row wrote a correct
//! `src/<module>.rs`, none of them touched `lib.rs`, and every bar read
//! the same unresolved path. The model is shown the whole tree and asked
//! for the production file; the declaration is a line in a file it was
//! not asked about. So the harness writes the line.

/// The lines that declare a module at the top level of a crate root -
/// `mod x;` or `pub mod x;`, with or without attributes on the line
/// before. Inline modules (`mod tests { ... }`) are not file modules
/// and are not matched.
fn declared_module(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix("pub ")
        .map(str::trim_start)
        .unwrap_or(rest);
    let rest = rest.strip_prefix("mod ")?.trim_start();
    let name = rest.strip_suffix(';')?.trim_end();
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')).then_some(name)
}

/// `crate_root` with `pub mod {module};` added, or `None` when the root
/// already declares it.
///
/// The line goes after the last top-level module declaration, so the
/// crate's own ordering is kept. A root with no declarations yet gets
/// it after the leading `//!` doc comment and `use` block, before the
/// first item - which is where a developer would put it.
pub fn declare_rust_module(crate_root: &str, module: &str) -> Option<String> {
    let lines: Vec<&str> = crate_root.lines().collect();
    if lines
        .iter()
        .any(|line| declared_module(line) == Some(module))
    {
        return None;
    }
    let declaration = format!("pub mod {module};");
    let after_last_mod = lines
        .iter()
        .rposition(|line| declared_module(line).is_some())
        .map(|index| index + 1);
    let insert_at = after_last_mod.unwrap_or_else(|| preamble_end(&lines));
    let mut out: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
    if after_last_mod.is_none() && insert_at > 0 && !lines[insert_at - 1].trim().is_empty() {
        // A blank line between the preamble and the new declaration,
        // the way `use` blocks and items are already separated.
        out.insert(insert_at, String::new());
        out.insert(insert_at + 1, declaration);
    } else {
        out.insert(insert_at, declaration);
    }
    let mut joined = out.join("\n");
    if crate_root.ends_with('\n') || crate_root.is_empty() {
        joined.push('\n');
    }
    Some(joined)
}

/// The index of the first line after the leading `//!` comments, inner
/// attributes, `use` lines, and the blank lines between them.
fn preamble_end(lines: &[&str]) -> usize {
    let mut end = 0;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_preamble = trimmed.is_empty()
            || trimmed.starts_with("//!")
            || trimmed.starts_with("#![")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("pub use ");
        if is_preamble {
            if !trimmed.is_empty() {
                end = index + 1;
            }
        } else {
            break;
        }
    }
    end
}

/// The module name a new top-level source file declares, when it is
/// one: `src/report.rs` is `report`; `src/lib.rs`, `src/main.rs`, and
/// anything in a subdirectory are not a new top-level module.
pub fn top_level_module<'a>(path: &'a str, production_root: &str) -> Option<&'a str> {
    let root = production_root.trim_end_matches('/');
    let relative = path.strip_prefix(root)?.strip_prefix('/')?;
    if relative.contains('/') {
        return None;
    }
    let stem = relative.strip_suffix(".rs")?;
    (!matches!(stem, "lib" | "main" | "mod")).then_some(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_is_declared_after_the_last_existing_one() {
        let root = "//! The crate.\n\npub mod adapters;\npub mod domain;\n\npub use domain::x;\n";
        let out = declare_rust_module(root, "report").unwrap();
        assert_eq!(
            out,
            "//! The crate.\n\npub mod adapters;\npub mod domain;\npub mod report;\n\npub use domain::x;\n"
        );
    }

    #[test]
    fn a_root_without_declarations_gets_one_after_the_preamble() {
        let root = "//! Docs.\nuse std::fmt;\n\npub fn add(a: u32, b: u32) -> u32 { a + b }\n";
        let out = declare_rust_module(root, "report").unwrap();
        assert_eq!(
            out,
            "//! Docs.\nuse std::fmt;\n\npub mod report;\n\npub fn add(a: u32, b: u32) -> u32 { a + b }\n"
        );
    }

    #[test]
    fn an_empty_root_gets_just_the_line() {
        assert_eq!(
            declare_rust_module("", "report").unwrap(),
            "pub mod report;\n"
        );
    }

    #[test]
    fn an_already_declared_module_is_left_alone() {
        assert_eq!(declare_rust_module("mod report;\n", "report"), None);
        assert_eq!(declare_rust_module("pub mod report;\n", "report"), None);
        assert_eq!(declare_rust_module("pub  mod  report ;\n", "report"), None);
    }

    #[test]
    fn an_inline_test_module_is_not_a_declaration_to_follow() {
        let root = "pub fn add() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n";
        let out = declare_rust_module(root, "report").unwrap();
        assert!(out.starts_with("pub mod report;\npub fn add() {}"), "{out}");
        assert!(
            out.ends_with("mod tests {\n    use super::*;\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn only_a_new_file_directly_under_the_root_is_a_top_level_module() {
        assert_eq!(top_level_module("src/report.rs", "src"), Some("report"));
        assert_eq!(top_level_module("src/report.rs", "src/"), Some("report"));
        assert_eq!(top_level_module("src/lib.rs", "src"), None);
        assert_eq!(top_level_module("src/main.rs", "src"), None);
        assert_eq!(top_level_module("src/domain/report.rs", "src"), None);
        assert_eq!(top_level_module("tests/report.rs", "src"), None);
        assert_eq!(top_level_module("src/notes.txt", "src"), None);
    }
}
