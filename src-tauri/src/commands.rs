//! The Tauri commands the frontend calls.
//!
//! Every command returns errors as a [`CommandError`] (`{ kind, message }`), never a panic. The
//! matching TypeScript types live in `src/lib/types.ts`.

use std::sync::Arc;

use serde::Serialize;
use tauri::{Manager, State};

use crate::addons::{self, InstalledAddon};
use crate::clients::{self, ClientError, GameClient};
use crate::config::{self, ConfigError, ResolvedWowRoot};
use crate::http::{CachedHttp, Http, UreqHttp};
use crate::secrets::{CredentialManager, GITHUB_TOKEN, SecretError, SecretStore};
use crate::sources::SourceError;
use crate::sources::github::{GitHubSource, RateLimit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// No World of Warcraft folder is configured.
    NoWowRoot,
    /// The configured World of Warcraft folder doesn't exist.
    WowRootMissing,
    /// No game client lives in the requested folder.
    UnknownClient,
    /// The settings file can't be read or parsed.
    Config,
    /// A file or folder can't be read.
    Io,
    /// A server can't be reached or gave an unexpected answer.
    Network,
    /// A server's rate limit is used up; the message says when to try again.
    RateLimited,
    /// A server didn't accept the token, or the token is malformed.
    BadToken,
    /// Windows Credential Manager failed.
    Credentials,
    /// The addon, release or file wasn't found on its source.
    Source,
    /// A bug in the app, such as a crashed background task.
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    pub kind: ErrorKind,
    pub message: String,
}

