//! Addons published as GitHub releases. Projects are `owner/repo`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use super::select::{ReleaseJson, select_file};
use super::{Release, ReleaseFile, Source, SourceError, Target};
use crate::http::Http;
use crate::state::Channel;

const API: &str = "https://api.github.com";

/// How many releases with zips to look through for one with a file for the client.
/// Each may cost a `release.json` download.
const MAX_RELEASES_CHECKED: usize = 10;

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
        target: &Target,
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
        // Releases without zips (failed uploads, source-only tags) don't count towards the limit.
        let with_zips = on_channel
            .filter(|r| r.zips().next().is_some())
            .take(MAX_RELEASES_CHECKED);
        for release in with_zips {
            if let Some(file) = self.pick_file(release, target)? {
                return Ok(Release {
                    version: release.tag_name.clone(),
                    channel: release.channel(),
                    file,
                });
            }
        }
        Err(SourceError::NoFileForClient(project.into()))
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

impl GhRelease {
    fn zips(&self) -> impl Iterator<Item = &GhAsset> {
        self.assets
            .iter()
            .filter(|a| a.name.to_ascii_lowercase().ends_with(".zip"))
    }

    fn release_json(&self) -> Option<&GhAsset> {
        self.assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case("release.json"))
    }
}

impl GitHubSource {
    /// The zip in `release` to install for `target`, using its `release.json` if it has one.
    fn pick_file(
        &self,
        release: &GhRelease,
        target: &Target,
    ) -> Result<Option<ReleaseFile>, SourceError> {
        let release_json = match release.release_json() {
            Some(asset) => self.fetch_release_json(asset)?,
            None => None,
        };
        let zips: Vec<&GhAsset> = release.zips().collect();
        let names: Vec<&str> = zips.iter().map(|a| a.name.as_str()).collect();
        let chosen = select_file(&names, release_json.as_ref(), target);
        Ok(chosen.and_then(|name| {
            zips.iter().find(|a| a.name == name).map(|a| ReleaseFile {
                name: a.name.clone(),
                url: a.browser_download_url.clone(),
            })
        }))
    }

