//! Where the app finds the World of Warcraft installation.
//!
//! The WoW root is resolved in this order:
//! 1. the `WAM_WOW_ROOT` environment variable,
//! 2. `wowRoot` in the settings file,
//! 3. in debug builds only, the `dev-sandbox` folder in the repository.
//!
//! The sandbox default keeps development builds away from the real installation.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sources::Flavor;

pub const WOW_ROOT_ENV: &str = "WAM_WOW_ROOT";
pub const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub wow_root: Option<PathBuf>,
    /// Release flavor to use per client product code, e.g. `{"wowf": "classic"}`, for
    /// clients whose files the interface number doesn't pick correctly.
    pub flavor_overrides: BTreeMap<String, Flavor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WowRootSource {
    Env,
    Settings,
    DevSandbox,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWowRoot {
    pub path: PathBuf,
    pub source: WowRootSource,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine the app config folder")]
    NoConfigDir,
    #[error("could not read settings file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("settings file {path} is not valid: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Loads the settings file. A missing file means default settings.
pub fn load_settings(path: &Path) -> Result<Settings, ConfigError> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(source) => Err(ConfigError::Read {
            path: path.to_owned(),
            source,
        }),
    }
}

/// The `dev-sandbox` folder next to `src-tauri`, only in debug builds.
pub fn dev_sandbox_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|repo| repo.join("dev-sandbox"))
    } else {
        None
    }
}

pub fn resolve_wow_root(
    env_value: Option<OsString>,
    settings: &Settings,
    dev_default: Option<PathBuf>,
) -> Option<ResolvedWowRoot> {
    if let Some(path) = env_value.map(PathBuf::from).and_then(non_empty) {
        return Some(ResolvedWowRoot {
            path,
            source: WowRootSource::Env,
        });
    }
    if let Some(path) = settings.wow_root.clone().and_then(non_empty) {
        return Some(ResolvedWowRoot {
            path,
            source: WowRootSource::Settings,
        });
    }
    dev_default.map(|path| ResolvedWowRoot {
        path,
        source: WowRootSource::DevSandbox,
    })
}

fn non_empty(path: PathBuf) -> Option<PathBuf> {
    (!path.as_os_str().is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with(root: &str) -> Settings {
        Settings {
            wow_root: Some(PathBuf::from(root)),
            ..Settings::default()
        }
    }

    #[test]
    fn env_var_wins_over_settings_and_sandbox() {
        let resolved = resolve_wow_root(
            Some("D:\\Games\\WoW".into()),
            &settings_with("E:\\WoW"),
            Some(PathBuf::from("sandbox")),
        );
        assert_eq!(
            resolved,
            Some(ResolvedWowRoot {
                path: PathBuf::from("D:\\Games\\WoW"),
                source: WowRootSource::Env,
            })
        );
    }

    #[test]
    fn settings_win_over_sandbox() {
        let resolved = resolve_wow_root(
            None,
            &settings_with("E:\\WoW"),
            Some(PathBuf::from("sandbox")),
        );
        assert_eq!(resolved.map(|r| r.source), Some(WowRootSource::Settings));
    }

    #[test]
    fn falls_back_to_sandbox() {
        let resolved = resolve_wow_root(None, &Settings::default(), Some(PathBuf::from("sandbox")));
        assert_eq!(
            resolved,
            Some(ResolvedWowRoot {
                path: PathBuf::from("sandbox"),
                source: WowRootSource::DevSandbox,
            })
        );
    }

    #[test]
    fn empty_values_are_ignored() {
        let resolved = resolve_wow_root(
            Some(OsString::new()),
            &settings_with(""),
            Some(PathBuf::from("sandbox")),
        );
        assert_eq!(resolved.map(|r| r.source), Some(WowRootSource::DevSandbox));
    }

    #[test]
    fn nothing_configured_resolves_to_none() {
        assert_eq!(resolve_wow_root(None, &Settings::default(), None), None);
    }

    #[test]
    fn dev_sandbox_is_inside_the_repository() {
        let root = dev_sandbox_root().expect("tests run as a debug build");
        assert!(root.ends_with("dev-sandbox"));
        assert!(root.parent().unwrap().join("src-tauri").is_dir());
    }

    #[test]
    fn missing_settings_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let settings = load_settings(&dir.path().join(SETTINGS_FILE)).unwrap();
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn reads_wow_root_from_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        fs::write(&path, r#"{ "wowRoot": "F:\\World of Warcraft" }"#).unwrap();
        assert_eq!(
            load_settings(&path).unwrap(),
            settings_with("F:\\World of Warcraft")
        );
    }

    #[test]
    fn reads_flavor_overrides_from_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        fs::write(
            &path,
            r#"{ "flavorOverrides": { "wowf": "classic", "wow_anniversary": "bcc" } }"#,
        )
        .unwrap();
        let settings = load_settings(&path).unwrap();
        assert_eq!(
            settings.flavor_overrides.get("wowf"),
            Some(&Flavor::Classic)
        );
        assert_eq!(
            settings.flavor_overrides.get("wow_anniversary"),
            Some(&Flavor::Bcc)
        );
    }

    #[test]
    fn unknown_flavors_make_the_settings_file_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        fs::write(&path, r#"{ "flavorOverrides": { "wowf": "titan" } }"#).unwrap();
        assert!(matches!(
            load_settings(&path),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn invalid_settings_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        fs::write(&path, "{ not json").unwrap();
        let err = load_settings(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains(SETTINGS_FILE));
    }
}
