use std::collections::hash_map::Entry;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use crate::state::user_profile::*;
use crate::state::AppState;
use crate::utils::foreground::normalize_whitespace;
use crate::utils::paths;

const MAX_CORRECTION_PATTERNS: usize = 500;
const MAX_SEGMENT_CHARS: usize = 12;
const MAX_HOT_WORD_CHARS: usize = 24;
const MAX_USER_HOT_WORD_CHARS: usize = 80;
pub const MAX_APP_PROFILE_RULES: usize = 100;
const PROFILE_SAVE_DEBOUNCE_MS: u64 = 350;

// ============================================================
// 持久化
// ============================================================

pub fn load_profile() -> UserProfile {
    let path = paths::get_data_dir().join("user_profile.json");
    let mut profile = match std::fs::read_to_string(&path) {
        Ok(data) => match serde_json::from_str::<serde_json::Value>(&data) {
            Ok(value) if value.is_object() => serde_json::from_value(value).unwrap_or_else(|err| {
                log::warn!("用户画像对象解析失败，使用默认值: {}", err);
                UserProfile::default()
            }),
            Ok(_) => {
                log::warn!("用户画像文件是有效 JSON 但不是对象，使用默认值且保留原文件");
                UserProfile::default()
            }
            Err(err) => {
                log::warn!("用户画像文件解析失败，使用默认值且保留原文件: {}", err);
                UserProfile::default()
            }
        },
        Err(_) => {
            log::info!("用户画像文件不存在，使用默认值");
            UserProfile::default()
        }
    };
    let stats = normalize_profile(&mut profile);
    if stats.removed_hot_words > 0 || stats.removed_corrections > 0 {
        log::info!(
            "加载画像时清理：热词 -{}, 纠错 -{}",
            stats.removed_hot_words,
            stats.removed_corrections
        );
    }
    profile
}

pub fn normalize_profile(profile: &mut UserProfile) -> ProfileCleanupStats {
    migrate_custom_provider(profile);
    migrate_reasoning_modes(profile);
    let stats = cleanup_profile(profile);
    super::correction_audit::prune(profile);
    stats
}

/// 导入与审核/确认删除互斥；清除外部画像的报告授权后再提交。
pub async fn import_profile<R, F, Fut>(
    state: &AppState,
    json: &str,
    persist: F,
) -> Result<R, String>
where
    F: FnOnce(UserProfile) -> Fut,
    Fut: std::future::Future<Output = Result<R, String>>,
{
    let _guard =
        super::correction_audit::AuditGuard::acquire(&state.profile.correction_audit_running)?;
    let mut imported: UserProfile =
        serde_json::from_str(json).map_err(|e| format!("解析画像数据失败: {e}"))?;
    imported.correction_audit.cache.clear();
    imported.correction_audit.report = None;
    normalize_profile(&mut imported);
    persist(imported).await
}

/// 在保存事务持有画像锁后调用，旧备份保留提交时的全局设置。
pub fn replace_imported_profile(
    profile: &mut UserProfile,
    mut imported: UserProfile,
    legacy: &str,
) {
    if imported.input_method.is_none() {
        imported.input_method = Some(profile.effective_input_method(legacy));
    }
    *profile = imported;
}

/// 即使前端还未完成旧缓存迁移，备份也始终带上当前有效的全局输入方式。
pub fn export_profile(state: &AppState) -> Result<String, String> {
    let legacy = state.ui.input_method.lock().clone();
    let mut profile = state.snapshot_profile();
    profile.input_method = Some(profile.effective_input_method(&legacy));
    serialize_profile(&profile)
}

#[cfg(test)]
mod input_config_tests {
    use super::*;

    #[tokio::test]
    async fn input_config_round_trip_keeps_global_and_complete_ordered_app_rules() {
        let state = AppState::default();
        let mut value = serde_json::to_value(UserProfile::default()).unwrap();
        value["input_method"] = serde_json::json!("clipboard");
        value["app_profile_rules"] = serde_json::json!([
            {"id":"chat", "name":"微信", "enabled":true, "process_name":"Weixin.exe",
             "window_title_contains":"工作", "input_method":"sendInput", "ai_polish":"disabled",
             "translation":"target", "translation_target":"English", "screen_context":"enabled",
             "history":"disabled", "custom_prompt":"保留原文标点"},
            {"id":"browser", "name":"浏览器", "enabled":false, "process_name":"chrome.exe",
             "input_method":"inherit"}
        ]);
        let original: UserProfile = serde_json::from_value(value).unwrap();
        state.update_profile_mut(|p| *p = original.clone());
        let exported = export_profile(&state).unwrap();
        let imported = import_profile(&state, &exported, |p| async { Ok(p) })
            .await
            .unwrap();
        let restored = serde_json::to_value(&imported).unwrap();
        assert_eq!(restored["input_method"], "clipboard");
        assert_eq!(
            restored["app_profile_rules"],
            serde_json::to_value(&original).unwrap()["app_profile_rules"]
        );
        assert_eq!(
            imported.resolve_input_method("Weixin.exe", "工作群", "sendInput"),
            "sendInput"
        );
        assert_eq!(
            imported.resolve_input_method("chrome.exe", "网页", "sendInput"),
            "clipboard"
        );
    }