impl CommandError {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<ConfigError> for CommandError {
    fn from(err: ConfigError) -> Self {
        Self::new(ErrorKind::Config, err.to_string())
    }
}

impl From<ClientError> for CommandError {
    fn from(err: ClientError) -> Self {
        let kind = match err {
            ClientError::RootMissing(_) => ErrorKind::WowRootMissing,
            ClientError::Read { .. } => ErrorKind::Io,
        };
        Self::new(kind, err.to_string())
    }
}

impl From<SourceError> for CommandError {
    fn from(err: SourceError) -> Self {
        let kind = match err {
            SourceError::NotFound(_)
            | SourceError::NoRelease(_)
            | SourceError::NoFileForClient(_)
            | SourceError::NeedsKey(_)
            | SourceError::Unsupported(_) => ErrorKind::Source,
            SourceError::RateLimited(_) => ErrorKind::RateLimited,
            SourceError::BadToken(_) => ErrorKind::BadToken,
            SourceError::Http(_) => ErrorKind::Network,
            SourceError::Io(_) => ErrorKind::Io,
        };
        Self::new(kind, err.to_string())
    }
}

impl From<SecretError> for CommandError {
    fn from(err: SecretError) -> Self {
        Self::new(ErrorKind::Credentials, err.to_string())
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// Services that live as long as the app, shared by the commands.
#[derive(Clone)]
pub struct AppState {
    github: Arc<GitHubSource>,
    secrets: Arc<dyn SecretStore>,
}

impl AppState {
    /// The real services, with the GitHub token from Windows Credential Manager.
    pub fn load() -> Result<Self, SecretError> {
        Ok(Self::new(
            Arc::new(CachedHttp::new(UreqHttp::new())),
            Arc::new(CredentialManager::new()?),
        ))
    }

    fn new(http: Arc<dyn Http>, secrets: Arc<dyn SecretStore>) -> Self {
        let github = Arc::new(GitHubSource::new(http));
        match secrets.get(GITHUB_TOKEN) {
            Ok(token) => github.set_token(token),
            // The app still works without it, at the lower rate limit.
            Err(err) => eprintln!("could not load the GitHub token: {err}"),
        }
        Self { github, secrets }
    }

    /// Checks the token with GitHub before storing it, so a typo isn't saved.
    fn save_github_token(&self, token: &str) -> CommandResult<RateLimit> {
        let token = token.trim();
        if token.is_empty() {
            return Err(CommandError::new(
                ErrorKind::BadToken,
                "Paste a token first.",
            ));
        }
        if !token.chars().all(|c| c.is_ascii_graphic()) {
            return Err(CommandError::new(
                ErrorKind::BadToken,
                "That doesn't look like a GitHub token: it contains spaces or other characters tokens don't have.",
            ));
        }
        let limit = self.github.check_token(token)?;
        self.secrets.set(GITHUB_TOKEN, token)?;
        self.github.set_token(Some(token.to_string()));
        Ok(limit)
    }

    fn remove_github_token(&self) -> CommandResult<()> {
        self.secrets.delete(GITHUB_TOKEN)?;
        self.github.set_token(None);
        Ok(())
    }
}

/// The World of Warcraft folder and where its location came from, or `None` if none is set.
#[tauri::command]
pub fn wow_root(app: tauri::AppHandle) -> CommandResult<Option<ResolvedWowRoot>> {
    resolve_wow_root(&app)
}

/// The game clients installed in the World of Warcraft folder.
#[tauri::command]
pub async fn list_clients(app: tauri::AppHandle) -> CommandResult<Vec<GameClient>> {
    let root = resolve_wow_root(&app)?;
    blocking(move || clients_in(root.as_ref())).await
}

/// The addons installed for the client in `folder` (e.g. `_retail_`), sorted by title.
#[tauri::command]
pub async fn list_addons(
    app: tauri::AppHandle,
    folder: String,
) -> CommandResult<Vec<InstalledAddon>> {
    let root = resolve_wow_root(&app)?;
    blocking(move || addons_in(root.as_ref(), &folder)).await
}

/// Whether a GitHub token is stored. The token itself never leaves the backend.
#[tauri::command]
pub fn github_token_stored(state: State<'_, AppState>) -> bool {
    state.github.has_token()
}

/// GitHub's current API rate limit, for the stored token or anonymous use.
#[tauri::command]
pub async fn github_rate_limit(state: State<'_, AppState>) -> CommandResult<RateLimit> {
    let github = state.github.clone();
    blocking(move || Ok(github.rate_limit()?)).await
}

/// Checks `token` with GitHub, then stores it in Windows Credential Manager and uses it.
#[tauri::command]
pub async fn save_github_token(
    state: State<'_, AppState>,
    token: String,
) -> CommandResult<RateLimit> {
    let state = state.inner().clone();
    blocking(move || state.save_github_token(&token)).await
}

#[tauri::command]
pub async fn remove_github_token(state: State<'_, AppState>) -> CommandResult<()> {
    let state = state.inner().clone();
    blocking(move || state.remove_github_token()).await
}

fn resolve_wow_root(app: &tauri::AppHandle) -> CommandResult<Option<ResolvedWowRoot>> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|_| ConfigError::NoConfigDir)?;
    let settings = config::load_settings(&config_dir.join(config::SETTINGS_FILE))?;
    Ok(config::resolve_wow_root(
        std::env::var_os(config::WOW_ROOT_ENV),
        &settings,
        config::dev_sandbox_root(),
    ))
}

/// Runs disk-heavy work off the async runtime's worker threads.
async fn blocking<T, F>(task: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| {
            CommandError::new(
                ErrorKind::Internal,
                format!("background task failed: {err}"),
            )
        })?
}

fn clients_in(root: Option<&ResolvedWowRoot>) -> CommandResult<Vec<GameClient>> {
    let root = root.ok_or_else(|| {
        CommandError::new(
            ErrorKind::NoWowRoot,
            "no World of Warcraft folder is configured",
        )
    })?;
    Ok(clients::detect_clients(&root.path)?)
}

