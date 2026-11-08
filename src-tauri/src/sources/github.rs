//! Addons published as GitHub releases. Projects are `owner/repo`.
//!
//! An optional token raises the API's rate limit from 60 to 5,000 requests an hour. It is
//! only sent to `api.github.com`, never with downloads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::select::{ReleaseJson, select_file};
use super::{Release, ReleaseFile, Source, SourceError, Target};
use crate::http::{Http, Response};
use crate::state::Channel;

const API: &str = "https://api.github.com";

/// How many releases with zips to look through for one with a file for the client.
/// Each may cost a `release.json` download.
const MAX_RELEASES_CHECKED: usize = 10;

/// How long to wait after a rate limit that doesn't say.
const DEFAULT_WAIT: Duration = Duration::from_secs(60);

pub struct GitHubSource {
    http: Arc<dyn Http>,
    token: RwLock<Option<String>>,
    /// Set when GitHub reports a rate limit. No API requests are sent until it has passed.
    blocked: Mutex<Option<RateLimitHit>>,
    /// Parsed `release.json` files by URL. They belong to a published tag, so they don't change.
    release_jsons: Mutex<HashMap<String, Option<ReleaseJson>>>,
    clock: fn() -> SystemTime,
}

/// The API rate limit that release lookups count against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub limit: u32,
    pub remaining: u32,
    /// When the limit resets, in seconds since the Unix epoch.
    pub resets_at: u64,
}

impl GitHubSource {
    pub fn new(http: Arc<dyn Http>) -> Self {
        Self {
            http,
            token: RwLock::default(),
            blocked: Mutex::default(),
            release_jsons: Mutex::default(),
            clock: SystemTime::now,
        }
    }

    /// Replaces the token (`None` removes it), and forgets any rate limit hit so far.
    pub fn set_token(&self, token: Option<String>) {
        *self.token.write().unwrap() = token;
        *self.blocked.lock().unwrap() = None;
    }

    pub fn has_token(&self) -> bool {
        self.token.read().unwrap().is_some()
    }

    fn token(&self) -> Option<String> {
        self.token.read().unwrap().clone()
    }

    /// The rate limit of the current token, or of anonymous use without one. Asking doesn't
    /// count against it.
    pub fn rate_limit(&self) -> Result<RateLimit, SourceError> {
        self.fetch_rate_limit(self.token().as_deref())
    }

    /// The rate limit `token` would get: fails with [`SourceError::BadToken`] if GitHub
    /// doesn't accept it.
    pub fn check_token(&self, token: &str) -> Result<RateLimit, SourceError> {
        self.fetch_rate_limit(Some(token))
    }

    fn fetch_rate_limit(&self, token: Option<&str>) -> Result<RateLimit, SourceError> {
        let response = self.send("/rate_limit", token)?;
        if response.status != 200 {
            return Err(SourceError::Http(format!(
                "GitHub answered {} for the rate limit",
                response.status
            )));
        }
        let limits: GhRateLimits = serde_json::from_slice(&response.body)
            .map_err(|e| SourceError::Http(format!("unexpected GitHub response: {e}")))?;
        let core = limits.resources.core;
        Ok(RateLimit {
            limit: core.limit,
            remaining: core.remaining,
            resets_at: core.reset,
        })
    }

    /// An API request with the current token, unless a rate limit is still in force.
    fn api_get(&self, path: &str) -> Result<Response, SourceError> {
        let now = (self.clock)();
        if let Some(hit) = self.blocked.lock().unwrap().as_ref()
            && now < hit.until
        {
            return Err(SourceError::RateLimited(hit.message(now)));
        }
        self.send(path, self.token().as_deref())
    }

