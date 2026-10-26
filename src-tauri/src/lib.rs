mod commands;
mod desktop;
mod error;
#[cfg(target_os = "macos")]
mod macos;
#[cfg_attr(test, allow(dead_code))]
mod security;
mod sync;
mod vault;

use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            desktop::show_main(app);
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .on_window_event(|_window, _event| {
            desktop::on_window_event(_window, _event);
            #[cfg(target_os = "macos")]
            macos::on_window_event(_window, _event);
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("main") {
                macos::initialize(&window)?;
            }
            let data_dir = app.path().app_data_dir()?;
            #[cfg(debug_assertions)]
            let data_dir = std::env::var_os("KVAULT_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(data_dir);
            let storage = vault::storage::Storage::new(data_dir)?;
            let state = Arc::new(vault::AppState::new(storage));
            app.manage(state.clone());
            desktop::setup(app.handle())?;
            sync::service::start_worker(state, app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::get_preferences,
            commands::set_theme,
            commands::set_close_settings,
            commands::minimize_to_tray,
            commands::exit_app,
            commands::set_system_unlock,
            commands::unlock_with_system,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::touch_vault,
            commands::mutate_vault,
            commands::test_connection,
            commands::connect_remote,
            commands::sync_now,
            commands::copy_value,
            commands::copy_key,
            commands::select_import_csv,
            commands::preview_import,
            commands::change_password,
            commands::migrate_password,
            commands::export_backup,
            commands::import_backup
        ])
        .build(tauri::generate_context!())
        .expect("failed to initialize KVault")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = &event
            {
                api.prevent_exit();
                desktop::request(app, true);
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                desktop::show_main(app);
            }
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app.try_state::<vault::Shared>()
            {
                let _ = state.lock();
            }
        });
}