    #[tokio::test]
    async fn input_config_legacy_import_preserves_current_global_method() {
        let state = AppState::default();
        *state.ui.input_method.lock() = "clipboard".into();
        let json = serialize_profile(&UserProfile::default()).unwrap();
        let imported = import_profile(&state, &json, |p| async {
            let legacy = state.ui.input_method.lock().clone();
            Ok(state
                .update_profile(|current| replace_imported_profile(current, p, &legacy))
                .1)
        })
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(imported).unwrap()["input_method"],
            "clipboard"
        );
    }

    #[tokio::test]
    async fn input_config_invalid_global_method_never_reaches_persistence() {
        let state = AppState::default();
        let mut value = serde_json::to_value(UserProfile::default()).unwrap();
        value["input_method"] = serde_json::json!("inherit");
        let result = import_profile(&state, &value.to_string(), |_| async {
            Err::<(), _>("unexpected persistence".into())
        })
        .await;
        assert!(result.unwrap_err().contains("解析画像数据失败"));
    }

    #[test]
    fn input_config_startup_migration_does_not_overwrite_imported_setting() {
        let mut profile = UserProfile::default();
        assert_eq!(
            profile.set_input_method(InputMethod::Clipboard, true),
            InputMethod::Clipboard
        );
        assert_eq!(
            profile.set_input_method(InputMethod::SendInput, true),
            InputMethod::Clipboard
        );
        assert_eq!(
            profile.set_input_method(InputMethod::SendInput, false),
            InputMethod::SendInput
        );
        let restarted: UserProfile =
            serde_json::from_str(&serialize_profile(&profile).unwrap()).unwrap();
        assert_eq!(
            restarted.effective_input_method("clipboard"),
            InputMethod::SendInput
        );
    }

    #[test]
    fn input_config_export_always_includes_current_method_before_migration() {
        let state = AppState::default();
        for method in ["sendInput", "clipboard"] {
            *state.ui.input_method.lock() = method.into();
            let value: serde_json::Value =
                serde_json::from_str(&export_profile(&state).unwrap()).unwrap();
            assert_eq!(value["input_method"], method);
        }
    }

    #[tokio::test]
    async fn input_config_legacy_import_prefers_saved_method_over_stale_cache() {
        let state = AppState::default();
        state.update_profile_mut(|p| p.input_method = Some(InputMethod::Clipboard));
        let json = serialize_profile(&UserProfile::default()).unwrap();
        let imported = import_profile(&state, &json, |p| async {
            Ok(state
                .update_profile(|current| replace_imported_profile(current, p, "sendInput"))
                .1)
        })
        .await
        .unwrap();
        assert_eq!(
            imported.effective_input_method("sendInput"),
            InputMethod::Clipboard
        );
    }

    #[tokio::test]
    async fn input_config_legacy_import_keeps_setting_changed_while_waiting_to_commit() {
        let state = AppState::default();
        state.update_profile_mut(|p| p.input_method = Some(InputMethod::SendInput));
        let json = serialize_profile(&UserProfile::default()).unwrap();
        let imported = import_profile(&state, &json, |p| async {
            // 模拟用户设置先取得保存锁，并在导入提交之前完成。
            state.update_profile_mut(|current| current.input_method = Some(InputMethod::Clipboard));
            Ok(state
                .update_profile(|current| replace_imported_profile(current, p, "sendInput"))
                .1)
        })
        .await
        .unwrap();
        assert_eq!(imported.input_method, Some(InputMethod::Clipboard));
    }

    #[test]
    fn input_config_failed_import_write_leaves_current_method_and_rules_unchanged() {
        let mut current = UserProfile {
            input_method: Some(InputMethod::SendInput),
            ..Default::default()
        };
        let mut imported = UserProfile {
            input_method: Some(InputMethod::Clipboard),
            ..Default::default()
        };
        imported.app_profile_rules.push(AppProfileRule {
            id: "new".into(),
            process_name: "Weixin.exe".into(),
            ..Default::default()
        });
        let result = commit_candidate(
            &mut current,
            |candidate| {
                replace_imported_profile(candidate, imported, "sendInput");
                Ok(())
            },
            |_| Err("disk full".into()),
        );
        assert!(result.is_err());
        assert_eq!(current.input_method, Some(InputMethod::SendInput));
        assert!(current.app_profile_rules.is_empty());
    }
}

fn migrate_reasoning_modes(profile: &mut UserProfile) {
    let config = &mut profile.llm_provider;
    if config.polish_reasoning_mode.is_none() {
        config.polish_reasoning_mode = Some(config.reasoning_mode);
    }
    if config.assistant_reasoning_mode.is_none() {
        config.assistant_reasoning_mode = Some(config.reasoning_mode);
    }
}

/// 迁移旧版单 custom provider 到 custom_providers 列表
fn migrate_custom_provider(profile: &mut UserProfile) {
    let config = &mut profile.llm_provider;
    if config.active != "custom" || !config.custom_providers.is_empty() {
        return;
    }
    let base_url = config.custom_base_url.clone().unwrap_or_default();
    let model = config.custom_model.clone().unwrap_or_default();
    if base_url.is_empty() && model.is_empty() {
        return;
    }
    let provider = CustomProvider {
        id: "custom_migrated".to_string(),
        name: "自定义兼容".to_string(),
        base_url,
        model,
        api_format: ApiFormat::default(),
    };
    config.custom_providers.push(provider);
    config.active = "custom_migrated".to_string();
    config.custom_base_url = None;
    config.custom_model = None;
    log::info!("已迁移旧版 custom provider 到 custom_providers");
}

fn serialize_profile(profile: &UserProfile) -> Result<String, String> {
    serde_json::to_string_pretty(profile).map_err(|e| format!("序列化失败: {}", e))
}

struct PendingProfileSave {
    generation: u64,
    profile: UserProfile,
}

fn pending_profile_save_slot() -> &'static parking_lot::Mutex<Option<PendingProfileSave>> {
    static SLOT: OnceLock<parking_lot::Mutex<Option<PendingProfileSave>>> = OnceLock::new();
    SLOT.get_or_init(|| parking_lot::Mutex::new(None))
}

