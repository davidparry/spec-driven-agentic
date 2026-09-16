//! Project-relative path jail, shared by staging writes and `command_run`
//! argv policy. Pure string/component logic: no filesystem, no `unsafe`.

use std::fmt;

/// Why a path is refused as a project-relative location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathJail {
    Empty,
    Absolute,
    Home,
    Escape,
}

impl fmt::Display for PathJail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the path is empty."),
            Self::Absolute => f.write_str(
                "absolute paths are not allowed — everything happens inside the project root.",
            ),
            Self::Home => f.write_str(
                "home-directory paths are not allowed — everything happens inside the project root.",
            ),
            Self::Escape => f.write_str("'..' could reach outside the project root."),
        }
    }
}

impl std::error::Error for PathJail {}

/// Normalize a project-relative path to `/`-separated components.
///
/// Refuses empty input, home (`~…`), Windows drive letters (`C:`),
/// leading `/` or `\`, and `..` that would leave the project root.
/// `.` is skipped; `a/../b` becomes `b`.
pub fn confine(path: &str) -> Result<String, PathJail> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(PathJail::Empty);
    }
    if trimmed.starts_with('~') {
        return Err(PathJail::Home);
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(PathJail::Absolute);
    }
    let absolute = trimmed.starts_with('/') || trimmed.starts_with('\\');
    let mut parts: Vec<&str> = Vec::new();
    for part in trimmed.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if parts.pop().is_none() {
                return Err(PathJail::Escape);
            }
            continue;
        }
        parts.push(part);
    }
    if absolute {
        return Err(PathJail::Absolute);
    }
    if parts.is_empty() {
        return Err(PathJail::Empty);
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_relative_paths_normalize() {
        assert_eq!(confine("features/x.feature").unwrap(), "features/x.feature");
        assert_eq!(confine("./src/Foo.java").unwrap(), "src/Foo.java");
        assert_eq!(confine("features/../src/Foo.java").unwrap(), "src/Foo.java");
        assert_eq!(
            confine(r"src\main\Kata.java").unwrap(),
            "src/main/Kata.java"
        );
        assert_eq!(confine("foo...bar").unwrap(), "foo...bar");
        assert_eq!(confine("--release").unwrap(), "--release");
    }

    #[test]
    fn absolute_unix_windows_and_home_are_refused() {
        assert_eq!(confine("/etc/passwd"), Err(PathJail::Absolute));
        assert_eq!(confine(r"\Windows\System32"), Err(PathJail::Absolute));
        assert_eq!(confine(r"C:\Windows"), Err(PathJail::Absolute));
        assert_eq!(confine("C:/Windows"), Err(PathJail::Absolute));
        assert_eq!(confine("~/code"), Err(PathJail::Home));
        assert_eq!(confine(""), Err(PathJail::Empty));
        assert_eq!(confine("."), Err(PathJail::Empty));
        assert_eq!(confine(".."), Err(PathJail::Escape));
        assert_eq!(confine("../outside.json"), Err(PathJail::Escape));
        assert_eq!(confine("a/../../b"), Err(PathJail::Escape));
    }
}