    /// Sends an API request. Turns a rejected token and rate limits into errors, and
    /// remembers rate limits so [`Self::api_get`] waits them out.
    fn send(&self, path: &str, token: Option<&str>) -> Result<Response, SourceError> {
        let authorization = token.map(|token| format!("Bearer {token}"));
        let mut headers = vec![
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ];
        if let Some(value) = &authorization {
            headers.push(("Authorization", value));
        }
        let response = self.http.get(&format!("{API}{path}"), &headers)?;
        match response.status {
            401 if token.is_some() => Err(SourceError::BadToken(
                "GitHub didn't accept the token. It may be mistyped, expired or revoked.".into(),
            )),
            403 | 429 => {
                let now = (self.clock)();
                match RateLimitHit::from_response(&response, token.is_some(), now) {
                    Some(hit) => {
                        let message = hit.message(now);
                        *self.blocked.lock().unwrap() = Some(hit);
                        Err(SourceError::RateLimited(message))
                    }
                    None => Ok(response),
                }
            }
            _ => Ok(response),
        }
    }

    /// Published releases, newest first.
    fn releases(&self, project: &str) -> Result<Vec<GhRelease>, SourceError> {
        let response = self.api_get(&format!("/repos/{project}/releases?per_page=100"))?;
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
        let url = &asset.browser_download_url;
        if let Some(cached) = self.release_jsons.lock().unwrap().get(url) {
            return Ok(cached.clone());
        }
        let response = self.http.get(url, &[])?;
        if response.status != 200 {
            return Err(SourceError::Http(format!(
                "GitHub answered {} for {url}",
                response.status
            )));
        }
        let parsed: Option<ReleaseJson> = serde_json::from_slice(&response.body).ok();
        self.release_jsons
            .lock()
            .unwrap()
            .insert(url.clone(), parsed.clone());
        Ok(parsed)
    }
}

#[derive(Deserialize)]
struct GhRateLimits {
    resources: GhResources,
}

#[derive(Deserialize)]
struct GhResources {
    core: GhRateLimit,
}