fn profile_save_generation() -> &'static AtomicU64 {
    static GENERATION: OnceLock<AtomicU64> = OnceLock::new();
    GENERATION.get_or_init(|| AtomicU64::new(0))
}

fn profile_save_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn take_pending_profile_save_if(
    predicate: impl FnOnce(&PendingProfileSave) -> bool,
) -> Option<PendingProfileSave> {
    let mut slot = pending_profile_save_slot().lock();
    if slot.as_ref().is_some_and(predicate) {
        slot.take()
    } else {
        None
    }
}

async fn write_profile_async(profile: &UserProfile) -> Result<(), String> {
    let path = paths::get_data_dir().join("user_profile.json");
    let profile = profile.clone();
    tokio::task::spawn_blocking(move || {
        write_profile_if_newer(&profile, &path, &LAST_WRITTEN_REVISION)
    })
    .await
    .map_err(|e| format!("写入任务异常: {}", e))?
}

fn backup_before_hotword_learning(path: &std::path::Path) -> std::io::Result<()> {
    backup_before_schema(path, "hotword_learning", "hotword-learning")
}

fn backup_before_schema(path: &std::path::Path, field: &str, name: &str) -> std::io::Result<()> {
    use std::io::Write;
    let old = match std::fs::read(path) {
        Ok(old) => old,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if serde_json::from_slice::<serde_json::Value>(&old)
        .ok()
        .is_some_and(|value| value.get(field).is_some())
    {
        return Ok(());
    }
    let mut backup = path.with_file_name(format!("user_profile.before-{name}.json"));
    if backup.exists() {
        if std::fs::read(&backup)? == old {
            return Ok(());
        }
        // 旧备份或上次失败留下的半成品不能冒充当前原文件的完整备份。
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        backup = path.with_file_name(format!("user_profile.before-{name}.{stamp}.json"));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)?;
    file.write_all(&old)?;
    file.sync_all()
}

static NEXT_REVISION: AtomicU64 = AtomicU64::new(0);
static LAST_WRITTEN_REVISION: AtomicU64 = AtomicU64::new(0);
pub fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::SeqCst) + 1
}

// 调用方统一持有保存锁；旧快照不能把已确认删除的数据写回。
fn write_profile_if_newer(
    profile: &UserProfile,
    path: &std::path::Path,
    revision: &AtomicU64,
) -> Result<(), String> {
    if profile.persistence_revision < revision.load(Ordering::SeqCst) {
        return Ok(());
    }
    let data = serialize_profile(profile)?;
    backup_before_hotword_learning(path).map_err(|e| format!("画像备份失败: {e}"))?;
    backup_before_schema(path, "correction_audit", "correction-audit")
        .map_err(|e| format!("审核数据迁移备份失败: {e}"))?;
    paths::atomic_write(path, data.as_bytes()).map_err(|e| format!("写入失败: {e}"))?;
    revision.store(profile.persistence_revision, Ordering::SeqCst);
    Ok(())
}

fn commit_candidate<R>(
    profile: &mut UserProfile,
    change: impl FnOnce(&mut UserProfile) -> Result<R, String>,
    write: impl FnOnce(&UserProfile) -> Result<(), String>,
) -> Result<R, String> {
    let mut candidate = profile.clone();
    let result = change(&mut candidate)?;
    candidate.persistence_revision = next_revision();
    write(&candidate)?;
    *profile = candidate;
    Ok(result)
}

/// 用户确认的操作必须在磁盘写入成功后才提交内存，不依赖防抖保存。
pub async fn commit_profile<R, F>(state: &AppState, change: F) -> Result<R, String>
where
    R: Send + 'static,
    F: FnOnce(&mut UserProfile) -> Result<R, String> + Send + 'static,
{
    let profile = state.profile.user_profile.clone();
    let write_guard = profile_save_lock().lock().await;
    let result = tokio::task::spawn_blocking(move || {
        let _write_guard = write_guard;
        let mut profile = profile.lock();
        let path = paths::get_data_dir().join("user_profile.json");
        commit_candidate(&mut profile, change, |candidate| {
            write_profile_if_newer(candidate, &path, &LAST_WRITTEN_REVISION)
        })
    })
    .await
    .map_err(|e| format!("保存事务异常: {e}"))?;
    result
}

#[cfg(test)]
mod learning_backup_tests {
    #[test]
    fn audit_transaction_rolls_back_memory_when_persistence_fails() {
        let mut profile = crate::state::user_profile::UserProfile {
            total_transcriptions: 7,
            ..Default::default()
        };
        let result = super::commit_candidate(
            &mut profile,
            |candidate| {
                candidate.total_transcriptions = 99;
                Ok(())
            },
            |_| Err("disk full".into()),
        );
        assert!(result.is_err());
        assert_eq!(profile.total_transcriptions, 7);
    }

