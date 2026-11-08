//! Addons published as GitHub releases. Projects are `owner/repo`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use super::{Release, ReleaseFile, Source, SourceError};
use crate::clients::GameClient;
use crate::http::Http;
use crate::state::Channel;

const API: &str = "https://api.github.com";

pub struct GitHubSource {
    http: Arc<dyn Http>,
}

impl GitHubSource {
    pub fn new(http: Arc<dyn Http>) -> Self {
        Self { http }
    }

    /// Published releases, newest first.
    fn releases(&self, project: &str) -> Result<Vec<GhRelease>, SourceError> {
        let url = format!("{API}/repos/{project}/releases?per_page=100");
        let response = self.http.get(
            &url,
            &[
                ("Accept", "application/vnd.github+json"),
                ("X-GitHub-Api-Version", "2022-11-28"),
            ],
        )?;
        match response.status {
            200 => {}
            404 => return Err(SourceError::NotFound(project.into())),
            status => {
                return Err(SourceError::Http(format!(
                    "GitHub answered {status} for {project}"
                )));
            }
        }
        let mut releases: Vec<GhRelease> = serde_json::from_slice(&response.body)
            .map_err(|e| SourceError::Http(format!("unexpected GitHub response: {e}")))?;
        releases.retain(|r| !r.draft);
        // GitHub's own order follows the tagged commits, not when a release was published.
        releases.sort_by(|a, b| b.published_at.cmp(&a.published_at));
        Ok(releases)
    }
}

impl Source for GitHubSource {
    fn id(&self) -> &'static str {
        "github"
    }

    fn name(&self) -> &'static str {
        "GitHub"
    }

    fn parse_url(&self, url: &str) -> Option<String> {
        parse_url(url)
    }

    fn latest(
        &self,
        project: &str,
        client: &GameClient,
        channel: Channel,
    ) -> Result<Release, SourceError> {
        let releases = self.releases(project)?;
        let mut on_channel = releases
            .iter()
            .filter(|r| channel.includes(r.channel()))
            .peekable();
        if on_channel.peek().is_none() {
            return Err(SourceError::NoRelease(project.into()));
        }
        on_channel
            .find_map(|r| {
                pick_file(r, client).map(|file| Release {
                    version: r.tag_name.clone(),
                    channel: r.channel(),
                    file,
                })
            })
            .ok_or_else(|| SourceError::NoFileForClient(project.into()))
    }

    fn download(&self, release: &Release, into_dir: &Path) -> Result<PathBuf, SourceError> {
        let dest = into_dir.join(&release.file.name);
        self.http.download(&release.file.url, &dest)?;
        Ok(dest)
    }
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    /// ISO 8601 in UTC, so it sorts as text.
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

impl GhRelease {
    /// Full releases are stable. Pre-releases are alpha when the tag says so, beta otherwise.
    fn channel(&self) -> Channel {
        if !self.prerelease {
            Channel::Stable
        } else if self.tag_name.to_ascii_lowercase().contains("alpha") {
            Channel::Alpha
        } else {
            Channel::Beta
        }
    }
}

/// The zip to install for `client`. For now only releases with a single zip qualify;
/// choosing between several zips comes with `release.json` support.
fn pick_file(release: &GhRelease, _client: &GameClient) -> Option<ReleaseFile> {
    let mut zips = release
        .assets
        .iter()
        .filter(|a| a.name.to_ascii_lowercase().ends_with(".zip"));
    match (zips.next(), zips.next()) {
        (Some(zip), None) => Some(ReleaseFile {
            name: zip.name.clone(),
            url: zip.browser_download_url.clone(),
        }),
        _ => None,
    }
}

/// Accepts `owner/repo`, GitHub web and clone URLs, with or without a scheme.
fn parse_url(input: &str) -> Option<String> {
    let rest = if let Some(ssh) = input.strip_prefix("git@github.com:") {
        ssh
    } else {
        let without_scheme = input
            .strip_prefix("https://")
            .or_else(|| input.strip_prefix("http://"))
            .unwrap_or(input);
        let lower = without_scheme.to_ascii_lowercase();
        if lower.starts_with("github.com/") {
            &without_scheme["github.com/".len()..]
        } else if lower.starts_with("www.github.com/") {
            &without_scheme["www.github.com/".len()..]
        } else if without_scheme.len() == input.len() {
            // Bare `owner/repo`: nothing may follow the repo.
            let mut parts = input.split('/');
            let (owner, repo) = (parts.next()?, parts.next()?);
            return (parts.next().is_none())
                .then(|| project(owner, repo))
                .flatten();
        } else {
            return None;
        }
    };
    let rest = rest.split(['?', '#']).next()?;
    let mut parts = rest.split('/');
    project(parts.next()?, parts.next()?)
}