/// Looks the client up among the detected ones, so `folder` can never point outside the
/// World of Warcraft folder.
fn addons_in(root: Option<&ResolvedWowRoot>, folder: &str) -> CommandResult<Vec<InstalledAddon>> {
    let client = clients_in(root)?
        .into_iter()
        .find(|client| client.folder == folder)
        .ok_or_else(|| {
            CommandError::new(
                ErrorKind::UnknownClient,
                format!("no game client in folder {folder}"),
            )
        })?;
    addons::list_addons(&client).map_err(|err| {
        CommandError::new(
            ErrorKind::Io,
            format!(
                "could not read the addons of {}: {err}",
                client.display_name
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::GameType;
    use crate::config::WowRootSource;
    use crate::http::recorded::RecordedHttp;
    use crate::secrets::MemoryStore;
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};

    fn root_at(path: PathBuf) -> ResolvedWowRoot {
        ResolvedWowRoot {
            path,
            source: WowRootSource::Env,
        }
    }

    fn fixture_root() -> ResolvedWowRoot {
        root_at(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wow"))
    }

    fn keys(value: &Value) -> Vec<&str> {
        let mut keys: Vec<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    #[test]
    fn lists_the_fixture_clients() {
        let clients = clients_in(Some(&fixture_root())).unwrap();
        let folders: Vec<_> = clients.iter().map(|c| c.folder.as_str()).collect();
        assert_eq!(folders, ["_retail_", "_classic_era_", "_forever_"]);
    }

    #[test]
    fn lists_the_addons_of_one_client() {
        let addons = addons_in(Some(&fixture_root()), "_classic_era_").unwrap();
        let names: Vec<_> = addons.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["EraOnly", "MultiToc", "SimpleAddon"]);
    }

    #[test]
    fn missing_configuration_is_an_error() {
        let err = clients_in(None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NoWowRoot);
        assert_eq!(
            addons_in(None, "_retail_").unwrap_err().kind,
            ErrorKind::NoWowRoot
        );
    }

    #[test]
    fn missing_wow_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = clients_in(Some(&root_at(dir.path().join("gone")))).unwrap_err();
        assert_eq!(err.kind, ErrorKind::WowRootMissing);
        assert!(err.message.contains("gone"));
    }

    #[test]
    fn only_detected_clients_can_be_listed() {
        for folder in ["_ptr_", "", "..", "_retail_/../_forever_", "Interface"] {
            let err = addons_in(Some(&fixture_root()), folder).unwrap_err();
            assert_eq!(err.kind, ErrorKind::UnknownClient, "{folder}");
        }
    }

    #[test]
    fn errors_serialize_as_kind_and_message() {
        let err = CommandError::new(ErrorKind::WowRootMissing, "gone");
        assert_eq!(
            serde_json::to_value(err).unwrap(),
            json!({ "kind": "wowRootMissing", "message": "gone" })
        );
        let config = CommandError::from(ConfigError::NoConfigDir);
        assert_eq!(serde_json::to_value(config).unwrap()["kind"], "config");
    }

    /// Guards the hand-written TypeScript types in `src/lib/types.ts`.
    #[test]
    fn json_matches_the_typescript_types() {
        let clients = clients_in(Some(&fixture_root())).unwrap();
        let client = serde_json::to_value(&clients[0]).unwrap();
        assert_eq!(
            keys(&client),
            [
                "displayName",
                "folder",
                "gameType",
                "interface",
                "path",
                "product",
                "version"
            ]
        );
        assert_eq!(client["gameType"], "standard");
        let game_types: Vec<_> = [
            GameType::Standard,
            GameType::Forever,
            GameType::Vanilla,
            GameType::Tbc,
            GameType::Wrath,
            GameType::Cata,
            GameType::Mists,
        ]
        .iter()
        .map(|g| serde_json::to_value(g).unwrap())
        .collect();
        assert_eq!(
            game_types,
            [
                "standard", "forever", "vanilla", "tbc", "wrath", "cata", "mists"
            ]
        );

        let addons = addons_in(Some(&fixture_root()), "_retail_").unwrap();
        assert_eq!(
            keys(&serde_json::to_value(&addons[0]).unwrap()),
            [
                "author",
                "curseProjectId",
                "folders",
                "interfaces",
                "name",
                "notes",
                "title",
                "version",
                "wagoId",
                "website",
                "wowiId"
            ]
        );

        let root = serde_json::to_value(fixture_root()).unwrap();
        assert_eq!(keys(&root), ["path", "source"]);
        let sources: Vec<_> = [
            WowRootSource::Env,
            WowRootSource::Settings,
            WowRootSource::DevSandbox,
        ]
        .iter()
        .map(|s| serde_json::to_value(s).unwrap())
        .collect();
        assert_eq!(sources, ["env", "settings", "devSandbox"]);

        let limit = RateLimit {
            limit: 60,
            remaining: 59,
            resets_at: 1,
        };
        assert_eq!(
            keys(&serde_json::to_value(limit).unwrap()),
            ["limit", "remaining", "resetsAt"]
        );

        let kinds: Vec<_> = [
            ErrorKind::NoWowRoot,
            ErrorKind::WowRootMissing,
            ErrorKind::UnknownClient,
            ErrorKind::Config,
            ErrorKind::Io,
            ErrorKind::Network,
            ErrorKind::RateLimited,
            ErrorKind::BadToken,
            ErrorKind::Credentials,
            ErrorKind::Source,
            ErrorKind::Internal,
        ]
        .iter()
        .map(|k| serde_json::to_value(k).unwrap())
        .collect();
        assert_eq!(
            kinds,
            [
                "noWowRoot",
                "wowRootMissing",
                "unknownClient",
                "config",
                "io",
                "network",
                "rateLimited",
                "badToken",
                "credentials",
                "source",
                "internal"
            ]
        );
    }

    const RATE_LIMIT_URL: &str = "https://api.github.com/rate_limit";
    const RATE_LIMIT: &str =
        r#"{"resources": {"core": {"limit": 5000, "remaining": 5000, "reset": 1800000000}}}"#;

    fn app_state(http: RecordedHttp, secrets: &Arc<MemoryStore>) -> AppState {
        AppState::new(Arc::new(http), secrets.clone())
    }

    #[test]
    fn uses_the_stored_github_token() {
        let secrets = Arc::new(MemoryStore::default());
        assert!(!app_state(RecordedHttp::new(), &secrets).github.has_token());
        secrets.set(GITHUB_TOKEN, "stored").unwrap();
        assert!(app_state(RecordedHttp::new(), &secrets).github.has_token());
    }

    #[test]
    fn saves_a_token_github_accepts() {
        let secrets = Arc::new(MemoryStore::default());
        let state = app_state(
            RecordedHttp::new().respond(RATE_LIMIT_URL, 200, RATE_LIMIT),
            &secrets,
        );
        let limit = state.save_github_token("  github_pat_abc123\r\n").unwrap();
        assert_eq!(limit.limit, 5000);
        assert_eq!(
            secrets.get(GITHUB_TOKEN).unwrap().as_deref(),
            Some("github_pat_abc123")
        );
        assert!(state.github.has_token());

        state.remove_github_token().unwrap();
        assert_eq!(secrets.get(GITHUB_TOKEN).unwrap(), None);
        assert!(!state.github.has_token());
    }

    #[test]
    fn does_not_save_a_token_github_rejects() {
        let secrets = Arc::new(MemoryStore::default());
        let state = app_state(
            RecordedHttp::new().respond(RATE_LIMIT_URL, 401, r#"{"message":"Bad credentials"}"#),
            &secrets,
        );
        let err = state.save_github_token("ghp_typo").unwrap_err();
        assert_eq!(err.kind, ErrorKind::BadToken);
        assert_eq!(secrets.get(GITHUB_TOKEN).unwrap(), None);
        assert!(!state.github.has_token());
    }

    #[test]
    fn does_not_send_malformed_tokens() {
        let secrets = Arc::new(MemoryStore::default());
        let http = Arc::new(RecordedHttp::new());
        let state = AppState::new(http.clone(), secrets.clone());
        for token in ["", "   ", "ghp one", "ghp_ä"] {
            let err = state.save_github_token(token).unwrap_err();
            assert_eq!(err.kind, ErrorKind::BadToken, "{token:?}");
        }
        assert!(http.requests().is_empty());
    }

    #[test]
    fn source_errors_keep_their_message() {
        let err = CommandError::from(SourceError::RateLimited("wait".into()));
        assert_eq!(err, CommandError::new(ErrorKind::RateLimited, "wait"));
        let err = CommandError::from(SourceError::NotFound("o/a".into()));
        assert_eq!(
            err,
            CommandError::new(ErrorKind::Source, "o/a was not found")
        );
    }
}