    #[test]
    fn older_snapshot_cannot_overwrite_a_committed_deletion() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("light-whisper-audit-save-{stamp}"));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("user_profile.json");
        let revision = std::sync::atomic::AtomicU64::new(0);
        let mut profile = crate::state::user_profile::UserProfile {
            persistence_revision: 20,
            total_transcriptions: 20,
            ..Default::default()
        };
        super::write_profile_if_newer(&profile, &path, &revision).unwrap();
        profile.persistence_revision = 10;
        profile.total_transcriptions = 10;
        super::write_profile_if_newer(&profile, &path, &revision).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["total_transcriptions"], 20);
        assert!(value.get("persistence_revision").is_none());
    }

    #[test]
    fn audit_schema_has_its_own_backup_after_hotword_migration() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("light-whisper-audit-backup-{stamp}"));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("user_profile.json");
        let original = br#"{"hotword_learning":{},"correction_patterns":[]}"#;
        std::fs::write(&path, original).unwrap();
        super::backup_before_schema(&path, "correction_audit", "correction-audit").unwrap();
        assert_eq!(
            std::fs::read(dir.join("user_profile.before-correction-audit.json")).unwrap(),
            original
        );
    }
    #[test]
    fn a_partial_existing_backup_cannot_replace_the_complete_original_backup() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("light-whisper-learning-backup-{stamp}"));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("user_profile.json");
        let original = br#"{"hot_words":[],"last_updated":123}"#;
        std::fs::write(&path, original).unwrap();
        let prior = dir.join("user_profile.before-hotword-learning.json");
        std::fs::write(&prior, b"partial").unwrap();
        super::backup_before_hotword_learning(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(&prior).unwrap(), b"partial");
        let complete_backups = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path() != path)
            .filter(|entry| std::fs::read(entry.path()).unwrap() == original)
            .count();
        assert_eq!(complete_backups, 1);
    }
}

pub fn schedule_profile_save(profile: UserProfile) {
    let generation = {
        let mut slot = pending_profile_save_slot().lock();
        if profile.persistence_revision < LAST_WRITTEN_REVISION.load(Ordering::SeqCst)
            || slot.as_ref().is_some_and(|saved| {
                saved.profile.persistence_revision > profile.persistence_revision
            })
        {
            return;
        }
        let generation = profile_save_generation().fetch_add(1, Ordering::SeqCst) + 1;
        *slot = Some(PendingProfileSave {
            generation,
            profile,
        });
        generation
    };

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(PROFILE_SAVE_DEBOUNCE_MS)).await;

        if profile_save_generation().load(Ordering::SeqCst) != generation {
            return;
        }

        let _write_guard = profile_save_lock().lock().await;
        if profile_save_generation().load(Ordering::SeqCst) != generation {
            return;
        }

        let pending = take_pending_profile_save_if(|pending| pending.generation == generation);
        if let Some(pending) = pending {
            if let Err(err) = write_profile_async(&pending.profile).await {
                log::warn!("异步保存用户画像失败: {}", err);
            }
        }
    });
}

pub fn update_profile_and_schedule<R>(
    state: &AppState,
    f: impl FnOnce(&mut UserProfile) -> R,
) -> R {
    let (result, profile) = state.update_profile(f);
    schedule_profile_save(profile);
    result
}

pub async fn save_profile_async(profile: &UserProfile) -> Result<(), String> {
    let _write_guard = profile_save_lock().lock().await;
    take_pending_profile_save_if(|pending| {
        pending.profile.persistence_revision <= profile.persistence_revision
    });
    write_profile_async(profile).await
}

// ============================================================
// 清理
// ============================================================

#[derive(Debug, Clone, Copy, Default)]
pub struct ProfileCleanupStats {
    pub removed_hot_words: usize,
    pub removed_corrections: usize,
}

pub fn cleanup_profile(profile: &mut UserProfile) -> ProfileCleanupStats {
    sanitize_history_settings(profile);
    sanitize_app_profile_rules(profile);
    sanitize_blocked_hot_words(profile);
    let removed_hot_words = sanitize_hot_words(profile);
    let removed_corrections = sanitize_corrections(profile) + limit_correction_patterns(profile);
    if removed_hot_words > 0 || removed_corrections > 0 {
        profile.last_updated = now_secs();
    }
    ProfileCleanupStats {
        removed_hot_words,
        removed_corrections,
    }
}

fn trimmed_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn sanitize_history_settings(profile: &mut UserProfile) {
    if profile.history_settings.retention_days > 3650 {
        profile.history_settings.retention_days = 3650;
    }
}

pub fn sanitize_app_profile_rules(profile: &mut UserProfile) {
    let mut seen_ids = HashSet::new();
    let seed = now_secs();
    let mut normalized =
        Vec::with_capacity(profile.app_profile_rules.len().min(MAX_APP_PROFILE_RULES));

    for (index, mut rule) in std::mem::take(&mut profile.app_profile_rules)
        .into_iter()
        .enumerate()
    {
        rule.process_name = rule.process_name.trim().to_string();
        if rule.process_name.is_empty() {
            continue;
        }
        rule.name = rule.name.trim().to_string();
        if rule.name.is_empty() {
            rule.name = rule.process_name.clone();
        }
        rule.window_title_contains = trimmed_option(rule.window_title_contains);
        rule.translation_target = trimmed_option(rule.translation_target);
        rule.custom_prompt = trimmed_option(rule.custom_prompt);
        if rule.translation == AppTranslationOverride::Target && rule.translation_target.is_none() {
            rule.translation = AppTranslationOverride::Inherit;
        }

        let mut id = rule.id.trim().to_string();
        if id.is_empty() || seen_ids.contains(&id) {
            id = format!("app-rule-{seed}-{index}");
        }
        seen_ids.insert(id.clone());
        rule.id = id;
        normalized.push(rule);
        if normalized.len() >= MAX_APP_PROFILE_RULES {
            break;
        }
    }

    profile.app_profile_rules = normalized;
}

