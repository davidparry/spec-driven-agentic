//! Staging overlays: feature catalogs and source trees that see
//! `.spec/staged/` as if it were already committed. `spec changes validate`
//! already overlays Gherkin; implement, status, and generation need
//! the same view so a just-staged scenario or test counts as present.

use crate::domain::feature::{self, FeatureDoc, FeatureSummary};
use crate::ports::{
    ChangeStore, FeatureCatalog, FeatureError, SourceError, SourceFile, SourceFiles,
};

pub struct OverlayCatalog<F, C> {
    inner: F,
    store: C,
}

impl<F: FeatureCatalog, C: ChangeStore> OverlayCatalog<F, C> {
    pub fn new(inner: F, store: C) -> Self {
        Self { inner, store }
    }
}

impl<F: FeatureCatalog, C: ChangeStore> FeatureCatalog for OverlayCatalog<F, C> {
    fn list(&self) -> Result<Vec<FeatureSummary>, FeatureError> {
        let mut summaries = self.inner.list()?;
        let changes = self.store.changes()?;
        for change in changes.into_iter().filter(|c| c.path.ends_with(".feature")) {
            let Some(content) = self.store.content(&change.path)? else {
                continue;
            };
            let doc = feature::parse(&change.path, &content).map_err(FeatureError)?;
            if let Some(existing) = summaries.iter_mut().find(|s| s.path == change.path) {
                *existing = doc.summary();
            } else {
                summaries.push(doc.summary());
            }
        }
        Ok(summaries)
    }

    fn read(&self, path: &str) -> Result<FeatureDoc, FeatureError> {
        match self.store.content(path)? {
            Some(content) => feature::parse(path, &content).map_err(FeatureError),
            None => self.inner.read(path),
        }
    }

    fn exists(&self, path: &str) -> bool {
        match self.store.content(path) {
            Ok(Some(_)) => true,
            Ok(None) => self.inner.exists(path),
            Err(error) => {
                tracing::error!(error = %error, path, "staging overlay unreadable");
                false
            }
        }
    }
}

impl<F: FeatureCatalog, C: ChangeStore> crate::ports::FeatureFiles for OverlayCatalog<F, C> {
    fn exists(&self, path: &str) -> bool {
        FeatureCatalog::exists(self, path)
    }

    fn has_tag(&self, path: &str, tag: &str) -> bool {
        match self.store.content(path) {
            Ok(Some(content)) => feature::parse(path, &content)
                .map(|doc| doc.all_tags().iter().any(|t| t == tag))
                .unwrap_or(false),
            Ok(None) => self
                .inner
                .read(path)
                .map(|doc| doc.all_tags().iter().any(|t| t == tag))
                .unwrap_or(false),
            Err(error) => {
                tracing::error!(error = %error, path, "staging overlay unreadable");
                false
            }
        }
    }
}

pub struct OverlaySources<S, C> {
    inner: S,
    store: C,
}

impl<S: SourceFiles, C: ChangeStore> OverlaySources<S, C> {
    pub fn new(inner: S, store: C) -> Self {
        Self { inner, store }
    }
}

impl<S: SourceFiles, C: ChangeStore> SourceFiles for OverlaySources<S, C> {
    fn sources(&self, extension: &str) -> Result<Vec<SourceFile>, SourceError> {
        let mut files = self.inner.sources(extension)?;
        let suffix = format!(".{extension}");
        let changes = self.store.changes()?;
        for change in changes.into_iter().filter(|c| c.path.ends_with(&suffix)) {
            let Some(content) = self.store.content(&change.path)? else {
                continue;
            };
            if let Some(existing) = files.iter_mut().find(|f| f.path == change.path) {
                existing.content = content;
            } else {
                files.push(SourceFile {
                    path: change.path,
                    content,
                });
            }
        }
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{InMemoryChangeStore, InMemoryFeatureCatalog};
    use std::collections::HashMap;

    #[test]
    fn a_staged_feature_is_listed_and_readable() {
        let store = InMemoryChangeStore::default();
        store
            .stage(
                "features/new.feature",
                "Feature: New\n\n  @REQ-003\n  Scenario: s\n    Given a calculator\n",
                "add",
            )
            .unwrap();
        let overlay = OverlayCatalog::new(
            InMemoryFeatureCatalog {
                files: HashMap::new(),
            },
            store,
        );
        let list = overlay.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, "features/new.feature");
        assert!(overlay.exists("features/new.feature"));
        assert!(
            overlay
                .read("features/new.feature")
                .unwrap()
                .all_tags()
                .iter()
                .any(|t| t == "@REQ-003")
        );
    }

