use crate::{
    services::{llm_provider, local_llm, profile_service},
    state::AppState,
};
use serde_json::{json, Value};

#[tauri::command]
pub async fn local_llm_models() -> Result<Value, String> {
    tokio::task::spawn_blocking(|| {
        let models: Vec<Value> = local_llm::MODELS.iter().map(|spec| {
            let path=local_llm::model_path(spec);
            let license=if spec.id=="lfm2.5-1.2b" { include_str!("../../resources/local-llm/LFM-LICENSE.txt") } else { include_str!("../../resources/local-llm/Qwen-LICENSE.txt") };
            json!({"spec":spec,"downloadActive":local_llm::is_model_downloading(spec.id),"licenseText":license,"filePresent":path.exists(),"downloaded":local_llm::verify_model(spec).is_ok(),"partialBytes":path.with_extension("gguf.part").metadata().map(|m|m.len()).unwrap_or(0)})
        }).collect();
        json!(models)
    }).await.map_err(|e|e.to_string())
}

#[tauri::command]
pub fn local_llm_status() -> local_llm::RuntimeStatus {
    local_llm::status()
}

#[tauri::command]
pub async fn local_llm_download(app: tauri::AppHandle, model: String) -> Result<(), String> {
    local_llm::download_model(app, model).await
}

#[tauri::command]
pub fn local_llm_cancel_download(model: String) {
    local_llm::cancel_download(&model);
}

#[tauri::command]
pub async fn local_llm_delete(app: tauri::AppHandle, model: String) -> Result<(), String> {
    local_llm::remove_model(&app, &model).await
}

#[tauri::command]
pub async fn local_llm_load(app: tauri::AppHandle) -> Result<(), String> {
    local_llm::load(&app).await
}

#[tauri::command]
pub async fn local_llm_release(app: tauri::AppHandle) {
    local_llm::release(&app).await;
}

#[tauri::command]
pub fn local_llm_cancel() {
    local_llm::cancel_requests();
}

#[tauri::command]
pub async fn local_llm_configure(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
    device: String,
) -> Result<(), String> {
    let config = local_llm::LocalLlmConfig { model, device };
    local_llm::validate_config(&config)?;
    profile_service::update_profile_and_schedule(state.inner(), |p| p.llm_provider.local = config);
    // load 持有串行锁，等待正在执行的任务结束再切换。
    local_llm::load(&app).await
}

#[tauri::command]
pub fn local_llm_switch_all(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    restore: bool,
) -> Result<(), String> {
    if restore && state.llm_provider_config().local_cloud_backup.is_none() {
        return Err("没有保存的云端配置".into());
    }
    profile_service::update_profile_and_schedule(state.inner(), |p| {
        let config = &mut p.llm_provider;
        if restore {
            if let Some(backup) = config.local_cloud_backup.take() {
                let local = config.local.clone();
                *config = *backup;
                config.local = local;
            }
        } else {
            if config.local_cloud_backup.is_none() {
                config.local_cloud_backup = Some(Box::new(config.clone()));
            }
            config.active = "local".into();
            config.assistant_use_separate_model = true;
            config.assistant_provider = Some("local".into());
            config.selection_use_separate_model = true;
            config.selection_provider = Some("local".into());
            config.validation_use_separate_model = true;
            config.validation_provider = Some("local".into());
        }
    });
    llm_provider::sync_runtime_api_key(&app, state.inner());
    local_llm::initialize(app);
    Ok(())
}