fn sanitize_corrections(profile: &mut UserProfile) -> usize {
    let before = profile.correction_patterns.len();

    // 标记矛盾对中 count 较低的（A→B 与 B→A 同时存在）
    let mut contradiction_victims: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for p in profile.correction_patterns.iter() {
        let key = (p.original.clone(), p.corrected.clone());
        if contradiction_victims.contains(&key) {
            continue;
        }
        if let Some(rev) = profile
            .correction_patterns
            .iter()
            .find(|q| q.original == p.corrected && q.corrected == p.original)
        {
            // count 相等则两个都删，否则删低的
            if rev.count >= p.count {
                contradiction_victims.insert(key);
            }
            if p.count >= rev.count {
                contradiction_victims.insert((rev.original.clone(), rev.corrected.clone()));
            }
        }
    }

    profile.correction_patterns.retain(|p| {
        // 用户手动纠错永远保留
        if p.source == CorrectionSource::User {
            return true;
        }

        let orig_chars = p.original.chars().count();
        let corrected_chars = p.corrected.chars().count();

        // 过长
        if orig_chars > 15 || corrected_chars > 15 {
            return false;
        }
        // 单字符原始 + 非单字符纠正（如 "不"→"一定"）
        if orig_chars == 1 && corrected_chars != 1 {
            return false;
        }
        // 长度比例异常（如 "话"→"的源代码，"）
        let (longer, shorter) = (
            orig_chars.max(corrected_chars),
            orig_chars.min(corrected_chars),
        );
        if shorter >= 2 && longer > shorter * 3 {
            return false;
        }
        // 矛盾对（仅清理 AI 来源的一方）
        if contradiction_victims.contains(&(p.original.clone(), p.corrected.clone())) {
            return false;
        }
        // AI 来源仅出现 1 次且超过 24h 的噪声（给新规则宽限期）
        if p.count <= 1 && now_secs().saturating_sub(p.last_seen) > 24 * 60 * 60 {
            return false;
        }
        true
    });
    before - profile.correction_patterns.len()
}

fn limit_correction_patterns(profile: &mut UserProfile) -> usize {
    if profile.correction_patterns.len() <= MAX_CORRECTION_PATTERNS {
        return 0;
    }
    let before = profile.correction_patterns.len();
    profile
        .correction_patterns
        .sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
    profile
        .correction_patterns
        .truncate(MAX_CORRECTION_PATTERNS);
    before - profile.correction_patterns.len()
}

// ============================================================
// 热词管理
// ============================================================

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalize_hot_word_text(text: &str) -> String {
    normalize_whitespace(text)
}

fn normalize_hot_word_key(text: &str) -> Option<(String, String)> {
    let normalized = normalize_hot_word_text(text);
    (!normalized.is_empty()).then(|| {
        let key = normalized.to_lowercase();
        (normalized, key)
    })
}

fn sanitize_blocked_hot_words(profile: &mut UserProfile) {
    let mut deduped = std::collections::HashSet::new();
    profile.blocked_hot_words = std::mem::take(&mut profile.blocked_hot_words)
        .into_iter()
        .filter_map(|text| normalize_hot_word_key(&text).map(|(_, key)| key))
        .filter(|key| deduped.insert(key.clone()))
        .collect();
}

fn is_blocked_hot_word(profile: &UserProfile, text: &str) -> bool {
    normalize_hot_word_key(text)
        .map(|(_, key)| {
            profile
                .blocked_hot_words
                .iter()
                .any(|blocked| blocked == &key)
        })
        .unwrap_or(false)
}

fn hot_word_priority(w: &HotWord) -> (u8, u8, u32, u64, usize) {
    let src = if w.source == HotWordSource::User {
        1
    } else {
        0
    };
    (
        src,
        w.weight,
        w.use_count,
        w.last_used,
        w.text.chars().count(),
    )
}

fn merge_hot_word(existing: &mut HotWord, candidate: HotWord) {
    if hot_word_priority(&candidate) > hot_word_priority(existing) {
        existing.text = candidate.text;
    }
    existing.weight = existing.weight.max(candidate.weight.clamp(1, 5));
    existing.use_count = existing.use_count.max(candidate.use_count);
    existing.last_used = existing.last_used.max(candidate.last_used);
    if candidate.source == HotWordSource::User {
        existing.source = HotWordSource::User;
    }
}

fn contains_sentence_punctuation(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch,
            '，' | '。'
                | '！'
                | '？'
                | '；'
                | '：'
                | '、'
                | ','
                | '.'
                | '!'
                | '?'
                | ';'
                | ':'
                | '\n'
                | '\r'
                | '\t'
        )
    })
}

fn learned_hot_word_looks_like_sentence(text: &str) -> bool {
    let action_like_chars = [
        '请', '帮', '写', '说', '问', '想', '要', '给', '把', '做', '发', '改',
    ];
    let action_count = text
        .chars()
        .filter(|ch| action_like_chars.contains(ch))
        .count();
    let has_ascii = text.chars().any(|ch| ch.is_ascii_alphanumeric());
    !has_ascii && text.chars().count() >= 6 && action_count >= 2
}

fn is_reasonable_hot_word(text: &str, source: HotWordSource) -> bool {
    let char_count = text.chars().count();
    if source == HotWordSource::User {
        return (1..=MAX_USER_HOT_WORD_CHARS).contains(&char_count)
            && !text.chars().any(|ch| matches!(ch, '\n' | '\r' | '\t'));
    }
    if !(2..=MAX_HOT_WORD_CHARS).contains(&char_count) {
        return false;
    }
    if contains_sentence_punctuation(text) {
        return false;
    }
    if text.split_whitespace().count() > 3 {
        return false;
    }
    if source == HotWordSource::Learned && learned_hot_word_looks_like_sentence(text) {
        return false;
    }
    is_potential_hot_word(text)
}

