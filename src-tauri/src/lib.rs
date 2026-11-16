mod clipboard;
mod commands;
mod image_output;
mod lifecycle;
mod queue;
mod recovery;
mod storage;
use tauri::Manager;

/// Acquire the single-instance guard before opening SQLite, then install guarded close and quit paths.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(lifecycle::ExitState::default())
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            app.manage(queue::Storage::new(storage::Store::open(&directory)?)?);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !window
                    .app_handle()
                    .state::<lifecycle::ExitState>()
                    .approved()
                {
                    api.prevent_close();
                    lifecycle::request(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            clipboard::paste_image,
            image_output::copy_image,
            image_output::export_image,
            commands::create_note,
            commands::load_note,
            commands::update_note,
            commands::list_notes,
            commands::put_attachment,
            commands::load_attachment,
            commands::load_session,
            commands::save_session,
            lifecycle::request_exit,
            lifecycle::exit_ready,
            lifecycle::cancel_exit,
            lifecycle::finish_exit,
            recovery::export_recovery
        ])
        .build(tauri::generate_context!())
        .expect("Unable to start simple-note; the existing database has been preserved");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !app.state::<lifecycle::ExitState>().approved() {
                api.prevent_exit();
                lifecycle::request(app);
            }
        }
    });
}
