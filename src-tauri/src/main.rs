#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analyzer;
mod github;
mod groq;

use analyzer::{analyze_repository, RepoReport};
use groq::explain_repository;
use serde_json::json;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

struct AppState {
    last_report: Mutex<Option<RepoReport>>,
}

fn emit_progress(app: &AppHandle, stage: &str, message: &str) {
    let _ = app.emit(
        "vimarsa-progress",
        json!({ "stage": stage, "message": message }),
    );
}

#[tauri::command]
async fn analyze_repository_url(
    url: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RepoReport, String> {
    let parsed = github::parse_github_url(&url).map_err(|error| error.to_string())?;

    emit_progress(
        &app,
        "cloning",
        &format!("Cloning {} …", parsed.clone_url),
    );

    let workspace = tauri::async_runtime::spawn_blocking({
        let parsed = parsed.clone();
        move || github::clone_repository(&parsed)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;

    emit_progress(&app, "scanning", "Scanning repository structure …");

    let root = workspace.path().to_string_lossy().to_string();

    let mut report = tauri::async_runtime::spawn_blocking(move || {
        analyze_repository(&root)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| {
        emit_progress(&app, "error", &error.to_string());
        error.to_string()
    })?;

    report.url = Some(url);

    *state
        .last_report
        .lock()
        .map_err(|_| "state lock poisoned".to_string())? = Some(report.clone());

    drop(workspace);

    emit_progress(&app, "done", "Analysis complete.");
    Ok(report)
}

#[tauri::command]
async fn analyze_local_path(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RepoReport, String> {
    emit_progress(&app, "scanning", "Scanning local folder …");

    let mut report = tauri::async_runtime::spawn_blocking(move || {
        analyze_repository(&path)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| {
        emit_progress(&app, "error", &error.to_string());
        error.to_string()
    })?;

    report.url = None;

    *state
        .last_report
        .lock()
        .map_err(|_| "state lock poisoned".to_string())? = Some(report.clone());

    emit_progress(&app, "done", "Analysis complete.");
    Ok(report)
}

#[tauri::command]
async fn explain(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let api_key = std::env::var("GROQ_API_KEY").map_err(|_| {
        "AI API key is not set. Create a `.env` file in the project root with \
         GROQ_API_KEY=your-key (or export it in your shell) and restart Vimarśa."
            .to_string()
    })?;

    let report = state
        .last_report
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?
        .clone()
        .ok_or_else(|| "Analyze a repository first.".to_string())?;

    emit_progress(
        &app,
        "preparing_ai",
        "Preparing a compact structured summary for the AI …",
    );

    emit_progress(
        &app,
        "explaining",
        "Asking AI to interpret the scanner's facts …",
    );

    match explain_repository(&api_key, &report).await {
        Ok(explanation) => {
            emit_progress(&app, "done", "Explanation ready.");
            Ok(explanation)
        }
        Err(error) => {
            emit_progress(&app, "error", &error.to_string());
            Err(error.to_string())
        }
    }
}

fn main() {
    let _ = dotenvy::dotenv();
    github::cleanup_stale_workspaces();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            last_report: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            analyze_repository_url,
            analyze_local_path,
            explain
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vimarśa");
}