fn sanitize_hot_words(profile: &mut UserProfile) -> usize {
    let before = profile.hot_words.len();
    let mut deduped = std::collections::HashMap::new();

    for mut hw in std::mem::take(&mut profile.hot_words) {
        let Some((text, key)) = normalize_hot_word_key(&hw.text) else {
            continue;
        };
        hw.text = text;
        hw.weight = hw.weight.clamp(1, 5);
        if profile
            .blocked_hot_words
            .iter()
            .any(|blocked| blocked == &key)
        {
            continue;
        }
        if !is_reasonable_hot_word(&hw.text, hw.source.clone()) {
            continue;
        }
        match deduped.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(hw);
            }
            Entry::Occupied(mut slot) => merge_hot_word(slot.get_mut(), hw),
        }
    }

    profile.hot_words = deduped.into_values().collect();
    profile
        .hot_words
        .sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
    before.saturating_sub(profile.hot_words.len())
}

#[derive(Debug, Default, serde::Serialize)]
pub struct HotWordBatchPreview {
    pub words: Vec<String>,
    pub duplicates: usize,
    pub invalid: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct HotWordBatchResult {
    pub added: usize,
    pub duplicates: usize,
    pub invalid: Vec<String>,
}

/// 按行解析，保留词组内的空格；预览和写入共用同一套校验。
pub fn preview_hot_words(profile: &UserProfile, text: &str) -> HotWordBatchPreview {
    let mut seen: HashSet<String> = profile
        .hot_words
        .iter()
        .filter_map(|word| normalize_hot_word_key(&word.text).map(|(_, key)| key))
        .collect();
    let mut result = HotWordBatchPreview::default();
    for line in text.split(['\n', '\r']) {
        let raw = line.trim();
        let Some((word, key)) = normalize_hot_word_key(raw) else {
            continue;
        };
        // 不把 Excel 多列误合并成一个词条。
        if raw.contains('\t') || !is_reasonable_hot_word(&word, HotWordSource::User) {
            result.invalid.push(raw.to_string());
        } else if !seen.insert(key) {
            result.duplicates += 1;
        } else {
            result.words.push(word);
        }
    }
    result
}

/// 在最新画像上重新校验，并一次性追加，不覆盖已有词条的来源和权重。
pub fn add_hot_words(profile: &mut UserProfile, text: &str) -> HotWordBatchResult {
    let preview = preview_hot_words(profile, text);
    let added = preview.words.len();
    let keys: HashSet<String> = preview
        .words
        .iter()
        .map(|word| word.to_lowercase())
        .collect();
    profile.blocked_hot_words.retain(|key| !keys.contains(key));
    let now = now_secs();
    profile
        .hot_words
        .extend(preview.words.into_iter().map(|text| HotWord {
            text,
            weight: 3,
            source: HotWordSource::User,
            use_count: 0,
            last_used: now,
        }));
    if added > 0 {
        sanitize_hot_words(profile);
        profile.last_updated = now;
    }
    HotWordBatchResult {
        added,
        duplicates: preview.duplicates,
        invalid: preview.invalid,
    }
}

pub fn add_hot_word(profile: &mut UserProfile, text: String, weight: u8) {
    let Some((normalized_text, normalized_key)) = normalize_hot_word_key(&text) else {
        return;
    };
    let now = now_secs();
    profile
        .blocked_hot_words
        .retain(|blocked| blocked != &normalized_key);

    if let Some(existing) = profile.hot_words.iter_mut().find(|h| {
        normalize_hot_word_key(&h.text)
            .map(|(_, k)| k == normalized_key)
            .unwrap_or(false)
    }) {
        existing.text = normalized_text;
        existing.weight = weight.clamp(1, 5);
        existing.source = HotWordSource::User;
        existing.last_used = now;
    } else {
        profile.hot_words.push(HotWord {
            text: normalized_text,
            weight: weight.clamp(1, 5),
            source: HotWordSource::User,
            use_count: 0,
            last_used: now,
        });
    }
    sanitize_hot_words(profile);
    profile.last_updated = now;
}

pub fn remove_hot_word(profile: &mut UserProfile, text: &str) {
    remove_hot_words(profile, &[text.to_string()]);
}

/// 批量删除只遍历一次词库，并保留“不再自动学回”的现有行为。
pub fn remove_hot_words(profile: &mut UserProfile, texts: &[String]) -> usize {
    for text in texts {
        super::hotword_learning::reset(profile, text, now_secs());
    }
    let keys: HashSet<String> = texts
        .iter()
        .filter_map(|text| normalize_hot_word_key(text).map(|(_, key)| key))
        .collect();
    let before = profile.hot_words.len();
    profile
        .hot_words
        .retain(|word| !keys.contains(&word.text.to_lowercase()));
    let removed = before - profile.hot_words.len();
    profile
        .vocab_frequency
        .retain(|word, _| normalize_hot_word_key(word).is_none_or(|(_, key)| !keys.contains(&key)));
    profile
        .hotword_learning
        .words
        .retain(|key, _| !keys.contains(key));
    profile.blocked_hot_words.extend(keys);
    sanitize_blocked_hot_words(profile);
    profile.last_updated = now_secs();
    removed
}

// ============================================================
// 学习
// ============================================================

/// 纠错模式 upsert：已有则递增，否则插入新条目
fn upsert_correction(
    patterns: &mut Vec<CorrectionPattern>,
    orig: &str,
    corrected: &str,
    initial_count: u32,
    source: &CorrectionSource,
    now: u64,
) {
    let orig_len = orig.chars().count();
    let corrected_len = corrected.chars().count();

    if orig.is_empty()
        || corrected.is_empty()
        || orig == corrected
        || orig_len > MAX_SEGMENT_CHARS
        || corrected_len > MAX_SEGMENT_CHARS
    {
        return;
    }

    // 单字符原始片段：仅允许 1:1 字符替换（如 "他"→"它"、"嘛"→"吗"）
    if orig_len == 1 && corrected_len != 1 {
        return;
    }

    // 长度比例过大：可能是对话碎片或句级重写的错误 diff
    let (longer, shorter) = (orig_len.max(corrected_len), orig_len.min(corrected_len));
    if shorter >= 2 && longer > shorter * 3 {
        return;
    }

    // 矛盾检测：已有反向映射则跳过
    if patterns
        .iter()
        .any(|p| p.original == corrected && p.corrected == orig)
    {
        return;
    }
    if let Some(p) = patterns
        .iter_mut()
        .find(|p| p.original == orig && p.corrected == corrected)
    {
        p.count += 1;
        p.last_seen = now;
        if *source == CorrectionSource::User {
            p.source = CorrectionSource::User;
        }
    } else {
        patterns.push(CorrectionPattern {
            original: orig.to_string(),
            corrected: corrected.to_string(),
            count: initial_count,
            last_seen: now,
            source: source.clone(),
        });
    }
}

/// 更新词频统计
fn update_vocab_frequency(
    vocab: &mut std::collections::HashMap<String, VocabEntry>,
    words: impl Iterator<Item = String>,
    now: u64,
) {
    for word in words {
        if word.chars().count() < 2 || !is_potential_hot_word(&word) {
            continue;
        }
        let entry = vocab.entry(word).or_insert(VocabEntry {
            count: 0,
            last_seen: 0,
        });
        entry.count += 1;
        entry.last_seen = now;
    }
}

/// 将高频词汇自动提升为热词
fn promote_vocab_to_hot_words(profile: &mut UserProfile, threshold: u32) {
    let existing: std::collections::HashSet<&str> =
        profile.hot_words.iter().map(|h| h.text.as_str()).collect();

    let new: Vec<HotWord> = profile
        .vocab_frequency
        .iter()
        .filter(|(w, e)| {
            e.count >= threshold
                && !existing.contains(w.as_str())
                && !is_blocked_hot_word(profile, w)
                && w.chars().count() >= 2
                && is_potential_hot_word(w)
        })
        .map(|(w, e)| HotWord {
            text: w.clone(),
            weight: 2,
            source: HotWordSource::Learned,
            use_count: e.count,
            last_used: e.last_seen,
        })
        .collect();

    profile.hot_words.extend(new);
}

/// 学习的公共收尾：限制数量、去重
fn finalize_learning(profile: &mut UserProfile) {
    limit_correction_patterns(profile);
    sanitize_hot_words(profile);
}

/// 从 ASR 原始文本与纠正后文本的字符 diff 中学习
pub fn learn_from_correction(
    profile: &mut UserProfile,
    original: &str,
    polished: &str,
    source: CorrectionSource,
) {
    if original == polished || original.is_empty() || polished.is_empty() {
        return;
    }

    let now = now_secs();
    let initial_count = if source == CorrectionSource::User {
        3
    } else {
        1
    };
    profile.total_transcriptions += 1;
    profile.last_updated = now;

    for (orig_seg, pol_seg) in collect_diff_correction_pairs(&[original], polished) {
        if !super::correction_audit::allow_learning(profile, &orig_seg, &pol_seg, &source) {
            continue;
        }
        upsert_correction(
            &mut profile.correction_patterns,
            &orig_seg,
            &pol_seg,
            initial_count,
            &source,
            now,
        );
    }
    finalize_learning(profile);
}

pub fn collect_diff_correction_pairs(baselines: &[&str], corrected: &str) -> Vec<(String, String)> {
    if corrected.is_empty() {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut pairs = Vec::new();

    for baseline in baselines {
        if baseline.is_empty() || *baseline == corrected {
            continue;
        }
        for (original, updated) in extract_diff_segments(baseline, corrected) {
            if seen.insert((original.clone(), updated.clone())) {
                pairs.push((original, updated));
            }
        }
    }

    pairs
}

/// 从 LLM 结构化输出中学习
pub fn learn_from_structured(
    profile: &mut UserProfile,
    corrections: &[(String, String)],
    key_terms: &[String],
    source: CorrectionSource,
) {
    let now = now_secs();
    let initial_count = if source == CorrectionSource::User {
        3
    } else {
        1
    };
    profile.total_transcriptions += 1;
    profile.last_updated = now;

    for (orig, corrected) in corrections {
        if !super::correction_audit::allow_learning(profile, orig, corrected, &source) {
            continue;
        }
        upsert_correction(
            &mut profile.correction_patterns,
            orig,
            corrected,
            initial_count,
            &source,
            now,
        );
    }

    update_vocab_frequency(
        &mut profile.vocab_frequency,
        key_terms.iter().filter_map(|term| {
            let normalized = normalize_hot_word_text(term);
            is_reasonable_hot_word(&normalized, HotWordSource::Learned).then_some(normalized)
        }),
        now,
    );
    promote_vocab_to_hot_words(profile, 3);
    finalize_learning(profile);
}

// ============================================================
// 辅助函数
// ============================================================

fn is_potential_hot_word(word: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上", "也",
        "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这", "他",
        "她", "它", "们", "那", "个", "什么", "怎么", "这个", "那个", "但是", "因为", "所以",
        "如果", "可以", "已经", "还是", "或者", "然后", "其实", "应该", "可能", "比较", "现在",
        "知道", "觉得", "时候", "这样", "那样",
    ];
    !STOPWORDS.contains(&word)
        && word
            .chars()
            .any(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&c))
}

