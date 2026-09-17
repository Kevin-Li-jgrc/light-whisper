use super::*;
use crate::services::profile_service;
use crate::state::RecordingPhase;

pub(super) fn classify_backend(spec: &HotkeySpec) -> HotkeyBackend {
    if spec.force_low_level_hook() {
        HotkeyBackend::LowLevelHook
    } else {
        HotkeyBackend::RegisterHotKey
    }
}

/// 热键录入会记录左右 Ctrl/Alt，不能绕过通用 Ctrl/Alt 绑定的冲突检查。
pub(super) fn shortcuts_overlap(left: &HotkeySpec, right: &HotkeySpec) -> bool {
    fn parts(spec: &HotkeySpec) -> (Option<u16>, u8) {
        let (key, modifiers) = match spec {
            HotkeySpec::Standard {
                main_vk, modifiers, ..
            } => (Some(*main_vk), &modifiers.keys),
            HotkeySpec::ModifierOnly {
                required_modifiers, ..
            } => (None, required_modifiers),
        };
        let mask = modifiers.iter().fold(0, |mask, key| {
            mask | match key.family() {
                ModifierFamily::Ctrl => 1,
                ModifierFamily::Alt => 2,
                ModifierFamily::Shift => 4,
                ModifierFamily::Super => 8,
            }
        });
        (key, mask)
    }
    let (left_key, left_mods) = parts(left);
    let (right_key, right_mods) = parts(right);
    match (left_key, right_key) {
        (None, None) => left_mods & right_mods == left_mods || left_mods & right_mods == right_mods,
        (Some(l), Some(r)) => {
            l == r && (left_mods & right_mods == left_mods || left_mods & right_mods == right_mods)
        }
        // 纯修饰键先于主键触发，不能作为补输入/录音组合键的前缀。
        (None, _) => left_mods & right_mods == left_mods,
        (_, None) => left_mods & right_mods == right_mods,
    }
}

#[tauri::command]
pub fn get_reinsert_hotkey(state: tauri::State<'_, AppState>) -> Option<String> {
    state.with_profile(|profile| profile.reinsert_hotkey.clone())
}

#[tauri::command]
pub async fn set_reinsert_hotkey(
    window: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    shortcut: Option<String>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("请在主窗口设置补输入快捷键".into());
    }
    let shortcut = shortcut
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    register_reinsert_hotkey_inner(app_handle, shortcut.clone()).map_err(|e| e.to_string())?;
    profile_service::update_profile_and_schedule(state.inner(), |p| p.reinsert_hotkey = shortcut);
    Ok(())
}

pub(crate) fn register_reinsert_hotkey_inner(
    app_handle: tauri::AppHandle,
    shortcut: Option<String>,
) -> Result<String, AppError> {
    #[cfg(not(target_os = "windows"))]
    {
        if shortcut.is_some() {
            ensure_unified_hotkey_monitor(app_handle)?;
        }
        Ok("补输入热键已更新".into())
    }
    #[cfg(target_os = "windows")]
    {
        // 先验证，再替换；失败时保留先前可用的快捷键。
        let next = shortcut
            .map(|s| -> Result<_, AppError> {
                let spec = normalize_shortcut(&s)?;
                ensure_hotkey_not_conflicting(&app_handle, HotkeyKind::Reinsert, spec.label())?;
                let previous = get_unified_hook_states().reinsert;
                if previous
                    .as_ref()
                    .is_none_or(|p| p.spec.label() != spec.label())
                {
                    if let Some(conflict) = probe_system_hotkey_conflict(&spec) {
                        return Err(AppError::Other(conflict));
                    }
                }
                Ok(build_hook_state(app_handle.clone(), spec, None))
            })
            .transpose()?;
        let previous = set_unified_hook_state(HotkeyKind::Reinsert, next.clone());
        unregister_via_reg_hotkey(HotkeyKind::Reinsert);
        let register = || -> Result<(), AppError> {
            if let Some(next) = next.as_ref() {
                if next.backend == HotkeyBackend::RegisterHotKey {
                    if let HotkeySpec::Standard {
                        modifiers, main_vk, ..
                    } = &next.spec
                    {
                        register_via_reg_hotkey(
                            HotkeyKind::Reinsert,
                            modifiers,
                            *main_vk,
                            next.clone(),
                        )?;
                    }
                }
            }
            sync_hotkey_monitor_lifecycle(app_handle.clone())
        };
        if let Err(error) = register() {
            unregister_via_reg_hotkey(HotkeyKind::Reinsert);
            set_unified_hook_state(HotkeyKind::Reinsert, previous.clone());
            if let Some(previous) = previous.as_ref() {
                try_register_hotkey_backend(HotkeyKind::Reinsert, previous);
            }
            let _ = sync_hotkey_monitor_lifecycle(app_handle);
            return Err(error);
        }
        Ok("补输入热键已更新".into())
    }
}

