use super::*;
use crate::{services::llm_client::LlmRequestOptions, state::AppState};
use eventsource_stream::Eventsource;
use rand::Rng;
use std::{
    process::Stdio,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tauri::{Emitter, Manager};
use tokio::process::{Child, Command};
use tokio_stream::StreamExt;

struct Process {
    child: Child,
    url: String,
    token: String,
    model: String,
    requested_device: String,
    device: String,
}
struct Connection {
    url: String,
    token: String,
    model: String,
    device: String,
}
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static INTERACTIVE: AtomicUsize = AtomicUsize::new(0);
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

pub(crate) fn gate() -> &'static tokio::sync::Mutex<()> {
    static G: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    G.get_or_init(Default::default)
}
fn process() -> &'static parking_lot::Mutex<Option<Process>> {
    static P: OnceLock<parking_lot::Mutex<Option<Process>>> = OnceLock::new();
    P.get_or_init(Default::default)
}
fn cancellation() -> &'static tokio::sync::Notify {
    static N: OnceLock<tokio::sync::Notify> = OnceLock::new();
    N.get_or_init(Default::default)
}
fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("local HTTP client")
    })
}

pub fn initialize(app: tauri::AppHandle) {
    if APP.set(app.clone()).is_ok() {
        let monitor_app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let mut slot = process().lock();
                let exited = slot
                    .as_mut()
                    .and_then(|p| p.child.try_wait().ok().flatten());
                if let Some(code) = exited {
                    *slot = None;
                    set_status(
                        &monitor_app,
                        "error",
                        None,
                        None,
                        Some(format!("本地引擎已退出 ({code})，下次请求可重新加载")),
                        None,
                    );
                }
            }
        });
    }
    let config = app.state::<AppState>().llm_provider_config();
    if [
        config.resolve_active_provider(),
        config.resolve_assistant_provider(),
        config.resolve_selection_provider(),
        config.resolve_validation_provider(),
    ]
    .iter()
    .any(|p| p == "local")
    {
        tauri::async_runtime::spawn(async move {
            if let Err(e) = load(&app).await {
                set_status(&app, "error", None, None, Some(e), None);
            }
        });
    }
}

pub fn cancel_requests() {
    CANCEL_GENERATION.fetch_add(1, Ordering::SeqCst);
    cancellation().notify_waiters();
}

pub(crate) async fn stop_locked(app: &tauri::AppHandle) {
    let current = process().lock().take();
    if let Some(mut p) = current {
        let _ = p.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), p.child.wait()).await;
    }
    set_status(app, "unloaded", None, None, None, None);
}

pub async fn release(app: &tauri::AppHandle) {
    let _gate = gate().lock().await;
    stop_locked(app).await;
}
pub async fn shutdown(app: &tauri::AppHandle) {
    cancel_requests();
    release(app).await;
}

fn runtime_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/local-llm"));
    }
    app.path()
        .resource_dir()
        .map(|p| p.join("resources/local-llm"))
        .map_err(|e| e.to_string())
}

