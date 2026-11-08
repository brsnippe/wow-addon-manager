mod addons;
mod clients;
mod commands;
mod config;
// Public until the install and update commands use them.
pub mod http;
pub mod sources;
pub mod state;
mod toc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::wow_root,
            commands::list_clients,
            commands::list_addons
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
