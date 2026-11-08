//! Addon sources (GitHub, WoWInterface, ...) behind one interface, and the registry that
//! picks the right one for a URL or a tracked addon.
//!
//! Sources are synchronous: callers run them on a background thread.

pub mod github;
#[cfg(test)]
pub mod mock;
pub mod select;

pub use select::{Flavor, Target};

use std::path::{Path, PathBuf};

use crate::http::HttpError;
use crate::state::Channel;

/// A place addons come from.
pub trait Source: Send + Sync {
    /// Stable id stored in the tracking file, e.g. `github`.
    fn id(&self) -> &'static str;

    /// Name shown in the UI, e.g. `GitHub`.
    fn name(&self) -> &'static str;

    /// Returns the project id if `url` points at an addon on this source.
    fn parse_url(&self, url: &str) -> Option<String>;

    /// The newest release on `channel` (or a more stable one) with a file for `target`.
    fn latest(
        &self,
        project: &str,
        target: &Target,
        channel: Channel,
    ) -> Result<Release, SourceError>;

    /// Downloads the release's file into `into_dir` and returns its path.
    fn download(&self, release: &Release, into_dir: &Path) -> Result<PathBuf, SourceError>;

    fn search(&self, _query: &str) -> Result<Vec<SearchHit>, SourceError> {
        Err(SourceError::Unsupported(format!(
            "{} does not support search",
            self.name()
        )))
    }

    /// Whether the source can't be used until an API key is set.
    fn needs_key(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The release's version or tag, e.g. `v427.8`.
    pub version: String,
    pub channel: Channel,
    /// The file picked for the client the release was looked up for.
    pub file: ReleaseFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFile {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub project: String,
    pub name: String,
}

/// An addon on a specific source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRef {
    pub source: &'static str,
    pub project: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("{0} was not found")]
    NotFound(String),
    #[error("{0} has no release on this channel")]
    NoRelease(String),
    #[error("{0} has no file for this game version")]
    NoFileForClient(String),
    #[error("{0} needs an API key")]
    NeedsKey(String),
    #[error("{0}")]
    Unsupported(String),
    /// The message says when to try again.
    #[error("{0}")]
    RateLimited(String),
    /// The source rejected the stored token or API key.
    #[error("{0}")]
    BadToken(String),
    #[error("request failed: {0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<HttpError> for SourceError {
    fn from(error: HttpError) -> Self {
        match error {
            HttpError::Io(e) => Self::Io(e),
            other => Self::Http(other.to_string()),
        }
    }
}

/// All known sources, in the order they are tried when resolving a URL.
pub struct Registry {
    sources: Vec<Box<dyn Source>>,
}

impl Registry {
    /// Panics if two sources share an id: that is a programming error.
    pub fn new(sources: Vec<Box<dyn Source>>) -> Self {
        for (i, source) in sources.iter().enumerate() {
            assert!(
                sources[..i].iter().all(|s| s.id() != source.id()),
                "duplicate source id {:?}",
                source.id()
            );
        }
        Self { sources }
    }

    pub fn get(&self, id: &str) -> Option<&dyn Source> {
        self.sources
            .iter()
            .find(|s| s.id() == id)
            .map(|s| s.as_ref())
    }

    pub fn sources(&self) -> impl Iterator<Item = &dyn Source> {
        self.sources.iter().map(|s| s.as_ref())
    }

    /// Finds the first source that recognises `input` as one of its addon URLs.
    pub fn resolve_url(&self, input: &str) -> Option<ProjectRef> {
        let url = input.trim();
        if url.is_empty() {
            return None;
        }
        self.sources.iter().find_map(|s| {
            s.parse_url(url).map(|project| ProjectRef {
                source: s.id(),
                project,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockSource;
    use super::*;

    fn registry() -> Registry {
        Registry::new(vec![
            Box::new(MockSource::new("first", "mock.test")),
            Box::new(MockSource::new("second", "other.test")),
            // Also claims mock.test URLs, but comes later so never wins.
            Box::new(MockSource::new("third", "mock.test")),
        ])
    }

    #[test]
    fn gets_sources_by_id() {
        let registry = registry();
        assert_eq!(registry.get("second").map(|s| s.id()), Some("second"));
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn lists_sources_in_registration_order() {
        let ids: Vec<_> = registry().sources().map(|s| s.id()).collect();
        assert_eq!(ids, ["first", "second", "third"]);
    }

    #[test]
    fn resolves_urls_to_the_first_matching_source() {
        let registry = registry();
        assert_eq!(
            registry.resolve_url("  https://mock.test/BigWigs \n"),
            Some(ProjectRef {
                source: "first",
                project: "BigWigs".into()
            })
        );
        assert_eq!(
            registry.resolve_url("https://other.test/WeakAuras"),
            Some(ProjectRef {
                source: "second",
                project: "WeakAuras".into()
            })
        );
    }

    #[test]
    fn unknown_or_empty_urls_resolve_to_nothing() {
        let registry = registry();
        assert_eq!(registry.resolve_url("https://example.com/addon"), None);
        assert_eq!(registry.resolve_url("   "), None);
    }

    #[test]
    #[should_panic(expected = "duplicate source id \"first\"")]
    fn duplicate_ids_panic() {
        Registry::new(vec![
            Box::new(MockSource::new("first", "mock.test")),
            Box::new(MockSource::new("first", "other.test")),
        ]);
    }

    #[test]
    fn search_is_unsupported_by_default() {
        let registry = registry();
        let err = registry
            .get("first")
            .unwrap()
            .search("bigwigs")
            .unwrap_err();
        assert!(matches!(err, SourceError::Unsupported(_)), "{err:?}");
        assert_eq!(err.to_string(), "Mock does not support search");
    }
}
