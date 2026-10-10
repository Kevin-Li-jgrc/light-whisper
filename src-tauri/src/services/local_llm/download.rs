use super::{model_dir, model_path, model_spec, ModelSpec};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::{Arc, OnceLock},
};
use tauri::Emitter;

fn downloads() -> &'static parking_lot::Mutex<HashMap<String, Arc<tokio::sync::Notify>>> {
    static CELL: OnceLock<parking_lot::Mutex<HashMap<String, Arc<tokio::sync::Notify>>>> =
        OnceLock::new();
    CELL.get_or_init(Default::default)
}

struct DownloadGuard(String);
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        downloads().lock().remove(&self.0);
    }
}

pub fn is_downloading() -> bool {
    !downloads().lock().is_empty()
}

pub fn is_model_downloading(id: &str) -> bool {
    downloads().lock().contains_key(id)
}

pub fn cancel_download(id: &str) {
    if let Some(cancel) = downloads().lock().get(id) {
        cancel.notify_one();
    }
}

pub fn verify_file(path: &Path, spec: &ModelSpec) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() != spec.size {
        return Err("模型文件大小不匹配".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    if format!("{:x}", hash.finalize()) != spec.sha256 {
        return Err("模型 SHA-256 校验失败，请删除损坏下载后重试".into());
    }
    Ok(())
}

pub async fn download_model(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let _directory = super::directory_gate().read().await;
    let spec = model_spec(&id)?;
    let cancel = Arc::new(tokio::sync::Notify::new());
    {
        let mut active = downloads().lock();
        if active.contains_key(&id) {
            return Err("该模型正在下载".into());
        }
        active.insert(id.clone(), cancel.clone());
    }
    let _guard = DownloadGuard(id.clone());
    let emit = |phase: &str, received: u64, error: Option<&str>| {
        let _ = app.emit("local-llm-download", serde_json::json!({"model":id,"phase":phase,"received":received,"total":spec.size,"error":error}));
    };
    let result = async {
        std::fs::create_dir_all(model_dir()).map_err(|e| e.to_string())?;
        let final_path = model_path(spec);
        if final_path.exists() {
            let path = final_path.clone();
            tokio::task::spawn_blocking(move || verify_file(&path, spec)).await.map_err(|e| e.to_string())??;
            return Ok(());
        }
        let partial = final_path.with_extension("gguf.part");
        let mut received = partial.metadata().map(|m| m.len()).unwrap_or(0);
        if received > spec.size { return Err("下载断点超过模型大小，请删除后重新下载".into()); }
        if received < spec.size {
            let client = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
            let url = format!("https://huggingface.co/{}/resolve/{}/{}", spec.repo, spec.revision, spec.file);
            let mut request = client.get(url);
            if received > 0 { request = request.header(reqwest::header::RANGE, format!("bytes={received}-")); }
            let mut response = tokio::select! {
                _ = cancel.notified() => return Err("下载已取消，保留断点".into()),
                response = request.send() => response.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?,
            };
            let append = received > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
            if append {
                let expected = format!("bytes {received}-");
                if !response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|s| s.to_str().ok()).is_some_and(|s| s.starts_with(&expected)) {
                    return Err("服务器返回无效的续传范围".into());
                }
            } else { received = 0; }
            let mut file = std::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&partial).map_err(|e| e.to_string())?;
            let mut last_emit = std::time::Instant::now();
            emit("downloading", received, None);
            loop {
                let chunk = tokio::select! {
                    _ = cancel.notified() => return Err("下载已取消，保留断点".into()),
                    chunk = tokio::time::timeout(std::time::Duration::from_secs(30), response.chunk()) => chunk.map_err(|_| "下载连接超时，保留断点")?.map_err(|e| e.to_string())?,
                };
                let Some(chunk) = chunk else { break; };
                if received + chunk.len() as u64 > spec.size { return Err("服务器返回文件大小异常".into()); }
                file.write_all(&chunk).map_err(|e| format!("写入模型失败（请检查磁盘空间）: {e}"))?;
                received += chunk.len() as u64;
                if last_emit.elapsed().as_millis() > 200 { emit("downloading", received, None); last_emit = std::time::Instant::now(); }
            }
            file.sync_all().map_err(|e| e.to_string())?;
        }
        emit("verifying", received, None);
        let check = partial.clone();
        tokio::select! {
            _=cancel.notified()=>return Err("下载校验已取消，保留断点".into()),
            result=tokio::task::spawn_blocking(move || verify_file(&check, spec))=>result.map_err(|e|e.to_string())??,
        }
        std::fs::rename(partial, final_path).map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    }.await;
    match &result {
        Ok(()) => emit("ready", spec.size, None),
        Err(e) => emit("error", 0, Some(e)),
    }
    result
}

pub async fn remove_model(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let spec = model_spec(id)?;
    if downloads().lock().contains_key(id) {
        return Err("请先取消下载".into());
    }
    let _gate = super::runtime::gate().lock().await;
    let _directory = super::directory_gate()
        .try_write()
        .map_err(|_| "本地模型正在下载或迁移，请完成或取消后再删除")?;
    super::runtime::stop_locked(app).await;
    for path in [
        model_path(spec),
        model_path(spec).with_extension("gguf.part"),
    ] {
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
