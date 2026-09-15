use crate::commands::audio::{
    start_recording_inner, stop_recording_inner, RECORDING_ALREADY_ACTIVE_ERROR,
    RECORDING_NOT_READY_ERROR, RECORDING_START_CANCELLED_ERROR,
};
use crate::state::{AppState, RecordingSlot, RecordingTrigger};
use crate::utils::AppError;
use std::sync::atomic::Ordering;
use tauri::Manager;

use super::{
    emit_recording_error, is_toggle_mode, now_unix_ms, reset_hotkey_gate_for_trigger,
    update_hotkey_diagnostic_for_trigger, HotkeyEventGate, HOTKEY_REPRESS_DEBOUNCE_MS,
};

// Recording start/stop business logic

pub(super) fn is_ignorable_start_audio_error(message: &str) -> bool {
    message == RECORDING_NOT_READY_ERROR
        || message == RECORDING_ALREADY_ACTIVE_ERROR
        || message == RECORDING_START_CANCELLED_ERROR
}

pub(super) fn handle_hotkey_start(
    app_handle: tauri::AppHandle,
    shortcut_label: String,
    trigger: RecordingTrigger,
) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();

        // 选中文本抓取并行化：立即 spawn 到后台，作为参数交给 start_recording_inner。
        // handle 由该会话的 RecordingSession.edit_grab 持有，finalize_recording 会以
        // 短超时 join；结果只在 finalize 的本地变量里流转，不写全局，避免跨会话串位。
        let grab_handle =
            tokio::task::spawn_blocking(crate::commands::clipboard::grab_selected_text);

        match start_recording_inner(
            app_handle.clone(),
            state.inner(),
            trigger,
            Some(grab_handle),
        )
        .await
        {
            Ok(session_id) => {
                log::info!(
                    "热键 {} 触发录音开始 (session {}, mode={})",
                    shortcut_label,
                    session_id,
                    trigger.mode().as_str()
                );
            }
            Err(AppError::Audio(message)) if is_ignorable_start_audio_error(&message) => {
                if is_toggle_mode() {
                    reset_hotkey_gate_for_trigger(trigger);
                }
                log::debug!("忽略热键 {} 的开始请求: {}", shortcut_label, message);
            }
            Err(AppError::Audio(message)) => {
                if is_toggle_mode() {
                    reset_hotkey_gate_for_trigger(trigger);
                }
                // Audio startup failures already publish a session-scoped
                // recording-state + start_error from start_recording_inner.
                // Keep diagnostics here without a second unscoped event that
                // could land after a newer session has started.
                log::warn!("热键 {} 开始录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
            }
            Err(err) => {
                if is_toggle_mode() {
                    reset_hotkey_gate_for_trigger(trigger);
                }
                let message = err.to_string();
                log::warn!("热键 {} 开始录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
                emit_recording_error(&app_handle, &message);
            }
        }
    });
}

pub(super) fn handle_hotkey_stop(
    app_handle: tauri::AppHandle,
    shortcut_label: String,
    trigger: RecordingTrigger,
    expected_session_id: u64,
) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        match stop_recording_inner(
            app_handle.clone(),
            state.inner(),
            Some((expected_session_id, trigger)),
        )
        .await
        {
            Ok(Some(session_id)) => {
                log::info!(
                    "热键 {} 触发录音停止 (session {}, mode={})",
                    shortcut_label,
                    session_id,
                    trigger.mode().as_str()
                );
            }
            Ok(None) => {
                log::debug!("忽略热键 {} 的停止请求：当前没有活跃录音", shortcut_label);
            }
            Err(err) => {
                let message = err.to_string();
                log::warn!("热键 {} 停止录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
                emit_recording_error(&app_handle, &message);
            }
        }
    });
}

pub(super) fn handle_current_hotkey_stop(
    app_handle: tauri::AppHandle,
    shortcut_label: String,
    trigger: RecordingTrigger,
) {
    let expected_session_id = app_handle
        .state::<AppState>()
        .recording
        .recording
        .lock()
        .as_ref()
        .filter(|slot| slot.trigger() == trigger)
        .map(RecordingSlot::session_id);
    if let Some(session_id) = expected_session_id {
        handle_hotkey_stop(app_handle, shortcut_label, trigger, session_id);
    } else {
        log::debug!(
            "忽略热键 {} 的停止请求：当前活跃录音不属于 trigger={:?}",
            shortcut_label,
            trigger
        );
    }
}

