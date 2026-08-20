use tauri::State;

use crate::commands::models::resolve_models_dir;
use crate::models::AppSettings;
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
pub fn save_settings(mut settings: AppSettings, state: State<'_, AppState>) -> Result<(), String> {
    normalize_settings(&mut settings);
    let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    let db = state.db.lock().unwrap();
    db.set_setting("app_settings", &json).map_err(|e| e.to_string())?;
    drop(db);
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

/// Clean up user-entered values before they are persisted, so every consumer
/// (engine, RAG embeddings, startup auto-connect) reads the same canonical form.
pub fn normalize_settings(settings: &mut AppSettings) {
    settings.remote_server_url = settings
        .remote_server_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(crate::engine::remote::normalize_server_url);
}

#[tauri::command]
pub fn get_data_dir(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.data_dir.to_string_lossy().to_string())
}

/// Return the absolute, resolved models directory path.
/// Respects `settings.models_directory` — absolute paths are used as-is,
/// relative paths are joined with the app data directory.
#[tauri::command]
pub fn get_models_dir(state: State<'_, AppState>) -> Result<String, String> {
    let dir = state.settings.lock().unwrap().models_directory.clone();
    let resolved = resolve_models_dir(&state.data_dir, &dir);
    Ok(resolved.to_string_lossy().to_string())
}
