//! The tracking file: which addons the app manages, per game client (ADR 0004).
//!
//! It lives in the app config folder as `state.json`. Saving writes a temporary file and renames
//! it over the old one, so a crash mid-save never leaves a half-written file. A file that can't
//! be read is reported as an error and left untouched, never replaced by an empty state.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const STATE_FILE: &str = "state.json";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub schema_version: u32,
    #[serde(default)]
    pub clients: Vec<ClientState>,
}

/// One game client. It is identified by its full path, so different World of Warcraft folders
/// (such as the development sandbox and the real installation) never share records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientState {
    pub path: PathBuf,
    #[serde(default)]
    pub addons: Vec<ManagedAddon>,
    /// Unmanaged folders the user chose to ignore.
    #[serde(default)]
    pub ignored_folders: Vec<String>,
}

/// An addon the app installed. `source` and `project` together identify it within a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAddon {
    /// The source's id, e.g. `github`.
    pub source: String,
    /// The addon's id within its source, e.g. `BigWigsMods/BigWigs` on GitHub.
    pub project: String,
    pub install_mode: InstallMode,
    #[serde(default)]
    pub channel: Channel,
    /// The release tag for release installs, the commit hash for clones.
    pub version: String,
    /// The folders this addon installed in `Interface\AddOns`: the only ones the app may change.
    pub folders: Vec<String>,
    /// Pinned addons are skipped by updates.
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallMode {
    Release,
    Clone,
}

/// Release channels, ordered from most to least stable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
    Alpha,
}

impl Channel {
    /// Whether an addon following this channel accepts a release from `release`:
    /// beta also takes stable releases, alpha takes everything.
    pub fn includes(self, release: Channel) -> bool {
        release <= self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("could not read the tracking file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the tracking file {path} is damaged and was left untouched: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "the tracking file {path} has schema version {found}, which this version of the app can't read; it was left untouched"
    )]
    UnsupportedVersion { path: PathBuf, found: u32 },
    #[error("could not save the tracking file {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            clients: Vec::new(),
        }
    }
}

impl State {
    pub fn client(&self, path: &Path) -> Option<&ClientState> {
        self.clients.iter().find(|c| same_path(&c.path, path))
    }

    /// The client's records, added empty if there are none yet.
    pub fn client_mut(&mut self, path: &Path) -> &mut ClientState {
        let index = match self.clients.iter().position(|c| same_path(&c.path, path)) {
            Some(index) => index,
            None => {
                self.clients.push(ClientState {
                    path: path.to_owned(),
                    addons: Vec::new(),
                    ignored_folders: Vec::new(),
                });
                self.clients.len() - 1
            }
        };
        &mut self.clients[index]
    }
}

impl ClientState {
    pub fn addon(&self, source: &str, project: &str) -> Option<&ManagedAddon> {
        self.addons
            .iter()
            .find(|a| a.source == source && a.project == project)
    }

    /// The managed addon that owns `folder`. Folder names compare case-insensitively, like
    /// Windows does.
    pub fn owner_of(&self, folder: &str) -> Option<&ManagedAddon> {
        self.addons
            .iter()
            .find(|a| a.folders.iter().any(|f| same_folder(f, folder)))
    }

    /// Adds `addon`, replacing the record with the same source and project.
    pub fn upsert(&mut self, addon: ManagedAddon) {
        match self
            .addons
            .iter_mut()
            .find(|a| a.source == addon.source && a.project == addon.project)
        {
            Some(existing) => *existing = addon,
            None => self.addons.push(addon),
        }
    }

    pub fn remove(&mut self, source: &str, project: &str) -> Option<ManagedAddon> {
        let index = self
            .addons
            .iter()
            .position(|a| a.source == source && a.project == project)?;
        Some(self.addons.remove(index))
    }
}