fn extract_diff_segments(original: &str, polished: &str) -> Vec<(String, String)> {
    let orig: Vec<char> = original.chars().collect();
    let pol: Vec<char> = polished.chars().collect();
    let (olen, plen) = (orig.len(), pol.len());
    let mut diffs = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < olen && j < plen {
        if orig[i] == pol[j] {
            i += 1;
            j += 1;
            continue;
        }

        let max_search = 20;
        let mut found = false;
        let (mut oi, mut oj) = (i + 1, j + 1);

        'outer: for di in 0..max_search.min(olen - i) {
            for dj in 0..max_search.min(plen - j) {
                if (di > 0 || dj > 0) && orig[i + di] == pol[j + dj] {
                    oi = i + di;
                    oj = j + dj;
                    found = true;
                    break 'outer;
                }
            }
        }

        if !found {
            break;
        }
        if oi == i && oj == j {
            i += 1;
            j += 1;
            continue;
        }

        let orig_seg: String = orig[i..oi].iter().collect();
        let pol_seg: String = pol[j..oj].iter().collect();
        if !orig_seg.is_empty() && !pol_seg.is_empty() && orig_seg.len() <= 30 {
            diffs.push((orig_seg, pol_seg));
        }
        i = if oi > i { oi } else { i + 1 };
        j = if oj > j { oj } else { j + 1 };
    }

    diffs
}

