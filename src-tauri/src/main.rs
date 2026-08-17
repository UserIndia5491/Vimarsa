#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod analyzer;
use analyzer::{analyze_repository, RepoReport};
use std::sync::Mutex;
use tauri::State;
struct AppState { last_report: Mutex<Option<RepoReport>> }
#[tauri::command]
fn analyze(path: String, state: State<'_, AppState>) -> Result<RepoReport, String> {
    let report = analyze_repository(&path).map_err(|e| e.to_string())?;
    *state.last_report.lock().map_err(|_| "state lock poisoned".to_string())? = Some(report.clone());
    Ok(report)
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { last_report: Mutex::new(None) })
        .invoke_handler(tauri::generate_handler![analyze])
        .run(tauri::generate_context!())
        .expect("error while running Vimarśa");
}
