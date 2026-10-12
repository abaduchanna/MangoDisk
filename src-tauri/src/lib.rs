#[cfg(target_os = "android")]
pub mod mobile_cleaner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_store::Builder::default().build());

    // Android Edition: register the real junk-scanner / cleaner commands.
    // The mobile.html frontend (see dist/) calls these via __TAURI_INTERNALS__.
    #[cfg(target_os = "android")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        mobile_cleaner::mobile_storage_overview,
        mobile_cleaner::mobile_scan_start,
        mobile_cleaner::mobile_scan_status,
        mobile_cleaner::mobile_clean_files,
        mobile_cleaner::mobile_clean_app_cache
    ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