// ---------------------------------------------------------------------------
// Dispatch — supports both hold and toggle modes
// ---------------------------------------------------------------------------

pub(super) fn dispatch_hotkey_press(
    app_handle: &tauri::AppHandle,
    gate: &HotkeyEventGate,
    trigger: RecordingTrigger,
    pressed_log: &str,
    shortcut_label: &str,
) {
    let active_trigger = app_handle
        .state::<AppState>()
        .recording
        .recording
        .lock()
        .as_ref()
        .map(RecordingSlot::trigger);
    let allow_toggle_stop = is_toggle_mode()
        && gate.toggle_active.load(Ordering::Acquire)
        && active_trigger == Some(trigger);

    if active_trigger.is_some() && !allow_toggle_stop {
        log::debug!(
            "忽略热键 {} 的按下：已有录音进行中 (active trigger={:?}, request trigger={:?})",
            shortcut_label,
            active_trigger,
            trigger
        );
        return;
    }

    if is_toggle_mode() {
        // Toggle mode: each press flips recording on/off
        let was_active = gate.toggle_active.load(Ordering::Acquire);
        if was_active {
            // Turn off
            gate.toggle_active.store(false, Ordering::Release);
            gate.is_pressed.store(false, Ordering::Release);
            let now_ms = now_unix_ms();
            gate.last_release_ms.store(now_ms, Ordering::Release);
            log::info!("切换模式：再次按下 {}，停止录音", shortcut_label);
            update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
                diagnostic.is_pressed = false;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("released".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_released_at_ms = Some(now_ms);
            });
            handle_current_hotkey_stop(app_handle.clone(), shortcut_label.to_string(), trigger);
        } else {
            // Turn on — apply debounce
            let now_ms = now_unix_ms();
            let last_release_ms = gate.last_release_ms.load(Ordering::Acquire);
            if now_ms.saturating_sub(last_release_ms) < HOTKEY_REPRESS_DEBOUNCE_MS {
                return;
            }
            gate.toggle_active.store(true, Ordering::Release);
            gate.is_pressed.store(true, Ordering::Release);
            log::info!("切换模式：{}", pressed_log);
            update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
                diagnostic.is_pressed = true;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("pressed".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_pressed_at_ms = Some(now_ms);
            });
            handle_hotkey_start(app_handle.clone(), shortcut_label.to_string(), trigger);
        }
        return;
    }

    // Hold mode (original behavior)
    let now_ms = now_unix_ms();
    let last_release_ms = gate.last_release_ms.load(Ordering::Acquire);

    if now_ms.saturating_sub(last_release_ms) < HOTKEY_REPRESS_DEBOUNCE_MS {
        log::debug!(
            "忽略热键 {} 的按下抖动（距离上次松开 {}ms）",
            shortcut_label,
            now_ms.saturating_sub(last_release_ms)
        );
        return;
    }

    if gate
        .is_pressed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        log::info!("{}", pressed_log);
        update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
            diagnostic.is_pressed = true;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("pressed".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_pressed_at_ms = Some(now_ms);
        });
        handle_hotkey_start(app_handle.clone(), shortcut_label.to_string(), trigger);
    }
}

pub(super) fn dispatch_hotkey_release(
    app_handle: &tauri::AppHandle,
    gate: &HotkeyEventGate,
    trigger: RecordingTrigger,
    released_log: &str,
    shortcut_label: &str,
) {
    // In toggle mode, release is a no-op (press handles both start and stop)
    if is_toggle_mode() {
        return;
    }

    if gate.is_pressed.swap(false, Ordering::AcqRel) {
        let now_ms = now_unix_ms();
        gate.last_release_ms.store(now_ms, Ordering::Release);
        log::info!("{}", released_log);
        update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
            diagnostic.is_pressed = false;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("released".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_released_at_ms = Some(now_ms);
        });
        handle_current_hotkey_stop(app_handle.clone(), shortcut_label.to_string(), trigger);
    }
}
