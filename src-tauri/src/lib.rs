pub mod clients;
mod config;
pub mod toc;

use config::{ConfigError, ResolvedWowRoot};
use tauri::Manager;

#[tauri::command]
fn wow_root(app: tauri::AppHandle) -> Result<Option<ResolvedWowRoot>, ConfigError> {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![wow_root])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