fn project(owner: &str, repo: &str) -> Option<String> {
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let owner_ok =
        !owner.is_empty() && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let repo_ok = !repo.is_empty()
        && repo != "."
        && repo != ".."
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (owner_ok && repo_ok).then(|| format!("{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::recorded::RecordedHttp;
    use crate::sources::mock::client;

    const RETAIL: u32 = 120105;

    fn releases_url(project: &str) -> String {
        format!("{API}/repos/{project}/releases?per_page=100")
    }

    fn source(http: RecordedHttp) -> GitHubSource {
        GitHubSource::new(Arc::new(http))
    }

    fn recorded(project: &str, fixture: &str) -> GitHubSource {
        source(RecordedHttp::new().fixture(&releases_url(project), fixture))
    }

    #[test]
    fn parses_repos_and_urls() {
        let cases = [
            ("BigWigsMods/BigWigs", "BigWigsMods/BigWigs"),
            (
                "https://github.com/BigWigsMods/BigWigs",
                "BigWigsMods/BigWigs",
            ),
            (
                "http://github.com/BigWigsMods/BigWigs/",
                "BigWigsMods/BigWigs",
            ),
            ("github.com/ls-/ls_UI", "ls-/ls_UI"),
            ("https://www.GitHub.com/ls-/ls_UI.git", "ls-/ls_UI"),
            (
                "https://github.com/WeakAuras/WeakAuras2/releases/tag/5.22.1",
                "WeakAuras/WeakAuras2",
            ),
            (
                "https://github.com/WeakAuras/WeakAuras2?tab=readme",
                "WeakAuras/WeakAuras2",
            ),
            (
                "git@github.com:WeakAuras/WeakAuras2.git",
                "WeakAuras/WeakAuras2",
            ),
            ("Owner/addon.name", "Owner/addon.name"),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_url(input).as_deref(), Some(expected), "{input}");
        }
    }

    #[test]
    fn rejects_other_urls() {
        for input in [
            "",
            "BigWigs",
            "BigWigsMods/BigWigs/extra",
            "https://gitlab.com/owner/repo",
            "https://mock.test/BigWigs",
            "mock.test/BigWigs",
            "https://github.com/BigWigsMods",
            "https://github.com/BigWigsMods/",
            "https://github.com/a b/c",
            "owner/..",
        ] {
            assert_eq!(parse_url(input), None, "{input:?}");
        }
    }

    #[test]
    fn finds_the_zip_of_a_packager_release() {
        let release = recorded("BigWigsMods/BigWigs", "github/bigwigs-releases.json")
            .latest("BigWigsMods/BigWigs", &client(RETAIL), Channel::Stable)
            .unwrap();
        assert_eq!(
            release,
            Release {
                version: "v427.8".into(),
                channel: Channel::Stable,
                file: ReleaseFile {
                    name: "BigWigs-v427.8.zip".into(),
                    url: "https://github.com/BigWigsMods/BigWigs/releases/download/v427.8/BigWigs-v427.8.zip".into(),
                },
            }
        );
    }

    #[test]
    fn finds_a_plain_zip_without_release_json() {
        let release = recorded("ls-/ls_UI", "github/ls_ui-releases.json")
            .latest("ls-/ls_UI", &client(RETAIL), Channel::Stable)
            .unwrap();
        assert_eq!(release.version, "110207.02");
        assert_eq!(release.file.name, "ls_UI-110207.02.zip");
    }

    #[test]
    fn skips_releases_without_files() {
        let release = recorded("WeakAuras/WeakAuras2", "github/weakauras-releases.json")
            .latest("WeakAuras/WeakAuras2", &client(RETAIL), Channel::Stable)
            .unwrap();
        assert_eq!(release.version, "5.21.8");
        assert_eq!(release.file.name, "WeakAuras-5.21.8.zip");
    }

    #[test]
    fn reads_channels_from_prereleases_in_publish_order() {
        let source = recorded(
            "kesava-wow/kuinameplates2",
            "github/kuinameplates2-releases.json",
        );
        let releases = source.releases("kesava-wow/kuinameplates2").unwrap();
        let listed: Vec<_> = releases
            .iter()
            .map(|r| (r.tag_name.as_str(), r.channel()))
            .collect();
        assert_eq!(
            listed,
            [
                ("2.29.1", Channel::Stable),
                ("2.29.0", Channel::Stable),
                ("2.28.0", Channel::Stable),
                // GitHub lists the beta first, but the alpha was published later.
                ("alpha-giggling-vampire-roaring-whale", Channel::Alpha),
                ("beta-giggling-shark-smiling-wolf", Channel::Beta),
                ("2.27.3a", Channel::Stable),
            ]
        );
    }

    #[test]
    fn several_zips_have_no_file_yet() {
        let err = recorded(
            "kesava-wow/kuinameplates2",
            "github/kuinameplates2-releases.json",
        )
        .latest("kesava-wow/kuinameplates2", &client(RETAIL), Channel::Alpha)
        .unwrap_err();
        assert!(matches!(err, SourceError::NoFileForClient(_)), "{err:?}");
    }

    #[test]
    fn ignores_drafts_and_respects_the_channel() {
        let body = r#"[
            {"tag_name": "v3-draft", "draft": true, "prerelease": false,
             "published_at": null, "assets": [{"name": "A-v3.zip", "browser_download_url": "https://x/3"}]},
            {"tag_name": "v2-beta", "draft": false, "prerelease": true,
             "published_at": "2026-02-01T00:00:00Z", "assets": [{"name": "A-v2.zip", "browser_download_url": "https://x/2"}]}
        ]"#;
        let source = source(RecordedHttp::new().respond(&releases_url("o/a"), 200, body));
        let latest = |channel| source.latest("o/a", &client(RETAIL), channel);
        assert_eq!(latest(Channel::Beta).unwrap().version, "v2-beta");
        assert!(matches!(
            latest(Channel::Stable),
            Err(SourceError::NoRelease(p)) if p == "o/a"
        ));
    }

    #[test]
    fn unknown_repos_are_not_found() {
        let source = source(RecordedHttp::new().respond(
            &releases_url("nobody/nothing"),
            404,
            r#"{"message":"Not Found","status":"404"}"#,
        ));
        let err = source
            .latest("nobody/nothing", &client(RETAIL), Channel::Stable)
            .unwrap_err();
        assert!(matches!(err, SourceError::NotFound(p) if p == "nobody/nothing"));
    }

    #[test]
    fn other_statuses_are_http_errors() {
        let source = source(RecordedHttp::new().respond(&releases_url("o/a"), 500, "oops"));
        let err = source
            .latest("o/a", &client(RETAIL), Channel::Stable)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "request failed: GitHub answered 500 for o/a"
        );
    }

    #[test]
    fn downloads_the_release_file() {
        let zip_url =
            "https://github.com/BigWigsMods/BigWigs/releases/download/v427.8/BigWigs-v427.8.zip";
        let http = Arc::new(
            RecordedHttp::new()
                .fixture(
                    &releases_url("BigWigsMods/BigWigs"),
                    "github/bigwigs-releases.json",
                )
                .respond(zip_url, 200, "zip bytes"),
        );
        let source = GitHubSource::new(http.clone());
        let release = source
            .latest("BigWigsMods/BigWigs", &client(RETAIL), Channel::Stable)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = source.download(&release, dir.path()).unwrap();
        assert_eq!(path, dir.path().join("BigWigs-v427.8.zip"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "zip bytes");
        assert_eq!(http.requests().last().map(String::as_str), Some(zip_url));
    }

    /// Talks to the real GitHub API: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_latest_and_download() {
        let source = GitHubSource::new(Arc::new(crate::http::UreqHttp::new()));
        let dir = tempfile::tempdir().unwrap();
        for project in ["BigWigsMods/BigWigs", "ls-/ls_UI"] {
            let release = source
                .latest(project, &client(RETAIL), Channel::Stable)
                .unwrap();
            let path = source.download(&release, dir.path()).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.starts_with(b"PK"), "{project}: not a zip");
            println!("{project}: {} ({} bytes)", release.version, bytes.len());
        }
    }
}