    /// A `release.json` that can't be parsed is ignored, so file names decide instead.
    fn fetch_release_json(&self, asset: &GhAsset) -> Result<Option<ReleaseJson>, SourceError> {
        let response = self.http.get(&asset.browser_download_url, &[])?;
        if response.status != 200 {
            return Err(SourceError::Http(format!(
                "GitHub answered {} for {}",
                response.status, asset.browser_download_url
            )));
        }
        Ok(serde_json::from_slice(&response.body).ok())
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
    use crate::sources::Flavor;

    const RETAIL: u32 = 120105;
    const ERA: u32 = 11509;
    const ANNIVERSARY: u32 = 20506;

    fn target(interface: u32, flavor: Flavor) -> Target {
        Target {
            interface: Some(interface),
            flavor: Some(flavor),
            overridden: false,
        }
    }

    fn retail() -> Target {
        target(RETAIL, Flavor::Mainline)
    }

    fn releases_url(project: &str) -> String {
        format!("{API}/repos/{project}/releases?per_page=100")
    }

    fn source(http: RecordedHttp) -> GitHubSource {
        GitHubSource::new(Arc::new(http))
    }

    /// The recorded release list of `project`, plus the recorded `release.json` of each
    /// tag in `release_jsons`: `github/<prefix>-<tag>-release.json`.
    fn recorded_http(project: &str, prefix: &str, release_jsons: &[&str]) -> RecordedHttp {
        let http = RecordedHttp::new().fixture(
            &releases_url(project),
            &format!("github/{prefix}-releases.json"),
        );
        release_jsons.iter().fold(http, |http, tag| {
            http.fixture(
                &format!("https://github.com/{project}/releases/download/{tag}/release.json"),
                &format!("github/{prefix}-{tag}-release.json"),
            )
        })
    }

    fn recorded(project: &str, prefix: &str, release_jsons: &[&str]) -> GitHubSource {
        source(recorded_http(project, prefix, release_jsons))
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
        let release = recorded("BigWigsMods/BigWigs", "bigwigs", &["v427.8"])
            .latest("BigWigsMods/BigWigs", &retail(), Channel::Stable)
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
        let release = recorded("ls-/ls_UI", "ls_ui", &[])
            .latest("ls-/ls_UI", &retail(), Channel::Stable)
            .unwrap();
        assert_eq!(release.version, "110207.02");
        assert_eq!(release.file.name, "ls_UI-110207.02.zip");
    }

    #[test]
    fn skips_releases_without_files() {
        // 5.21.8 lists classic 11508 only, so Era 11509 gets it through the flavor.
        let source = recorded("WeakAuras/WeakAuras2", "weakauras", &["5.21.8"]);
        let release = source
            .latest(
                "WeakAuras/WeakAuras2",
                &target(ERA, Flavor::Classic),
                Channel::Stable,
            )
            .unwrap();
        assert_eq!(release.version, "5.21.8");
        assert_eq!(release.file.name, "WeakAuras-5.21.8.zip");
        // It has no retail file at all.
        let err = source
            .latest("WeakAuras/WeakAuras2", &retail(), Channel::Stable)
            .unwrap_err();
        assert!(matches!(err, SourceError::NoFileForClient(_)), "{err:?}");
    }

    #[test]
    fn reads_channels_from_prereleases_in_publish_order() {
        let source = recorded("kesava-wow/kuinameplates2", "kuinameplates2", &[]);
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

    fn kui() -> GitHubSource {
        recorded(
            "kesava-wow/kuinameplates2",
            "kuinameplates2",
            &["2.29.1", "2.29.0", "2.28.0", "2.27.3a"],
        )
    }

    #[test]
    fn picks_the_zip_for_the_client_from_several() {
        let latest = |target| kui().latest("kesava-wow/kuinameplates2", &target, Channel::Alpha);
        assert_eq!(
            latest(retail()).unwrap().file.name,
            "Kui_Nameplates-2.29.1.zip"
        );
        assert_eq!(
            latest(target(ERA, Flavor::Classic)).unwrap().file.name,
            "Kui_Nameplates-2.29.1-classic.zip"
        );
    }

    #[test]
    fn falls_back_to_an_older_release_with_a_file_for_the_client() {
        let release = kui()
            .latest(
                "kesava-wow/kuinameplates2",
                &target(ANNIVERSARY, Flavor::Bcc),
                Channel::Stable,
            )
            .unwrap();
        assert_eq!(release.version, "2.27.3a");
        assert_eq!(release.file.name, "Kui_Nameplates-2.27.3a-bcc.zip");
    }

    #[test]
    fn stops_looking_after_ten_releases() {
        // Twelve releases, newest first, where only the `classic`-th has a classic zip.
        let latest_with_classic_at =
            |classic: usize| {
                let releases: Vec<_> = (0..12)
                .map(|i| {
                    let name = if i == classic { "A-classic.zip" } else { "A-retail.zip" };
                    serde_json::json!({
                        "tag_name": format!("v{i}"),
                        "published_at": format!("2026-01-{:02}T00:00:00Z", 20 - i),
                        "assets": [{"name": name, "browser_download_url": format!("https://x/{i}")}]
                    })
                })
                .collect();
                let body = serde_json::to_vec(&releases).unwrap();
                source(RecordedHttp::new().respond(&releases_url("o/a"), 200, body)).latest(
                    "o/a",
                    &target(ERA, Flavor::Classic),
                    Channel::Stable,
                )
            };
        assert_eq!(latest_with_classic_at(9).unwrap().version, "v9");
        let err = latest_with_classic_at(10).unwrap_err();
        assert!(matches!(err, SourceError::NoFileForClient(_)), "{err:?}");
    }

    #[test]
    fn an_unreadable_release_json_falls_back_to_file_names() {
        let body = r#"[{"tag_name": "v1", "published_at": "2026-01-01T00:00:00Z", "assets": [
            {"name": "A-v1.zip", "browser_download_url": "https://x/A-v1.zip"},
            {"name": "A-v1-classic.zip", "browser_download_url": "https://x/A-v1-classic.zip"},
            {"name": "release.json", "browser_download_url": "https://x/release.json"}
        ]}]"#;
        let source = source(
            RecordedHttp::new()
                .respond(&releases_url("o/a"), 200, body)
                .respond("https://x/release.json", 200, "<html>not json</html>"),
        );
        let release = source
            .latest("o/a", &target(ERA, Flavor::Classic), Channel::Stable)
            .unwrap();
        assert_eq!(release.file.name, "A-v1-classic.zip");
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
        let latest = |channel| source.latest("o/a", &retail(), channel);
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
            .latest("nobody/nothing", &retail(), Channel::Stable)
            .unwrap_err();
        assert!(matches!(err, SourceError::NotFound(p) if p == "nobody/nothing"));
    }

    #[test]
    fn other_statuses_are_http_errors() {
        let source = source(RecordedHttp::new().respond(&releases_url("o/a"), 500, "oops"));
        let err = source
            .latest("o/a", &retail(), Channel::Stable)
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
            recorded_http("BigWigsMods/BigWigs", "bigwigs", &["v427.8"]).respond(
                zip_url,
                200,
                "zip bytes",
            ),
        );
        let source = GitHubSource::new(http.clone());
        let release = source
            .latest("BigWigsMods/BigWigs", &retail(), Channel::Stable)
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
            let release = source.latest(project, &retail(), Channel::Stable).unwrap();
            let path = source.download(&release, dir.path()).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.starts_with(b"PK"), "{project}: not a zip");
            println!("{project}: {} ({} bytes)", release.version, bytes.len());
        }
    }
}