#[cfg(test)]
mod tests {
    use super::collect_diff_correction_pairs;

    #[test]
    fn collect_diff_correction_pairs_merges_and_dedupes_baselines() {
        let pairs = collect_diff_correction_pairs(&["abc", "xbc", "abc"], "zbc");
        assert_eq!(
            pairs,
            vec![
                ("a".to_string(), "z".to_string()),
                ("x".to_string(), "z".to_string()),
            ]
        );
    }
}

#[cfg(test)]
mod vocabulary_capacity_tests {
    use super::*;

    #[test]
    fn batch_preview_and_add_dedupe_validate_and_preserve_existing_metadata() {
        let mut profile = UserProfile::default();
        add_hot_word(&mut profile, "PLC".into(), 5);
        let input = format!(
            "  plc\r\nSPC   工作站\nspc 工作站\n狗窝检具\n\n{}\nMES\tHMI",
            "长".repeat(81)
        );
        let preview = preview_hot_words(&profile, &input);
        assert_eq!(preview.words, ["SPC 工作站", "狗窝检具"]);
        assert_eq!(preview.duplicates, 2);
        assert_eq!(preview.invalid.len(), 2);
        assert_eq!(profile.hot_words.len(), 1);
        let result = add_hot_words(&mut profile, &input);
        assert_eq!(result.added, 2);
        assert_eq!(result.duplicates, 2);
        assert_eq!(result.invalid.len(), 2);
        assert_eq!(
            profile
                .hot_words
                .iter()
                .find(|word| word.text == "PLC")
                .unwrap()
                .weight,
            5
        );
    }

    #[test]
    fn batch_add_rechecks_latest_profile_and_handles_2000_terms() {
        let mut profile = UserProfile::default();
        let input = (0..2000)
            .map(|index| format!("Equipment {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(preview_hot_words(&profile, &input).words.len(), 2000);
        add_hot_word(&mut profile, "Equipment 0".into(), 5);
        let result = add_hot_words(&mut profile, &input);
        assert_eq!(result.added, 1999);
        assert_eq!(result.duplicates, 1);
        assert_eq!(profile.hot_words.len(), 2000);
        let mut reloaded: UserProfile =
            serde_json::from_str(&serialize_profile(&profile).unwrap()).unwrap();
        normalize_profile(&mut reloaded);
        assert_eq!(reloaded.hot_words.len(), 2000);
        assert_eq!(reloaded.get_hot_word_texts(100).len(), 100);
        assert_eq!(add_hot_words(&mut reloaded, &input).added, 0);
    }

    #[test]
    fn batch_delete_blocks_relearning_and_explicit_add_unblocks_only_requested_terms() {
        let mut profile = UserProfile::default();
        add_hot_words(&mut profile, "PLC\nMES\nSPC 工作站");
        profile.vocab_frequency.insert(
            "PLC".into(),
            VocabEntry {
                count: 10,
                last_seen: 1,
            },
        );
        let removed = remove_hot_words(&mut profile, &["plc".into(), "MES".into(), "PLC".into()]);
        assert_eq!(removed, 2);
        assert_eq!(profile.hot_words[0].text, "SPC 工作站");
        assert!(is_blocked_hot_word(&profile, "PLC"));
        assert!(profile.vocab_frequency.is_empty());
        add_hot_words(&mut profile, "PLC");
        assert!(!is_blocked_hot_word(&profile, "PLC"));
        assert!(is_blocked_hot_word(&profile, "MES"));
        assert_eq!(profile.hot_words.len(), 2);
    }

    #[test]
    fn vocabulary_preserves_more_than_300_terms_after_reload_and_learning() {
        let mut profile = UserProfile::default();
        for index in 0..600 {
            add_hot_word(&mut profile, format!("Equipment {index}"), 3);
        }
        assert_eq!(profile.hot_words.len(), 600);
        let json = serialize_profile(&profile).unwrap();
        let mut reloaded: UserProfile = serde_json::from_str(&json).unwrap();
        normalize_profile(&mut reloaded);
        assert_eq!(reloaded.hot_words.len(), 600);
        reloaded.hot_words.push(HotWord {
            text: "PLC".into(),
            weight: 5,
            source: HotWordSource::Learned,
            use_count: 10,
            last_used: 1,
        });
        cleanup_profile(&mut reloaded);
        assert_eq!(reloaded.hot_words.len(), 601);
        assert_eq!(
            reloaded
                .hot_words
                .iter()
                .filter(|word| word.source == HotWordSource::User)
                .count(),
            600
        );
    }
}
