//! 内置文字模型：固定模型清单、本地边界与进程状态。
mod download;
mod runtime;
pub use download::{
    cancel_download, download_model, is_downloading, is_model_downloading, remove_model,
};
pub use runtime::{cancel_requests, initialize, load, release, send, shutdown};
pub(crate) use runtime::{gate, stop_locked};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{atomic::AtomicU64, OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalLlmConfig {
    pub model: String,
    pub device: String,
}

impl Default for LocalLlmConfig {
    fn default() -> Self {
        Self {
            model: "qwen3.5-0.8b".into(),
            device: "auto".into(),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    pub license_url: &'static str,
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "qwen3.5-0.8b",
        name: "Qwen3.5-0.8B · Q8",
        repo: "unsloth/Qwen3.5-0.8B-GGUF",
        revision: "6ab461498e2023f6e3c1baea90a8f0fe38ab64d0",
        file: "Qwen3.5-0.8B-Q8_0.gguf",
        size: 811843840,
        sha256: "0ad885ffd4bb022fc4f0d33a3308fa108ef8613159d3b3a67e23abca056b7a6c",
        license: "Apache-2.0",
        license_url: "https://huggingface.co/Qwen/Qwen3.5-0.8B/blob/main/LICENSE",
    },
    ModelSpec {
        id: "lfm2.5-1.2b",
        name: "LFM2.5-1.2B-Instruct · Q8",
        repo: "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
        revision: "8ed288026e23958ad9dfa92d53ed773a8eee7125",
        file: "LFM2.5-1.2B-Instruct-Q8_0.gguf",
        size: 1246253888,
        sha256: "f6b981dcb86917fa463f78a362320bd5e2dc45445df147287eedb85e5a30d26a",
        license: "LFM Open License v1.0",
        license_url: "https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct/blob/main/LICENSE",
    },
];

pub fn model_spec(id: &str) -> Result<&'static ModelSpec, String> {
    MODELS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "不支持的本地模型".into())
}

pub fn model_dir() -> PathBuf {
    crate::utils::paths::get_effective_models_dir().join("light-whisper-local-llm")
}

pub fn model_path(spec: &ModelSpec) -> PathBuf {
    model_dir().join(spec.file)
}

pub fn verify_model(spec: &ModelSpec) -> Result<(), String> {
    download::verify_file(&model_path(spec), spec)
}

pub fn validate_polish_schema(content: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(content).map_err(|_| "本地润色 JSON 无效")?;
    if value["polished"]
        .as_str()
        .is_none_or(|s| s.trim().is_empty())
        || !value["corrections"].is_array()
        || !value["key_terms"].is_array()
    {
        return Err("本地润色输出缺少正文或纠错字段，未应用结果".into());
    }
    if value["corrections"].as_array().unwrap().iter().any(|item| {
        !item["original"].is_string()
            || !item["corrected"].is_string()
            || !matches!(
                item["type"].as_str(),
                Some("homophone" | "term" | "pronoun" | "style")
            )
    }) || value["key_terms"]
        .as_array()
        .unwrap()
        .iter()
        .any(|term| !term.is_string())
    {
        return Err("本地纠错明细格式无效，未应用结果或学习数据".into());
    }
    Ok(())
}

pub fn directory_gate() -> &'static tokio::sync::RwLock<()> {
    static GATE: OnceLock<tokio::sync::RwLock<()>> = OnceLock::new();
    GATE.get_or_init(Default::default)
}

pub fn validate_config(config: &LocalLlmConfig) -> Result<(), String> {
    model_spec(&config.model)?;
    if !matches!(config.device.as_str(), "auto" | "cpu") {
        return Err("无效的本地推理设备".into());
    }
    Ok(())
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub phase: String,
    pub model: Option<String>,
    pub device: Option<String>,
    pub error: Option<String>,
    pub request_id: Option<u64>,
}

pub(super) fn status_cell() -> &'static parking_lot::Mutex<RuntimeStatus> {
    static CELL: OnceLock<parking_lot::Mutex<RuntimeStatus>> = OnceLock::new();
    CELL.get_or_init(|| {
        parking_lot::Mutex::new(RuntimeStatus {
            phase: "unloaded".into(),
            ..Default::default()
        })
    })
}

pub fn status() -> RuntimeStatus {
    status_cell().lock().clone()
}

pub(super) fn set_status(
    app: &tauri::AppHandle,
    phase: &str,
    model: Option<&str>,
    device: Option<&str>,
    error: Option<String>,
    request_id: Option<u64>,
) {
    use tauri::Emitter;
    let status = RuntimeStatus {
        phase: phase.into(),
        model: model.map(str::to_string),
        device: device.map(str::to_string),
        error,
        request_id,
    };
    *status_cell().lock() = status.clone();
    let _ = app.emit("local-llm-status", status);
}

pub(super) static CANCEL_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn validate_request(body: &Value) -> Result<(), String> {
    if body.get("tools").is_some() || body.get("tool_choice").is_some() {
        return Err("本地文字模型不支持联网工具".into());
    }
    if let Some(messages) = body["messages"].as_array().filter(|m| !m.is_empty()) {
        for message in messages {
            if !message["content"].is_string() {
                return Err("本地模型仅接受文字输入".into());
            }
        }
    } else {
        return Err("本地请求缺少文字消息".into());
    }
    Ok(())
}

pub fn validate_completion(
    content: &str,
    finish: Option<&str>,
    structured: bool,
) -> Result<String, String> {
    if finish != Some("stop") {
        return Err("本地模型输出未完整结束，结果未应用".into());
    }
    if content.trim().is_empty() {
        return Err("本地模型返回空结果".into());
    }
    if structured && !serde_json::from_str::<Value>(content).is_ok_and(|v| v.is_object()) {
        return Err("本地模型返回的 JSON 无效，结果未应用".into());
    }
    Ok(content.trim().to_string())
}
