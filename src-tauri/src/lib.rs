#[cfg(test)]
#[allow(dead_code)]
mod backup;
mod clipboard;
#[cfg(test)]
#[allow(dead_code)]
mod commands;
mod files;
mod image_output;
mod lifecycle;
#[cfg(test)]
#[allow(dead_code)]
mod queue;
#[cfg(test)]
#[allow(dead_code)]
mod recovery;
#[allow(dead_code)] // Legacy data remains untouched; its regression tests stay available.
mod storage;
use tauri::Manager;

/// One instance owns the recovery files; saved documents stay in their chosen locations.
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
            app.manage(files::Files(std::sync::Mutex::new(files::FileStore::new(
                directory,
            )?)));
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
            image_output::copy_image,
            image_output::export_image,
            files::file_operation,
            files::file_picker,
            clipboard::file_clipboard,
            clipboard::clipboard_read_text,
            clipboard::clipboard_write_text,
            lifecycle::request_exit,
            lifecycle::exit_ready,
            lifecycle::cancel_exit,
            lifecycle::finish_exit
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
