//! Branch names: the one the developer types, cleaned up, and the one
//! generated for them when they just press Enter.

/// The prefix every branch the harness offers carries, so a developer
/// scanning `git branch` can tell which ones a run made.
pub const BRANCH_PREFIX: &str = "spec/";

/// A typed branch name as git will accept it, or why it cannot be one.
///
/// Branch names reach git as a ref, and a ref has rules: no spaces, no
/// `..`, no leading or trailing `/`, none of `~^:?*[\`. Rather than
/// bouncing a developer who typed "add newline support", the obvious
/// separators are folded to `-` and the rest is refused.
pub fn clean_branch_name(typed: &str) -> Result<String, String> {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        return Err("a branch name cannot be empty".into());
    }
    let folded: String = trimmed
        .chars()
        .map(|c| {
            if c.is_whitespace() || c == '_' {
                '-'
            } else {
                c
            }
        })
        .collect();
    // Checked before the prefix as well as after, or `-x` would pass
    // as `spec/-x` and git would refuse the segment.
    if let Some(reason) = unusable(&folded) {
        return Err(reason);
    }
    let name = prefixed(&folded);
    if let Some(reason) = unusable(&name) {
        return Err(reason);
    }
    Ok(name)
}

/// `name` under [`BRANCH_PREFIX`], unless the developer already typed a
/// prefix of their own - someone who asks for `feature/x` means it.
fn prefixed(name: &str) -> String {
    if name.contains('/') {
        return name.to_string();
    }
    format!("{BRANCH_PREFIX}{name}")
}

/// Why git would refuse this ref, or `None` when it would take it.
fn unusable(name: &str) -> Option<String> {
    const FORBIDDEN: [char; 8] = ['~', '^', ':', '?', '*', '[', '\\', ' '];
    if let Some(bad) = name
        .chars()
        .find(|c| FORBIDDEN.contains(c) || c.is_control())
    {
        return Some(format!("a branch name cannot contain '{bad}'"));
    }
    if name.contains("..") {
        return Some("a branch name cannot contain '..'".into());
    }
    if name.contains("//") {
        return Some("a branch name cannot contain '//'".into());
    }
    if name.ends_with('/') || name.ends_with('.') || name.ends_with(".lock") {
        return Some(format!(
            "a branch name cannot end with '{}'",
            trailing(name)
        ));
    }
    if name.starts_with('-') || name.starts_with('/') {
        return Some("a branch name cannot start with '-' or '/'".into());
    }
    None
}

/// The trailing bit a name was refused for, for the message.
fn trailing(name: &str) -> &str {
    if name.ends_with(".lock") {
        ".lock"
    } else if name.ends_with('/') {
        "/"
    } else {
        "."
    }
}

/// A branch name for a developer who pressed Enter: the prefix, the
/// day, and `seed` rendered short so two runs on one day do not collide.
///
/// Pure, so the test can say what it will produce: the caller supplies
/// both the date and the seed. Nothing about the name needs to be
/// unguessable - it needs to be unique enough and readable in `git
/// branch`.
pub fn generated_branch_name(today: &str, seed: u64) -> String {
    format!("{BRANCH_PREFIX}{today}-{}", base36(seed))
}

/// `seed` in lowercase base36, six characters, so the name stays short
/// enough to read.
fn base36(seed: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut n = seed;
    let mut out = [b'0'; 6];
    for slot in out.iter_mut().rev() {
        *slot = DIGITS[(n % 36) as usize];
        n /= 36;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_gets_the_harness_prefix() {
        assert_eq!(clean_branch_name("newlines").unwrap(), "spec/newlines");
    }

    /// Nobody types a branch name the way git wants it on the first go.
    #[test]
    fn spaces_and_underscores_become_dashes() {
        assert_eq!(
            clean_branch_name("  add newline support ").unwrap(),
            "spec/add-newline-support"
        );
        assert_eq!(
            clean_branch_name("add_newline_support").unwrap(),
            "spec/add-newline-support"
        );
    }

    /// A developer who typed their own prefix meant it.
    #[test]
    fn a_name_that_already_has_a_prefix_keeps_it() {
        assert_eq!(clean_branch_name("feature/x").unwrap(), "feature/x");
    }

    #[test]
    fn a_name_git_would_refuse_is_refused_here_with_the_reason() {
        for (typed, expected) in [
            ("", "cannot be empty"),
            ("a..b", "'..'"),
            ("a~b", "'~'"),
            ("a:b", "':'"),
            ("a?b", "'?'"),
            ("x/", "'/'"),
            ("x.lock", "'.lock'"),
            ("-x", "'-' or '/'"),
            ("a//b", "'//'"),
        ] {
            let error = clean_branch_name(typed).unwrap_err();
            assert!(error.contains(expected), "{typed}: {error}");
        }
    }

    #[test]
    fn a_generated_name_is_the_prefix_the_day_and_the_seed() {
        assert_eq!(
            generated_branch_name("2026-10-05", 0),
            "spec/2026-10-05-000000"
        );
        assert_ne!(
            generated_branch_name("2026-10-05", 1),
            generated_branch_name("2026-10-05", 2)
        );
    }

    /// Whatever the generator produces has to survive the same check a
    /// typed name does, or pressing Enter would be the one path that
    /// hands git something it refuses.
    #[test]
    fn a_generated_name_is_always_a_usable_ref() {
        for seed in [0, 1, 35, 36, u64::MAX, 7_777_777] {
            let name = generated_branch_name("2026-10-05", seed);
            assert_eq!(unusable(&name), None, "{name}");
            assert_eq!(clean_branch_name(&name).unwrap(), name);
        }
    }
}