/// Loads the tracking file. A missing file means nothing is managed yet.
pub fn load(path: &Path) -> Result<State, StateError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(State::default()),
        Err(source) => {
            return Err(StateError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let parse_error = |source| StateError::Parse {
        path: path.to_owned(),
        source,
    };

    // Check the version first, so a newer file gets a clear message rather than a parse error.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        schema_version: u32,
    }
    let header: Header = serde_json::from_str(text).map_err(parse_error)?;
    if header.schema_version != SCHEMA_VERSION {
        return Err(StateError::UnsupportedVersion {
            path: path.to_owned(),
            found: header.schema_version,
        });
    }
    serde_json::from_str(text).map_err(parse_error)
}

/// Saves the tracking file atomically: the old file stays intact until the new one is complete.
pub fn save(path: &Path, state: &State) -> Result<(), StateError> {
    let write_error = |source| StateError::Write {
        path: path.to_owned(),
        source,
    };
    let mut json = serde_json::to_string_pretty(state).map_err(|e| write_error(e.into()))?;
    json.push('\n');

    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(write_error)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);

    let result = write_synced(&temp, json.as_bytes()).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(write_error)
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Windows paths compare case-insensitively and ignore the separator style and trailing
/// separators.
fn same_path(a: &Path, b: &Path) -> bool {
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    normalize(a) == normalize(b)
}

