//! An in-memory source for tests. Its addon URLs look like `https://<host>/<project>`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Release, ReleaseFile, Source, SourceError, Target};
use crate::state::Channel;

pub struct MockSource {
    id: &'static str,
    host: &'static str,
    /// Releases per project, newest first.
    projects: HashMap<String, Vec<MockRelease>>,
    needs_key: bool,
}

pub struct MockRelease {
    pub version: &'static str,
    pub channel: Channel,
    /// The client interfaces the release has a file for.
    pub interfaces: Vec<u32>,
}

impl MockSource {
    pub fn new(id: &'static str, host: &'static str) -> Self {
        Self {
            id,
            host,
            projects: HashMap::new(),
            needs_key: false,
        }
    }

    /// Adds a project; `releases` are newest first.
    pub fn with_project(mut self, project: &str, releases: Vec<MockRelease>) -> Self {
        self.projects.insert(project.to_string(), releases);
        self
    }

    /// Makes the source act as if its API key is missing.
    pub fn needing_key(mut self) -> Self {
        self.needs_key = true;
        self
    }
}

impl Source for MockSource {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        "Mock"
    }

    fn parse_url(&self, url: &str) -> Option<String> {
        let project = url.strip_prefix(&format!("https://{}/", self.host))?;
        (!project.is_empty() && !project.contains('/')).then(|| project.to_string())
    }

    fn latest(
        &self,
        project: &str,
        target: &Target,
        channel: Channel,
    ) -> Result<Release, SourceError> {
        if self.needs_key {
            return Err(SourceError::NeedsKey(self.name().into()));
        }
        let releases = self
            .projects
            .get(project)
            .ok_or_else(|| SourceError::NotFound(project.into()))?;
        let mut on_channel = releases
            .iter()
            .filter(|r| channel.includes(r.channel))
            .peekable();
        if on_channel.peek().is_none() {
            return Err(SourceError::NoRelease(project.into()));
        }
        let release = on_channel
            .find(|r| target.interface.is_some_and(|i| r.interfaces.contains(&i)))
            .ok_or_else(|| SourceError::NoFileForClient(project.into()))?;
        Ok(Release {
            version: release.version.into(),
            channel: release.channel,
            file: ReleaseFile {
                name: format!("{project}-{}.zip", release.version),
                url: format!("https://{}/{project}/{}.zip", self.host, release.version),
            },
        })
    }

    fn download(&self, release: &Release, into_dir: &Path) -> Result<PathBuf, SourceError> {
        if self.needs_key {
            return Err(SourceError::NeedsKey(self.name().into()));
        }
        let path = into_dir.join(&release.file.name);
        fs::write(&path, &release.file.url)?;
        Ok(path)
    }

    fn needs_key(&self) -> bool {
        self.needs_key
    }
}

/// A target known only by its interface number.
pub fn target(interface: u32) -> Target {
    Target {
        interface: Some(interface),
        flavor: None,
        overridden: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Channel::*;

    const RETAIL: u32 = 120105;
    const ERA: u32 = 11509;

    fn release(version: &'static str, channel: Channel, interfaces: &[u32]) -> MockRelease {
        MockRelease {
            version,
            channel,
            interfaces: interfaces.to_vec(),
        }
    }

    fn source() -> MockSource {
        MockSource::new("mock", "mock.test")
            .with_project(
                "BigWigs",
                vec![
                    release("v3-alpha", Alpha, &[RETAIL, ERA]),
                    release("v2-beta", Beta, &[RETAIL]),
                    release("v2", Stable, &[RETAIL]),
                    release("v1", Stable, &[RETAIL, ERA]),
                ],
            )
            .with_project("Nightly", vec![release("n1", Alpha, &[RETAIL])])
    }

    #[test]
    fn parses_only_its_own_project_urls() {
        let source = source();
        assert_eq!(
            source.parse_url("https://mock.test/BigWigs"),
            Some("BigWigs".into())
        );
        assert_eq!(source.parse_url("https://mock.test/"), None);
        assert_eq!(source.parse_url("https://mock.test/a/b"), None);
        assert_eq!(source.parse_url("https://other.test/BigWigs"), None);
    }

    #[test]
    fn latest_respects_the_channel() {
        let source = source();
        let version = |channel| {
            source
                .latest("BigWigs", &target(RETAIL), channel)
                .unwrap()
                .version
        };
        assert_eq!(version(Stable), "v2");
        assert_eq!(version(Beta), "v2-beta");
        assert_eq!(version(Alpha), "v3-alpha");
    }

    #[test]
    fn latest_skips_releases_without_a_file_for_the_client() {
        let release = source().latest("BigWigs", &target(ERA), Stable).unwrap();
        assert_eq!(release.version, "v1");
        assert_eq!(release.channel, Stable);
        assert_eq!(release.file.name, "BigWigs-v1.zip");
        assert_eq!(release.file.url, "https://mock.test/BigWigs/v1.zip");
    }

    #[test]
    fn latest_reports_why_nothing_was_found() {
        let source = source();
        let err = |project, interface, channel| {
            source
                .latest(project, &target(interface), channel)
                .unwrap_err()
        };
        assert!(matches!(
            err("Missing", RETAIL, Stable),
            SourceError::NotFound(p) if p == "Missing"
        ));
        assert!(matches!(
            err("Nightly", RETAIL, Stable),
            SourceError::NoRelease(p) if p == "Nightly"
        ));
        assert!(matches!(
            err("Nightly", ERA, Alpha),
            SourceError::NoFileForClient(p) if p == "Nightly"
        ));
    }

    #[test]
    fn downloads_into_the_given_directory() {
        let dir = tempfile::tempdir().unwrap();
        let source = source();
        let release = source.latest("BigWigs", &target(RETAIL), Stable).unwrap();
        let path = source.download(&release, dir.path()).unwrap();
        assert_eq!(path, dir.path().join("BigWigs-v2.zip"));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "https://mock.test/BigWigs/v2.zip"
        );
    }

    #[test]
    fn a_source_needing_a_key_refuses_to_work() {
        let source = source().needing_key();
        assert!(source.needs_key());
        assert!(!MockSource::new("mock", "mock.test").needs_key());
        let err = source
            .latest("BigWigs", &target(RETAIL), Stable)
            .unwrap_err();
        assert!(matches!(err, SourceError::NeedsKey(_)), "{err:?}");
        assert_eq!(err.to_string(), "Mock needs an API key");
    }
}