fn recording_busy(state: &AppState) -> bool {
    state.recording.recording.lock().is_some()
        || state.recording.reinsert.is_processing()
        || state.recording.snapshot().is_some_and(|s| {
            matches!(
                s.phase,
                RecordingPhase::Starting | RecordingPhase::Recording | RecordingPhase::Processing
            )
        })
}

#[cfg(target_os = "windows")]
fn keys_released(hook: &UnifiedHookState) -> bool {
    if hook.backend == HotkeyBackend::LowLevelHook && hook.activated.load(Ordering::Acquire) {
        return false;
    }
    let main_released = match &hook.spec {
        HotkeySpec::Standard { main_vk, .. } => !is_key_physically_down(*main_vk),
        HotkeySpec::ModifierOnly { .. } => true,
    };
    main_released
        && [
            VK_LCONTROL,
            VK_RCONTROL,
            VK_LMENU,
            VK_RMENU,
            VK_LSHIFT,
            VK_RSHIFT,
            VK_LWIN,
            VK_RWIN,
        ]
        .into_iter()
        .all(|vk| !is_key_physically_down(vk))
}

#[cfg(target_os = "windows")]
pub(super) fn handle_press(hook: Arc<UnifiedHookState>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    if !get_unified_hook_states()
        .reinsert
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, &hook))
    {
        return;
    }
    let app = hook.app_handle.clone();
    let state = app.state::<AppState>();
    let Some(request) = state.recording.reinsert.try_begin() else {
        return;
    };
    let session_id = state.recording.session_counter.load(Ordering::Acquire);
    let target = unsafe { GetForegroundWindow() } as usize;
    let mut target_process = 0;
    unsafe {
        GetWindowThreadProcessId(target as _, &mut target_process);
    }
    // 在软件设置中录入快捷键时，不能把缓存文字输入到软件自身。
    if target == 0 || target_process == std::process::id() {
        return;
    }
    let initial_busy = recording_busy(&state);
    tauri::async_runtime::spawn(async move {
        let _request = request;
        let status = async {
            let state = app.state::<AppState>();
            if initial_busy {
                return "busy";
            }
            if state.recording.reinsert.text().is_none() {
                return "empty";
            }
            // 等待真实松键后再发送输入，避免 Ctrl/Alt 修饰键影响目标编辑器。
            let released = tokio::time::timeout(std::time::Duration::from_secs(4), async {
                while !keys_released(&hook) {
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                }
            })
            .await;
            if released.is_err() {
                return "releaseKeys";
            }
            if !get_unified_hook_states()
                .reinsert
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &hook))
            {
                return "cancelled";
            }
            if state.recording.session_counter.load(Ordering::Acquire) != session_id
                || recording_busy(&state)
            {
                return "busy";
            }
            if unsafe { GetForegroundWindow() } as usize != target {
                return "focusChanged";
            }
            let Ok(_output) = state.recording.reinsert.output_lock.try_lock() else {
                return "busy";
            };
            if recording_busy(&state) {
                return "busy";
            }
            let Some(text) = state.recording.reinsert.text() else {
                return "empty";
            };
            let method = state.ui.input_method.lock().clone();
            match crate::commands::clipboard::paste_text_unlocked(&app, &text, &method).await {
                Ok(_) => "sent",
                Err(error) => {
                    log::warn!("补输入失败: {}", error);
                    "failed"
                }
            }
        }
        .await;
        if status != "cancelled" {
            crate::commands::window::show_reinsert_notice(&app, status).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_generic_side_specific_and_modifier_prefix_conflicts() {
        for (a, b) in [
            ("Ctrl+Alt+V", "LeftCtrl+LeftAlt+V"),
            ("Alt", "Alt+V"),
            ("F2", "Ctrl+F2"),
        ] {
            let a = normalize_shortcut(a).unwrap();
            let b = normalize_shortcut(b).unwrap();
            assert!(shortcuts_overlap(&a, &b));
            assert!(shortcuts_overlap(&b, &a));
        }
        assert!(!shortcuts_overlap(
            &normalize_shortcut("F2").unwrap(),
            &normalize_shortcut("Ctrl+Alt+V").unwrap()
        ));
    }
    #[test]
    fn one_shot_hotkey_backend_does_not_depend_on_recording_mode() {
        assert_eq!(
            classify_backend(&normalize_shortcut("Ctrl+Alt+V").unwrap()),
            HotkeyBackend::RegisterHotKey
        );
        assert_eq!(
            classify_backend(&normalize_shortcut("RightAlt").unwrap()),
            HotkeyBackend::LowLevelHook
        );
    }
    #[test]
    fn blocks_during_processing_even_after_recording_slot_is_cleared() {
        let state = AppState::default();
        let task = state.recording.reinsert.processing();
        assert!(recording_busy(&state));
        drop(task);
        assert!(!recording_busy(&state));
    }
}