#[derive(Deserialize)]
struct GhRateLimit {
    limit: u32,
    remaining: u32,
    reset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Limit {
    /// The hourly allowance is used up.
    Hourly { limit: u64, with_token: bool },
    /// GitHub wants fewer requests in a short time.
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RateLimitHit {
    limit: Limit,
    until: SystemTime,
}

impl RateLimitHit {
    /// `None` if a 403 or 429 isn't about rate limits, such as a blocked repository.
    fn from_response(response: &Response, with_token: bool, now: SystemTime) -> Option<Self> {
        let number = |name: &str| -> Option<u64> { response.header(name)?.trim().parse().ok() };
        if number("x-ratelimit-remaining") == Some(0) {
            let until = number("x-ratelimit-reset")
                .map(|reset| UNIX_EPOCH + Duration::from_secs(reset))
                .unwrap_or(now + DEFAULT_WAIT);
            let limit = number("x-ratelimit-limit").unwrap_or(if with_token { 5000 } else { 60 });
            return Some(Self {
                limit: Limit::Hourly { limit, with_token },
                until,
            });
        }
        let wait = number("retry-after").map(Duration::from_secs).or_else(|| {
            let body = String::from_utf8_lossy(&response.body).to_ascii_lowercase();
            body.contains("rate limit").then_some(DEFAULT_WAIT)
        })?;
        Some(Self {
            limit: Limit::Secondary,
            until: now + wait,
        })
    }

    fn message(&self, now: SystemTime) -> String {
        let wait = minutes(self.until.duration_since(now).unwrap_or_default());
        match self.limit {
            Limit::Hourly {
                limit,
                with_token: false,
            } => format!(
                "GitHub's limit of {} requests an hour without a token is used up. It resets in \
                 {wait}. Add a GitHub token to raise it to 5,000 an hour.",
                thousands(limit)
            ),
            Limit::Hourly {
                limit,
                with_token: true,
            } => format!(
                "GitHub's limit of {} requests an hour is used up. It resets in {wait}.",
                thousands(limit)
            ),
            Limit::Secondary => format!("GitHub asked the app to slow down. Try again in {wait}."),
        }
    }
}

/// Whole minutes, rounded up: `1 minute`, `12 minutes`.
fn minutes(wait: Duration) -> String {
    match wait.as_secs().div_ceil(60).max(1) {
        1 => "1 minute".into(),
        n => format!("{n} minutes"),
    }
}

/// `5000` as `5,000`.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
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

    /// 2027-01-15 08:00 UTC, as seen by the sources the rate-limit tests build.
    const NOW: u64 = 1_800_000_000;

    fn at_now(mut source: GitHubSource) -> GitHubSource {
        source.clock = || UNIX_EPOCH + Duration::from_secs(NOW);
        source
    }

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn sends_the_token_to_the_api_only() {
        let http = Arc::new(recorded_http("BigWigsMods/BigWigs", "bigwigs", &["v427.8"]));
        let source = GitHubSource::new(http.clone());
        source
            .latest("BigWigsMods/BigWigs", &retail(), Channel::Stable)
            .unwrap();
        assert_eq!(header(&http.request_headers(0), "authorization"), None);

        source.set_token(Some("ghp_secret".into()));
        assert!(source.has_token());
        source
            .latest("BigWigsMods/BigWigs", &retail(), Channel::Stable)
            .unwrap();
        let requests = http.requests();
        assert_eq!(
            requests.len(),
            3,
            "release.json is fetched once: {requests:?}"
        );
        assert!(requests[2].starts_with(API));
        assert_eq!(
            header(&http.request_headers(2), "authorization"),
            Some("Bearer ghp_secret")
        );
        assert_eq!(header(&http.request_headers(1), "authorization"), None);
    }

    /// A source whose release list of `o/a` answers with `response`.
    fn answering(response: Response) -> (Arc<RecordedHttp>, GitHubSource) {
        let http = Arc::new(RecordedHttp::new().respond_with(&releases_url("o/a"), response));
        let source = at_now(GitHubSource::new(http.clone()));
        (http, source)
    }

    /// The hourly limit is used up and resets in 11.5 minutes.
    fn hourly_limit(limit: &str) -> Response {
        Response::new(
            403,
            r#"{"message":"API rate limit exceeded for 192.0.2.1."}"#,
        )
        .with_header("X-RateLimit-Limit", limit)
        .with_header("X-RateLimit-Remaining", "0")
        .with_header("X-RateLimit-Reset", &(NOW + 12 * 60 - 30).to_string())
    }

    fn rate_limit_error(source: &GitHubSource) -> String {
        match source.latest("o/a", &retail(), Channel::Stable) {
            Err(SourceError::RateLimited(message)) => message,
            other => panic!("expected a rate limit, got {other:?}"),
        }
    }

    #[test]
    fn explains_the_hourly_limit_without_a_token() {
        let (http, source) = answering(hourly_limit("60"));
        let message = rate_limit_error(&source);
        assert_eq!(
            message,
            "GitHub's limit of 60 requests an hour without a token is used up. It resets in 12 \
             minutes. Add a GitHub token to raise it to 5,000 an hour."
        );
        // Until the reset, nothing more is sent.
        assert_eq!(rate_limit_error(&source), message);
        assert_eq!(http.requests().len(), 1);
        // A new token gets a fresh start.
        source.set_token(Some("t".into()));
        rate_limit_error(&source);
        assert_eq!(http.requests().len(), 2);
    }

    #[test]
    fn explains_the_hourly_limit_with_a_token() {
        let (_, source) = answering(hourly_limit("5000"));
        source.set_token(Some("t".into()));
        assert_eq!(
            rate_limit_error(&source),
            "GitHub's limit of 5,000 requests an hour is used up. It resets in 12 minutes."
        );
    }

    #[test]
    fn explains_secondary_limits() {
        let (_, source) = answering(Response::new(429, "").with_header("Retry-After", "90"));
        assert_eq!(
            rate_limit_error(&source),
            "GitHub asked the app to slow down. Try again in 2 minutes."
        );
        let (_, source) = answering(Response::new(
            403,
            r#"{"message":"You have exceeded a secondary rate limit."}"#,
        ));
        assert_eq!(
            rate_limit_error(&source),
            "GitHub asked the app to slow down. Try again in 1 minute."
        );
    }

    #[test]
    fn other_forbidden_answers_are_http_errors() {
        let (_, source) = answering(Response::new(
            403,
            r#"{"message":"Repository access blocked"}"#,
        ));
        let err = source
            .latest("o/a", &retail(), Channel::Stable)
            .unwrap_err();
        assert!(matches!(err, SourceError::Http(_)), "{err:?}");
    }

    #[test]
    fn a_rejected_token_is_a_bad_token() {
        let source = source(RecordedHttp::new().respond(
            &releases_url("o/a"),
            401,
            r#"{"message":"Bad credentials"}"#,
        ));
        source.set_token(Some("expired".into()));
        let err = source
            .latest("o/a", &retail(), Channel::Stable)
            .unwrap_err();
        assert!(matches!(err, SourceError::BadToken(_)), "{err:?}");
    }

    const RATE_LIMIT: &str = r#"{"resources": {
        "core": {"limit": 5000, "used": 13, "remaining": 4987, "reset": 1800000720, "resource": "core"},
        "search": {"limit": 30, "used": 0, "remaining": 30, "reset": 1800000060, "resource": "search"}
    }, "rate": {"limit": 5000, "used": 13, "remaining": 4987, "reset": 1800000720, "resource": "core"}}"#;

    #[test]
    fn reads_the_core_rate_limit() {
        let http =
            Arc::new(RecordedHttp::new().respond(&format!("{API}/rate_limit"), 200, RATE_LIMIT));
        let source = GitHubSource::new(http.clone());
        source.set_token(Some("stored".into()));
        let expected = RateLimit {
            limit: 5000,
            remaining: 4987,
            resets_at: 1_800_000_720,
        };
        assert_eq!(source.rate_limit().unwrap(), expected);
        assert_eq!(source.check_token("new").unwrap(), expected);
        assert_eq!(
            header(&http.request_headers(0), "authorization"),
            Some("Bearer stored")
        );
        assert_eq!(
            header(&http.request_headers(1), "authorization"),
            Some("Bearer new")
        );
    }

    #[test]
    fn checking_a_rejected_token_fails() {
        let source = source(RecordedHttp::new().respond(
            &format!("{API}/rate_limit"),
            401,
            r#"{"message":"Bad credentials"}"#,
        ));
        let err = source.check_token("nope").unwrap_err();
        assert!(matches!(err, SourceError::BadToken(_)), "{err:?}");
    }

    #[test]
    fn the_rate_limit_can_be_read_while_blocked() {
        let (http, source) = answering(hourly_limit("60"));
        http.replace(&format!("{API}/rate_limit"), Response::new(200, RATE_LIMIT));
        rate_limit_error(&source);
        assert_eq!(source.rate_limit().unwrap().remaining, 4987);
    }

    #[test]
    fn several_game_versions_share_one_release_list() {
        let http = Arc::new(
            recorded_http(
                "kesava-wow/kuinameplates2",
                "kuinameplates2",
                &["2.29.1", "2.29.0", "2.28.0", "2.27.3a"],
            )
            .header(
                &releases_url("kesava-wow/kuinameplates2"),
                "Cache-Control",
                "private, max-age=60",
            ),
        );
        let source = GitHubSource::new(Arc::new(crate::http::CachedHttp::new(http.clone())));
        for target in [
            retail(),
            target(ERA, Flavor::Classic),
            target(ANNIVERSARY, Flavor::Bcc),
        ] {
            source
                .latest("kesava-wow/kuinameplates2", &target, Channel::Stable)
                .unwrap();
        }
        let requests = http.requests();
        let lists = requests.iter().filter(|url| url.starts_with(API)).count();
        assert_eq!(lists, 1, "{requests:?}");
        // One per release checked, each once.
        assert_eq!(requests.len(), 1 + 4, "{requests:?}");
    }

    #[test]
    fn formats_waits_and_numbers() {
        assert_eq!(minutes(Duration::ZERO), "1 minute");
        assert_eq!(minutes(Duration::from_secs(60)), "1 minute");
        assert_eq!(minutes(Duration::from_secs(61)), "2 minutes");
        assert_eq!(thousands(60), "60");
        assert_eq!(thousands(5000), "5,000");
        assert_eq!(thousands(15000), "15,000");
        assert_eq!(thousands(1234567), "1,234,567");
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