fn same_folder(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const RETAIL: &str = "F:\\World of Warcraft\\_retail_";

    fn bigwigs() -> ManagedAddon {
        ManagedAddon {
            source: "github".into(),
            project: "BigWigsMods/BigWigs".into(),
            install_mode: InstallMode::Release,
            channel: Channel::Stable,
            version: "v427.8".into(),
            folders: vec!["BigWigs".into(), "BigWigs_Core".into()],
            pinned: false,
        }
    }

    fn sample_state() -> State {
        let mut state = State::default();
        let retail = state.client_mut(Path::new(RETAIL));
        retail.upsert(bigwigs());
        retail.upsert(ManagedAddon {
            source: "git".into(),
            project: "https://example.com/addon.git".into(),
            install_mode: InstallMode::Clone,
            channel: Channel::Alpha,
            version: "0123456789abcdef0123456789abcdef01234567".into(),
            folders: vec!["Addon".into()],
            pinned: true,
        });
        retail.ignored_folders.push("ManualAddon".into());
        state
            .client_mut(Path::new("F:\\World of Warcraft\\_classic_era_"))
            .upsert(bigwigs());
        state
    }

    #[test]
    fn channels_include_the_more_stable_ones() {
        use Channel::*;
        assert!(Stable.includes(Stable));
        assert!(!Stable.includes(Beta));
        assert!(!Stable.includes(Alpha));
        assert!(Beta.includes(Stable));
        assert!(Beta.includes(Beta));
        assert!(!Beta.includes(Alpha));
        assert!(Alpha.includes(Stable));
        assert!(Alpha.includes(Beta));
        assert!(Alpha.includes(Alpha));
    }

    #[test]
    fn round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config").join(STATE_FILE);
        let state = sample_state();
        save(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap(), state);
    }

    #[test]
    fn writes_the_documented_json_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        save(&path, &sample_state()).unwrap();
        let json: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["schemaVersion"], SCHEMA_VERSION);
        assert_eq!(json["clients"][0]["path"], RETAIL);
        assert_eq!(json["clients"][0]["ignoredFolders"], json!(["ManualAddon"]));
        assert_eq!(
            json["clients"][0]["addons"][0],
            json!({
                "source": "github",
                "project": "BigWigsMods/BigWigs",
                "installMode": "release",
                "channel": "stable",
                "version": "v427.8",
                "folders": ["BigWigs", "BigWigs_Core"],
                "pinned": false
            })
        );
        assert_eq!(json["clients"][0]["addons"][1]["installMode"], "clone");
        assert_eq!(json["clients"][0]["addons"][1]["channel"], "alpha");
    }

    #[test]
    fn optional_fields_have_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        let minimal = json!({
            "schemaVersion": 1,
            "clients": [{
                "path": RETAIL,
                "addons": [{
                    "source": "github",
                    "project": "BigWigsMods/BigWigs",
                    "installMode": "release",
                    "version": "v427.8",
                    "folders": ["BigWigs", "BigWigs_Core"]
                }]
            }]
        });
        fs::write(&path, format!("\u{feff}{minimal}")).unwrap();
        let state = load(&path).unwrap();
        let retail = state.client(Path::new(RETAIL)).unwrap();
        assert_eq!(retail.addons, [bigwigs()]);
        assert!(retail.ignored_folders.is_empty());
    }

    #[test]
    fn missing_file_means_nothing_is_managed() {
        let dir = tempfile::tempdir().unwrap();
        let state = load(&dir.path().join(STATE_FILE)).unwrap();
        assert_eq!(state, State::default());
        assert_eq!(state.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn damaged_files_are_errors_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        let damaged = [
            "{ \"schemaVersion\": 1, \"clients\": [",
            "",
            "{}",
            "{ \"clients\": [] }",
            "{ \"schemaVersion\": 1, \"clients\": [{ \"addons\": [] }] }",
            "{ \"schemaVersion\": 1, \"clients\": [{ \"path\": \"x\", \"addons\": [{ \"source\": \"github\" }] }] }",
        ];
        for text in damaged {
            fs::write(&path, text).unwrap();
            let err = load(&path).unwrap_err();
            assert!(matches!(err, StateError::Parse { .. }), "{text}: {err}");
            assert!(err.to_string().contains(STATE_FILE), "{err}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn other_schema_versions_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        let newer = "{ \"schemaVersion\": 2, \"somethingNew\": true }";
        fs::write(&path, newer).unwrap();
        let err = load(&path).unwrap_err();
        assert!(
            matches!(err, StateError::UnsupportedVersion { found: 2, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("schema version 2"));
        assert_eq!(fs::read_to_string(&path).unwrap(), newer);
    }

    #[test]
    fn saving_replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        save(&path, &sample_state()).unwrap();
        save(&path, &State::default()).unwrap();
        assert_eq!(load(&path).unwrap(), State::default());
        let files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(files, [STATE_FILE]);
    }

    #[test]
    fn a_failed_save_keeps_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        save(&path, &sample_state()).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        // A folder where the temp file should go makes the write fail.
        fs::create_dir(dir.path().join(format!("{STATE_FILE}.tmp"))).unwrap();
        let err = save(&path, &State::default()).unwrap_err();
        assert!(matches!(err, StateError::Write { .. }), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn clients_are_found_by_path_regardless_of_case_and_separators() {
        let mut state = sample_state();
        for path in [
            RETAIL,
            "f:\\world of warcraft\\_RETAIL_\\",
            "F:/World of Warcraft/_retail_",
        ] {
            assert!(state.client(Path::new(path)).is_some(), "{path}");
        }
        assert!(
            state
                .client(Path::new("F:\\dev\\sandbox\\_retail_"))
                .is_none()
        );

        state.client_mut(Path::new("f:/world of warcraft/_retail_/"));
        state.client_mut(Path::new("F:\\dev\\sandbox\\_retail_"));
        assert_eq!(state.clients.len(), 3);
    }

    #[test]
    fn finds_owners_and_replaces_or_removes_records() {
        let mut state = sample_state();
        let retail = state.client_mut(Path::new(RETAIL));
        assert_eq!(
            retail.owner_of("bigwigs_core").map(|a| a.project.as_str()),
            Some("BigWigsMods/BigWigs")
        );
        assert!(retail.owner_of("BigWigs_Options").is_none());

        let mut updated = bigwigs();
        updated.version = "v428".into();
        updated.folders.push("BigWigs_Options".into());
        retail.upsert(updated.clone());
        assert_eq!(retail.addons.len(), 2);
        assert_eq!(
            retail.addon("github", "BigWigsMods/BigWigs"),
            Some(&updated)
        );
        assert!(retail.owner_of("BigWigs_Options").is_some());

        assert_eq!(
            retail.remove("github", "BigWigsMods/BigWigs"),
            Some(updated)
        );
        assert!(retail.owner_of("BigWigs").is_none());
        assert_eq!(retail.remove("github", "BigWigsMods/BigWigs"), None);
    }
}
