pub mod commands;
pub mod pipeline;
pub mod state;
pub mod video_protocol;
pub mod video_server;
pub mod watcher;

use tauri::Manager;

#[cfg(target_os = "windows")]
use window_vibrancy::apply_acrylic;

#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        // localvideo:// is used only for thumbnails (small JPEGs). Videos are
        // served by the axum HTTP server to get real range-request streaming.
        .register_asynchronous_uri_scheme_protocol("localvideo", |_app, request, responder| {
            tauri::async_runtime::spawn(async move {
                video_protocol::handle(request, responder).await;
            });
        })
        .manage(state::AppStateHandle::new())
        .manage(pipeline::PipelineHandle::new())
        .invoke_handler(tauri::generate_handler![
            commands::store_get,
            commands::store_set,
            commands::store_get_all,
            commands::store_get_folder_thumb,
            commands::fs_read_videos,
            commands::fs_read_video_entries,
            commands::dialog_open_folder,
            commands::shell_show_in_folder,
            commands::shell_copy_path,
            commands::pipeline_cancel,
            commands::pipeline_process,
            commands::get_video_server_port,
        ])
        .setup(|app| {
            let win = app.get_webview_window("main").unwrap();

            #[cfg(target_os = "windows")]
            apply_acrylic(&win, Some((18, 18, 18, 125))).ok();

            #[cfg(target_os = "macos")]
            apply_vibrancy(&win, NSVisualEffectMaterial::HudWindow, None, None).ok();

            // Load persisted state synchronously before the frontend can invoke
            // store_get_all. Without this, a fast startup could race the async
            // load and initialize the OnceCell with an empty default, losing
            // folder history and lastFolder.
            let state = app.state::<state::AppStateHandle>().inner().clone();
            tauri::async_runtime::block_on(async move {
                state.load().await;
            });

            let server_state =
                tauri::async_runtime::block_on(async { video_server::start_video_server().await });

            eprintln!("[setup] Video server port: {}", server_state.port());
            app.manage(server_state);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