    #[test]
    fn a_staged_source_overrides_the_working_tree() {
        let store = InMemoryChangeStore::default();
        store
            .stage(
                "src/test/java/StringCalculatorTest.java",
                "@DisplayName(\"REQ-003\") @Test void two() {}",
                "generate",
            )
            .unwrap();
        let overlay = OverlaySources::new(
            crate::test_support::FakeSources(vec![crate::ports::SourceFile {
                path: "src/test/java/StringCalculatorTest.java".into(),
                content: "class StringCalculatorTest {}".into(),
            }]),
            store,
        );
        let files = overlay.sources("java").unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].content.contains("REQ-003"));
    }

    #[test]
    fn a_staged_feature_replaces_the_working_tree_listing() {
        let store = InMemoryChangeStore::default();
        store
            .stage(
                "features/old.feature",
                "Feature: New name\n\n  Scenario: t\n    Given y\n",
                "edit",
            )
            .unwrap();
        let overlay = OverlayCatalog::new(
            InMemoryFeatureCatalog {
                files: [(
                    "features/old.feature".into(),
                    "Feature: Old\n\n  Scenario: s\n    Given x\n".into(),
                )]
                .into_iter()
                .collect(),
            },
            store,
        );
        let list = overlay.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "New name");
    }

    #[test]
    fn read_and_exists_fall_through_to_the_working_tree() {
        let overlay = OverlayCatalog::new(
            InMemoryFeatureCatalog {
                files: [(
                    "features/on-disk.feature".into(),
                    "Feature: Disk\n\n  Scenario: s\n    Given x\n".into(),
                )]
                .into_iter()
                .collect(),
            },
            InMemoryChangeStore::default(),
        );
        assert!(overlay.exists("features/on-disk.feature"));
        assert_eq!(
            overlay.read("features/on-disk.feature").unwrap().name,
            "Disk"
        );
        assert!(!overlay.exists("features/missing.feature"));
    }

    #[test]
    fn a_listed_feature_with_no_content_is_skipped() {
        let overlay = OverlayCatalog::new(
            InMemoryFeatureCatalog {
                files: HashMap::new(),
            },
            GhostStore {
                listed: vec!["features/ghost.feature".into()],
                fail_content: false,
            },
        );
        assert!(overlay.list().unwrap().is_empty());
    }

    #[test]
    fn a_new_staged_source_is_appended() {
        let store = InMemoryChangeStore::default();
        store
            .stage("src/Foo.java", "class Foo {}", "generate")
            .unwrap();
        let overlay = OverlaySources::new(crate::test_support::FakeSources(vec![]), store);
        let files = overlay.sources("java").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/Foo.java");
    }

    #[test]
    fn a_listed_source_with_no_content_is_skipped() {
        let overlay = OverlaySources::new(
            crate::test_support::FakeSources(vec![]),
            GhostStore {
                listed: vec!["src/Ghost.java".into()],
                fail_content: false,
            },
        );
        assert!(overlay.sources("java").unwrap().is_empty());
    }

    #[test]
    fn a_failing_store_surfaces_as_a_feature_error() {
        let overlay = OverlayCatalog::new(
            InMemoryFeatureCatalog {
                files: HashMap::new(),
            },
            InMemoryChangeStore::failing("disk full"),
        );
        assert!(overlay.list().unwrap_err().0.contains("disk full"));
    }

    #[test]
    fn a_failing_content_read_surfaces_as_a_source_error() {
        let overlay = OverlaySources::new(
            crate::test_support::FakeSources(vec![]),
            GhostStore {
                listed: vec!["src/Foo.java".into()],
                fail_content: true,
            },
        );
        assert!(
            overlay
                .sources("java")
                .unwrap_err()
                .0
                .contains("cannot read")
        );
    }

    #[test]
    fn a_failing_content_read_surfaces_as_a_feature_error() {
        let overlay = OverlayCatalog::new(
            InMemoryFeatureCatalog {
                files: HashMap::new(),
            },
            GhostStore {
                listed: vec!["features/x.feature".into()],
                fail_content: true,
            },
        );
        assert!(
            overlay
                .read("features/x.feature")
                .unwrap_err()
                .0
                .contains("cannot read")
        );
        assert!(!overlay.exists("features/x.feature"));
    }

    struct GhostStore {
        listed: Vec<String>,
        fail_content: bool,
    }

    impl crate::ports::ChangeStore for GhostStore {
        fn stage(
            &self,
            _path: &str,
            _content: &str,
            _summary: &str,
        ) -> Result<crate::ports::StagedChange, crate::ports::StageError> {
            unimplemented!("ghost store is read-only")
        }

        fn changes(&self) -> Result<Vec<crate::ports::StagedChange>, crate::ports::StageError> {
            Ok(self
                .listed
                .iter()
                .map(|path| crate::ports::StagedChange {
                    path: path.clone(),
                    action: "create".into(),
                    summary: String::new(),
                })
                .collect())
        }

        fn content(&self, _path: &str) -> Result<Option<String>, crate::ports::StageError> {
            if self.fail_content {
                Err(crate::ports::StageError("cannot read".into()))
            } else {
                Ok(None)
            }
        }

        fn commit(&self) -> Result<Vec<crate::ports::StagedChange>, crate::ports::StageError> {
            Ok(vec![])
        }

        fn discard(&self) -> Result<Vec<crate::ports::StagedChange>, crate::ports::StageError> {
            Ok(vec![])
        }
    }
}