async fn start(
    app: &tauri::AppHandle,
    config: &LocalLlmConfig,
    backend: &str,
) -> Result<Process, String> {
    let spec = model_spec(&config.model)?;
    let path = model_path(spec);
    if !path.exists() {
        return Err("请先下载本地文字模型".into());
    }
    let verify = path.clone();
    tokio::task::spawn_blocking(move || super::download::verify_file(&verify, spec))
        .await
        .map_err(|e| e.to_string())??;
    let root = runtime_root(app)?.join(backend);
    let binary = root.join("llama-server.exe");
    if !binary.exists() {
        return Err(format!("缺少 {backend} 本地推理组件，请重新安装完整版本"));
    }
    if backend == "cuda" {
        let mut probe = Command::new(&binary);
        probe
            .current_dir(&root)
            .arg("--list-devices")
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        probe.creation_flags(0x08000000);
        let output = tokio::time::timeout(Duration::from_secs(10), probe.output())
            .await
            .map_err(|_| "GPU 检测超时")?
            .map_err(|e| e.to_string())?;
        if !output.status.success() || !String::from_utf8_lossy(&output.stdout).contains("CUDA0:") {
            return Err("未检测到可用 NVIDIA GPU".into());
        }
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(48)
        .map(char::from)
        .collect();
    let url = format!("http://127.0.0.1:{port}");
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).clamp(1, 8))
        .unwrap_or(2);
    let mut command = Command::new(binary);
    command
        .current_dir(&root)
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--model",
        ])
        .arg(path)
        .args([
            "--alias",
            "local-selected",
            "--ctx-size",
            "8192",
            "--parallel",
            "1",
            "--n-predict",
            "2048",
            "--threads",
            &threads.to_string(),
            "--batch-size",
            "256",
            "--ubatch-size",
            "128",
            "--no-webui",
            "--no-context-shift",
            "--chat-template-kwargs",
            "{\"enable_thinking\":false}",
            "--gpu-layers",
            if backend == "cuda" { "99" } else { "0" },
        ])
        .env("LLAMA_API_KEY", &token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000);
    command.args([
        "--fit",
        "off",
        "--reasoning",
        "off",
        "--device",
        if backend == "cuda" { "CUDA0" } else { "none" },
    ]);
    drop(listener);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let ready = tokio::time::timeout(Duration::from_secs(115), async {
        loop {
            if let Some(code) = child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!("{backend} 引擎启动失败 ({code})"));
            }
            if let Ok(response) = client()
                .get(format!("{url}/v1/models"))
                .bearer_auth(&token)
                .send()
                .await
            {
                if response.status().is_success() {
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .map_err(|_| "本地模型加载超时".to_string())?;
    ready?;
    // 使用一个短请求预热模板和推理路径，之后保持驻留。
    client().post(format!("{url}/v1/chat/completions")).bearer_auth(&token)
        .json(&serde_json::json!({"messages":[{"role":"user","content":"Hi"}],"max_tokens":1,"stream":false,"chat_template_kwargs":{"enable_thinking":false}}))
        .send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
    Ok(Process {
        child,
        url,
        token,
        model: config.model.clone(),
        requested_device: config.device.clone(),
        device: backend.into(),
    })
}

async fn ensure_locked(app: &tauri::AppHandle) -> Result<(), String> {
    let config = app.state::<AppState>().llm_provider_config().local;
    validate_config(&config)?;
    {
        let mut p = process().lock();
        if let Some(p) = p.as_mut() {
            if p.model == config.model
                && p.requested_device == config.device
                && p.child.try_wait().map_err(|e| e.to_string())?.is_none()
            {
                return Ok(());
            }
        }
    }
    stop_locked(app).await;
    set_status(app, "loading", Some(&config.model), None, None, None);
    let loaded = tokio::time::timeout(Duration::from_secs(120), async {
        if config.device == "auto" {
            match start(app, &config, "cuda").await {
                Ok(p) => return Ok(p),
                Err(e) => log::warn!("本地 GPU 不可用，退回 CPU: {e}"),
            }
        }
        start(app, &config, "cpu").await
    })
    .await
    .map_err(|_| "本地模型加载超过 120 秒".to_string())??;
    set_status(
        app,
        "ready",
        Some(&loaded.model),
        Some(&loaded.device),
        None,
        None,
    );
    *process().lock() = Some(loaded);
    Ok(())
}

pub async fn load(app: &tauri::AppHandle) -> Result<(), String> {
    set_status(app, "loading", None, None, None, None);
    let cancelled = cancellation().notified();
    tokio::pin!(cancelled);
    cancelled.as_mut().enable();
    let _gate = gate().lock().await;
    let result = tokio::select! {
        _=&mut cancelled => Err("本地请求已取消".into()),
        result=ensure_locked(app) => result,
    };
    match &result {
        Ok(()) => {
            let slot = process().lock();
            if let Some(p) = slot.as_ref() {
                set_status(app, "ready", Some(&p.model), Some(&p.device), None, None);
            }
        }
        Err(error) => set_status(app, "error", None, None, Some(error.clone()), None),
    }
    result
}

struct InteractiveGuard(bool);
impl Drop for InteractiveGuard {
    fn drop(&mut self) {
        if self.0 {
            INTERACTIVE.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

// 调用方取消 future 时也结束正在生成的进程，避免残留生成占据下一个会话。
struct GenerationGuard(bool);
impl Drop for GenerationGuard {
    fn drop(&mut self) {
        if self.0 {
            let mut slot = process().lock();
            if let Some(p) = slot.as_mut() {
                p.model.clear(); // 下一次请求必须先回收被取消的进程，不能复用尚在退出的连接。
                let _ = p.child.start_kill();
            }
        }
    }
}

pub async fn send(
    body: &Value,
    app: Option<&tauri::AppHandle>,
    options: LlmRequestOptions<'_>,
) -> Result<String, String> {
    let app = app.or(APP.get()).ok_or("本地模型服务尚未初始化")?;
    validate_request(body)?;
    let interactive = options.session_id.is_some() || options.stream_event.is_some();
    if interactive {
        INTERACTIVE.fetch_add(1, Ordering::SeqCst);
    }
    let _interactive = InteractiveGuard(interactive);
    let generation = CANCEL_GENERATION.load(Ordering::SeqCst);
    let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    let started = tokio::time::Instant::now();
    let deadline = started + Duration::from_secs(180);
    let _ = app.emit(
        "local-llm-request",
        serde_json::json!({"requestId":request_id,"sessionId":options.session_id,"phase":"queued"}),
    );
    let lock=tokio::time::timeout_at(deadline,async {
        loop {
            if generation!=CANCEL_GENERATION.load(Ordering::SeqCst) { return Err("本地请求已取消".to_string()); }
            if !interactive && INTERACTIVE.load(Ordering::SeqCst)>0 { tokio::time::sleep(Duration::from_millis(50)).await; continue; }
            let notified=cancellation().notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if generation!=CANCEL_GENERATION.load(Ordering::SeqCst) { return Err("本地请求已取消".into()); }
            let guard=tokio::select! { guard=gate().lock()=>guard, _=&mut notified=>return Err("本地请求已取消".into()) };
            if !interactive && INTERACTIVE.load(Ordering::SeqCst)>0 { drop(guard); continue; }
            return Ok(guard);
        }
    }).await.map_err(|_| "本地请求排队超时".to_string())??;
    let cancelled = cancellation().notified();
    tokio::pin!(cancelled);
    cancelled.as_mut().enable();
    if generation != CANCEL_GENERATION.load(Ordering::SeqCst) {
        return Err("本地请求已取消".into());
    }
    let mut generation_guard = GenerationGuard(true);
    let result = tokio::select! {
        _=&mut cancelled => Err("本地请求已取消".into()),
        result=tokio::time::timeout_at(deadline,run(app,body,options,request_id,started)) => result.unwrap_or_else(|_|Err("本地请求超过 180 秒，结果未应用".into())),
    };
    if let Err(ref error) = result {
        stop_locked(app).await;
        set_status(
            app,
            "error",
            None,
            None,
            Some(error.clone()),
            Some(request_id),
        );
    } else {
        let p = process().lock();
        if let Some(p) = p.as_ref() {
            set_status(app, "ready", Some(&p.model), Some(&p.device), None, None);
        }
    }
    drop(lock);
    generation_guard.0 = false;
    result
}

async fn run(
    app: &tauri::AppHandle,
    body: &Value,
    options: LlmRequestOptions<'_>,
    request_id: u64,
    started: tokio::time::Instant,
) -> Result<String, String> {
    ensure_locked(app).await?;
    let p = {
        let slot = process().lock();
        let p = slot.as_ref().ok_or("本地引擎未就绪")?;
        Connection {
            url: p.url.clone(),
            token: p.token.clone(),
            model: p.model.clone(),
            device: p.device.clone(),
        }
    };
    set_status(
        app,
        "generating",
        Some(&p.model),
        Some(&p.device),
        None,
        Some(request_id),
    );
    let mut body = body.clone();
    body["model"] = serde_json::json!("local-selected");
    body["stream"] = serde_json::json!(true);
    body["max_tokens"] = serde_json::json!(2048);
    let rendered: Value = client()
        .post(format!("{}/apply-template", p.url))
        .bearer_auth(&p.token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let prompt = rendered["prompt"]
        .as_str()
        .ok_or("本地引擎无法计算完整提示词长度")?;
    let tokenized: Value = client()
        .post(format!("{}/tokenize", p.url))
        .bearer_auth(&p.token)
        .json(&serde_json::json!({"content":prompt,"add_special":true}))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let input_tokens = tokenized["tokens"]
        .as_array()
        .ok_or("本地引擎分词失败")?
        .len();
    if input_tokens + 2048 > 8192 {
        return Err("输入超过本地模型上下文预算，请缩短文本或新建对话；未截断原文".into());
    }
    let queue_ms = started.elapsed().as_millis();
    let request_started = std::time::Instant::now();
    let response = client()
        .post(format!("{}/v1/chat/completions", p.url))
        .bearer_auth(&p.token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("本地推理请求失败: {}", response.status()));
    }
    let stream = response.bytes_stream().eventsource();
    tokio::pin!(stream);
    let mut text = String::new();
    let mut finish = None;
    let mut first_ms = None;
    while let Some(event) = stream.next().await {
        let event = event.map_err(|e| format!("本地流中断: {e}"))?;
        if event.data == "[DONE]" {
            break;
        }
        let data: Value = serde_json::from_str(&event.data).map_err(|e| e.to_string())?;
        if data.get("error").is_some() {
            return Err("本地模型返回推理错误".into());
        }
        if let Some(reason) = data["choices"][0]["finish_reason"].as_str() {
            finish = Some(reason.to_string());
        }
        if let Some(chunk) = data["choices"][0]["delta"]["content"].as_str() {
            if !chunk.is_empty() {
                first_ms.get_or_insert(request_started.elapsed().as_millis());
            }
            text.push_str(chunk);
            if let Some(event) = options.stream_event {
                let _=app.emit(event,serde_json::json!({"status":"streaming","sessionId":options.session_id,"chunk":chunk,"tokens":text.chars().count()}));
            }
        }
    }
    let result = validate_completion(&text, finish.as_deref(), options.json_output)?;
    let _=app.emit("local-llm-timing",serde_json::json!({"requestId":request_id,"sessionId":options.session_id,"model":p.model,"device":p.device,"queueAndLoadMs":queue_ms,"firstTokenMs":first_ms,"generationMs":request_started.elapsed().as_millis(),"totalMs":started.elapsed().as_millis(),"inputTokens":input_tokens}));
    Ok(result)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_reaps_child_even_when_monitor_holds_state_lock() {
        let _gate = gate().lock().await;
        let child = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .creation_flags(0x08000000)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        *process().lock() = Some(Process {
            child,
            url: String::new(),
            token: String::new(),
            model: "test".into(),
            requested_device: "cpu".into(),
            device: "cpu".into(),
        });
        let (ready_rx_tx, ready_rx) = std::sync::mpsc::channel();
        let monitor = std::thread::spawn(move || {
            let _guard = process().lock();
            ready_rx_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(30));
        });
        ready_rx.recv().unwrap();
        drop(GenerationGuard(true));
        monitor.join().unwrap();
        let mut p = process().lock().take().unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), p.child.wait())
            .await
            .is_ok());
    }
}
