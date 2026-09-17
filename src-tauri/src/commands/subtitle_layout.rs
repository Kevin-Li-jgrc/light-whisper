use crate::services::profile_service;
use crate::state::{user_profile::SubtitleTiming, AppState};
use tauri::Manager;

const EDITOR_LABEL: &str = "subtitle-layout";

fn check_caller(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err("此窗口无权操作悬浮窗设置".to_string())
    }
}

#[tauri::command]
pub async fn open_subtitle_layout_editor(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    check_caller(window.label(), "main")?;
    if let Some(editor) = app.get_webview_window(EDITOR_LABEL) {
        editor.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let (width, height, x, y) = super::window::resolve_subtitle_layout(&app);
    tauri::WebviewWindowBuilder::new(
        &app,
        EDITOR_LABEL,
        tauri::WebviewUrl::App("/?window=subtitle-layout".into()),
    )
    .title("调整悬浮窗")
    .inner_size(width, height)
    .position(x, y)
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn show_subtitle_layout_editor(window: tauri::WebviewWindow) -> Result<(), String> {
    check_caller(window.label(), EDITOR_LABEL)?;
    // Show only after the editor has rendered its transparent background.
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn close_subtitle_layout_editor(window: tauri::WebviewWindow) -> Result<(), String> {
    check_caller(window.label(), EDITOR_LABEL)?;
    window.close().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_main_can_open_the_editor() {
        assert!(check_caller("main", "main").is_ok());
        assert!(check_caller("subtitle", "main").is_err());
        assert!(check_caller(EDITOR_LABEL, "main").is_err());
    }

    #[test]
    fn editor_commands_cannot_show_or_close_recording_windows() {
        assert!(check_caller(EDITOR_LABEL, EDITOR_LABEL).is_ok());
        assert!(check_caller("subtitle", EDITOR_LABEL).is_err());
        assert!(check_caller("main", EDITOR_LABEL).is_err());
    }
}

#[tauri::command]
pub fn get_subtitle_timing(state: tauri::State<'_, AppState>) -> SubtitleTiming {
    state.with_profile(|profile| profile.subtitle_timing.validated_or_default())
}

#[tauri::command]
pub fn set_subtitle_timing(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    timing: SubtitleTiming,
) -> Result<(), String> {
    check_caller(window.label(), "main")?;
    timing.validate()?;
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.subtitle_timing = timing;
    });
    Ok(())
}
