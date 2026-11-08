mod addons;
mod clients;
mod commands;
mod config;
// Public until the install and update commands use them.
pub mod http;
mod secrets;
pub mod sources;
pub mod state;
mod toc;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(commands::AppState::load()?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::wow_root,
            commands::list_clients,
            commands::list_addons,
            commands::github_token_stored,
            commands::github_rate_limit,
            commands::save_github_token,
            commands::remove_github_token
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
