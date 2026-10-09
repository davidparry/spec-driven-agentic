//! The shape of a requirement id, and where the next one comes from.
//!
//! Every id in a catalog carries a prefix: `REQ-003` in the kata,
//! `HARNESS-014` in this crate's own spec, `CLI-009` in the Java smoke
//! test. Nothing configures that prefix — it is read back off the
//! catalog the id is being drafted into, so a spec keeps the convention
//! it already has instead of the one the tool was compiled with.

use regex::Regex;
use std::sync::LazyLock;

use crate::domain::model::SpecCatalog;

/// An uppercase prefix, a dash, and a number. The validator rejects an
/// id that does not read this way, and the CLI uses it to tell an id
/// from a description.
static ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9]*-\d+$").expect("valid regex"));

/// The prefix used when a catalog holds nothing to read one from.
pub const DEFAULT_PREFIX: &str = "REQ";

/// The narrowest id the padding ever produces, so a fresh catalog
/// starts at `REQ-001` rather than `REQ-1`.
const MIN_WIDTH: usize = 3;

/// Does `text` read as a requirement id? Case-sensitive: ids are
/// uppercase, and the validator reports a lowercase one as malformed.
pub fn is_id_shape(text: &str) -> bool {
    ID.is_match(text)
}

/// Split an id into its prefix and its number, or `None` when it is not
/// shaped like one.
pub fn split_id(id: &str) -> Option<(&str, &str)> {
    if !is_id_shape(id) {
        return None;
    }
    id.rfind('-').map(|cut| (&id[..cut], &id[cut + 1..]))
}

/// The prefix new requirements in `target_file` should carry.
///
/// Nearest first, because a catalog split across files may well use a
/// different prefix per file: the target's own requirements, then its
/// ancestors up the include tree, then anything else in the catalog in
/// catalog order, then [`DEFAULT_PREFIX`]. Each step takes the first
/// requirement it finds rather than the most common one, so the answer
/// does not move when a file grows.
pub fn prefix_for(catalog: &SpecCatalog, target_file: &str) -> String {
    let from = |path: &str| {
        catalog.file(path).and_then(|file| {
            file.spec
                .requirements
                .iter()
                .find_map(|r| split_id(&r.id).map(|(prefix, _)| prefix.to_string()))
        })
    };
    if let Some(prefix) = from(target_file) {
        return prefix;
    }
    let mut ancestor = catalog.parent_of(target_file);
    while let Some(path) = ancestor {
        if let Some(prefix) = from(path) {
            return prefix;
        }
        ancestor = catalog.parent_of(path);
    }
    catalog
        .files()
        .iter()
        .find_map(|file| {
            file.spec
                .requirements
                .iter()
                .find_map(|r| split_id(&r.id).map(|(prefix, _)| prefix.to_string()))
        })
        .unwrap_or_else(|| DEFAULT_PREFIX.to_string())
}

/// The next free id for `target_file`: the catalog's own prefix, and one
/// past the highest number already carrying it.
///
/// The scan is over the whole catalog rather than the target file, so
/// two files sharing a prefix never hand out the same id. Width follows
/// the ids already there, so a catalog that numbered past `REQ-999`
/// keeps four digits instead of dropping back to three.
pub fn next_id(catalog: &SpecCatalog, target_file: &str) -> String {
    let prefix = prefix_for(catalog, target_file);
    let (max, width) = catalog
        .files()
        .iter()
        .flat_map(|file| file.spec.requirements.iter())
        .filter_map(|r| split_id(&r.id))
        .filter(|(found, _)| *found == prefix)
        .filter_map(|(_, digits)| digits.parse::<u32>().ok().map(|n| (n, digits.len())))
        .fold((0, MIN_WIDTH), |(max, width), (number, digits)| {
            (max.max(number), width.max(digits))
        });
    format!("{prefix}-{next:0width$}", next = max + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{Requirement, Spec, SpecCatalog};

    fn requirement(id: &str) -> Requirement {
        Requirement {
            id: id.into(),
            title: "t".into(),
            status: "pending".into(),
            story: "s".into(),
            acceptance_criteria: Vec::new(),
            feature_file: None,
            ..Default::default()
        }
    }

    fn spec(ids: &[&str]) -> Spec {
        Spec {
            requirements: ids.iter().map(|id| requirement(id)).collect(),
            ..Spec::default()
        }
    }

    #[test]
    fn an_empty_catalog_starts_at_the_default_prefix() {
        let catalog = SpecCatalog::single_root(Spec::default());
        assert_eq!(next_id(&catalog, "requirements.json"), "REQ-001");
    }

    #[test]
    fn the_prefix_is_read_off_the_catalog_rather_than_assumed() {
        let catalog = SpecCatalog::single_root(spec(&["HARNESS-001", "HARNESS-014"]));
        assert_eq!(next_id(&catalog, "requirements.json"), "HARNESS-015");
    }

    #[test]
    fn numbering_follows_the_widest_id_already_there() {
        let catalog = SpecCatalog::single_root(spec(&["REQ-0007"]));
        assert_eq!(next_id(&catalog, "requirements.json"), "REQ-0008");
    }

    #[test]
    fn an_id_that_is_not_shaped_like_one_does_not_count() {
        let catalog = SpecCatalog::single_root(spec(&["req-1", "NOT AN ID", "REQ-002"]));
        assert_eq!(next_id(&catalog, "requirements.json"), "REQ-003");
    }

    #[test]
    fn a_foreign_prefix_does_not_raise_the_number() {
        let catalog = SpecCatalog::single_root(spec(&["HARNESS-001", "CLI-099"]));
        assert_eq!(next_id(&catalog, "requirements.json"), "HARNESS-002");
    }

    #[test]
    fn split_id_separates_the_prefix_from_the_number() {
        assert_eq!(split_id("HARNESS-014"), Some(("HARNESS", "014")));
        assert_eq!(split_id("REQ-3"), Some(("REQ", "3")));
        assert_eq!(split_id("req-3"), None);
        assert_eq!(split_id("REQ-"), None);
    }

    #[test]
    fn the_shape_is_an_uppercase_prefix_a_dash_and_digits() {
        for id in ["REQ-001", "HARNESS-14", "CLI-9", "A1-2"] {
            assert!(is_id_shape(id), "{id} should read as an id");
        }
        for not_id in ["req-1", "REQ", "REQ-", "REQ-1a", "-1", "1-1"] {
            assert!(!is_id_shape(not_id), "{not_id} should not read as an id");
        }
    }
}
