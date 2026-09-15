//! FunASR engine discovery and installation helpers.

use crate::state::AppState;
use crate::utils::{paths, AppError};
use std::path::PathBuf;
use tauri::Emitter;
use tokio::process::Command;

use super::{now_unix_ms, EngineProgressGate, ENGINE_ARCHIVE_FINGERPRINT};

/// 引擎运行模式
pub enum EngineRuntime {
    /// 生产模式：直接运行打包的 engine.exe
    Bundled { exe_path: String },
    /// 开发模式：使用系统 Python 解释器 + .py 脚本
    Development { python_path: String },
}

fn to_normalized_path(path: &std::path::Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    paths::strip_win_prefix(&canonical)
}

fn expected_engine_install_fingerprint(app_handle: &tauri::AppHandle) -> String {
    if paths::get_engine_archive_path(app_handle).is_some() {
        ENGINE_ARCHIVE_FINGERPRINT.to_string()
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

pub(super) fn engine_install_fingerprint_matches(installed: &str, expected: &str) -> bool {
    let installed = installed.trim();
    installed == expected
        || (expected == ENGINE_ARCHIVE_FINGERPRINT
            && installed
                .rsplit_once('+')
                .is_some_and(|(_, archive_fingerprint)| archive_fingerprint == expected))
}

/// 查找引擎运行时
///
/// 按优先级：
/// 1. 已解压到数据目录的 engine.exe（版本匹配时直接复用）
/// 2. 未解压的引擎归档（engine.tar.xz / 兼容 engine.zip）→ 解压后使用
/// 3. 资源目录中的 engine.exe（开发时直接放置 python-dist）
/// 4. 系统 Python（开发模式）
pub async fn find_engine(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    expected_generation: u64,
) -> Result<EngineRuntime, AppError> {
    let progress_gate = EngineProgressGate {
        generation: state.engine.funasr_generation.clone(),
        status_commit: state.engine.funasr_status_commit.clone(),
        expected_generation,
    };
    let _install_guard = state.engine.engine_install_op.lock().await;
    if !progress_gate.is_current() {
        return Err(AppError::Asr("引擎查找已被取消".to_string()));
    }
    let expected_fingerprint = expected_engine_install_fingerprint(app_handle);

    // 策略1：已解压的 engine.exe（版本匹配时使用）
    if let Some(engine_path) = paths::get_engine_exe_path(app_handle) {
        let version_file = paths::get_engine_dir().join(".version");
        let installed_version = std::fs::read_to_string(&version_file).unwrap_or_default();

        if engine_install_fingerprint_matches(&installed_version, &expected_fingerprint) {
            let path_str = paths::strip_win_prefix(&engine_path);
            log::info!("找到引擎: {} ({})", path_str, expected_fingerprint);
            return Ok(EngineRuntime::Bundled { exe_path: path_str });
        }

        log::info!(
            "引擎指纹不匹配 (已安装: {:?}, 当前: {}), 需要重新解压",
            installed_version.trim(),
            expected_fingerprint
        );
    }

    // 策略2：存在引擎归档，需要解压（首次启动或版本升级）
    if let Some(archive_path) = paths::get_engine_archive_path(app_handle) {
        log::info!("找到引擎压缩包，准备解压: {}", archive_path.display());
        let engine_exe = extract_engine_archive(&archive_path, app_handle, &progress_gate).await?;
        let path_str = paths::strip_win_prefix(&engine_exe);
        log::info!("引擎解压完成: {}", path_str);
        return Ok(EngineRuntime::Bundled { exe_path: path_str });
    }

    // 策略3：资源目录中的 engine.exe（开发时直接 PyInstaller 输出）
    if let Some(engine_path) = paths::get_resource_engine_exe_path(app_handle) {
        let path_str = paths::strip_win_prefix(&engine_path);
        log::info!("找到资源目录引擎: {}", path_str);
        return Ok(EngineRuntime::Bundled { exe_path: path_str });
    }

    // 策略4：开发模式
    let python_path = find_python().await?;
    Ok(EngineRuntime::Development { python_path })
}

/// 解压引擎归档到数据目录
async fn extract_engine_archive(
    archive_path: &std::path::Path,
    app_handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<std::path::PathBuf, AppError> {
    progress_gate.commit_if_current(|| {
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "首次启动，正在解压引擎文件..."
            }),
        );
    });

    let engine_dir = paths::get_engine_dir();
    let archive = archive_path.to_path_buf();
    let handle = app_handle.clone();
    let progress_gate = progress_gate.clone();

    // 解压是 CPU 密集型 + IO 密集型，放到阻塞线程
    tokio::task::spawn_blocking(move || {
        let parent = engine_dir
            .parent()
            .ok_or_else(|| AppError::Asr("引擎目录缺少父目录".to_string()))?;
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Asr(format!("创建引擎父目录失败: {}", e)))?;

        let stamp = now_unix_ms();
        let staging_dir = parent.join(format!("engine.staging.{}", stamp));
        let backup_dir = parent.join(format!("engine.backup.{}", stamp));

        if staging_dir.exists() {
            let _ = std::fs::remove_dir_all(&staging_dir);
        }
        std::fs::create_dir_all(&staging_dir)
            .map_err(|e| AppError::Asr(format!("创建引擎目录失败: {}", e)))?;

        let extract_result = (|| -> Result<usize, AppError> {
            let archive_name = archive
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();

            let total = if archive_name.ends_with(".tar.xz") {
                extract_tar_xz_archive(&archive, &staging_dir, &handle, &progress_gate)?
            } else {
                extract_zip_archive(&archive, &staging_dir, &handle, &progress_gate)?
            };

            if total == 0 {
                return Err(AppError::Asr("引擎归档为空".to_string()));
            }
            let staged_exe = staging_dir.join("engine.exe");
            if !staged_exe.is_file() {
                return Err(AppError::Asr("引擎归档缺少 engine.exe".to_string()));
            }

            // 写入版本标记，用于后续升级检测；只写 staging，验证成功后整体替换。
            let fingerprint = expected_engine_install_fingerprint(&handle);
            paths::atomic_write(
                staging_dir.join(".version").as_path(),
                fingerprint.as_bytes(),
            )
            .map_err(|e| AppError::Asr(format!("写入引擎版本标记失败: {}", e)))?;

            Ok(total)
        })();

        let total = match extract_result {
            Ok(total) => total,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return Err(err);
            }
        };

        replace_engine_dir(&engine_dir, &staging_dir, &backup_dir)?;
        let _ = std::fs::remove_dir_all(&backup_dir);

        log::info!("引擎解压完成: {} 个条目", total);
        Ok(engine_dir.join("engine.exe"))
    })
    .await
    .map_err(|e| AppError::Asr(format!("解压任务异常: {}", e)))?
}

pub(super) fn replace_engine_dir(
    engine_dir: &std::path::Path,
    staging_dir: &std::path::Path,
    backup_dir: &std::path::Path,
) -> Result<(), AppError> {
    if backup_dir.exists() {
        let _ = std::fs::remove_dir_all(backup_dir);
    }

    let had_previous = engine_dir.exists();
    if had_previous {
        std::fs::rename(engine_dir, backup_dir)
            .map_err(|e| AppError::Asr(format!("备份旧引擎失败: {}", e)))?;
    }

    match std::fs::rename(staging_dir, engine_dir) {
        Ok(()) => Ok(()),
        Err(err) => {
            if had_previous {
                if let Err(restore_err) = std::fs::rename(backup_dir, engine_dir) {
                    return Err(AppError::Asr(format!(
                        "替换引擎目录失败: {}; 恢复旧引擎也失败: {}（备份保留在 {}）",
                        err,
                        restore_err,
                        backup_dir.display()
                    )));
                }
            }
            Err(AppError::Asr(format!("替换引擎目录失败: {}", err)))
        }
    }
}

fn extract_zip_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<usize, AppError> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("打开引擎压缩包失败: {}", e)))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Asr(format!("读取引擎压缩包失败: {}", e)))?;

    let total = zip.len();
    log::info!("开始解压 ZIP 引擎归档: {} 个文件", total);

    for i in 0..total {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Asr(format!("读取压缩条目失败: {}", e)))?;

        let entry_path = engine_dir.join(
            entry
                .enclosed_name()
                .ok_or_else(|| AppError::Asr("压缩包含不安全路径".to_string()))?,
        );

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)
                .map_err(|e| AppError::Asr(format!("创建目录失败: {}", e)))?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| AppError::Asr(format!("创建父目录失败: {}", e)))?;
            }
            let mut outfile = std::fs::File::create(&entry_path)
                .map_err(|e| AppError::Asr(format!("创建文件失败: {}", e)))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| AppError::Asr(format!("写入文件失败: {}", e)))?;
        }

        report_extract_progress(handle, progress_gate, i + 1, Some(total), false);
    }

    Ok(total)
}

fn extract_tar_xz_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<usize, AppError> {
    log::info!("开始解压 TAR.XZ 引擎归档");

    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("打开引擎压缩包失败: {}", e)))?;
    let decoder = xz2::read::XzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut extracted = 0usize;

    for entry_result in tar
        .entries()
        .map_err(|e| AppError::Asr(format!("读取引擎压缩包失败: {}", e)))?
    {
        let mut entry =
            entry_result.map_err(|e| AppError::Asr(format!("读取压缩条目失败: {}", e)))?;
        entry
            .unpack_in(engine_dir)
            .map_err(|e| AppError::Asr(format!("写入文件失败: {}", e)))?;
        extracted += 1;

        report_extract_progress(handle, progress_gate, extracted, None, false);
    }

    if extracted > 0 && !extracted.is_multiple_of(200) {
        report_extract_progress(handle, progress_gate, extracted, None, true);
    }

    Ok(extracted)
}

fn report_extract_progress(
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
    current: usize,
    total: Option<usize>,
    force: bool,
) {
    let should_emit = force || current.is_multiple_of(200) || total.is_some_and(|t| current == t);

    if !should_emit {
        return;
    }
    progress_gate.commit_if_current(|| {
        if let Some(total) = total {
            if total == 0 {
                return;
            }
            let pct = current * 100 / total;
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "loading",
                    "message": format!("正在解压引擎文件... {}%", pct)
                }),
            );
        } else {
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "loading",
                    "message": format!("正在解压引擎文件... 已处理 {} 项", current)
                }),
            );
        }
    });
}

/// 查找可用的 Python 解释器（开发模式回退）
async fn find_python() -> Result<String, AppError> {
    // ---- 策略1：检查项目 .venv 虚拟环境 ----
    let mut venv_candidates = vec![PathBuf::from(".venv"), PathBuf::from("..").join(".venv")];
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            venv_candidates.push(exe_dir.join("..").join("..").join("..").join(".venv"));
            venv_candidates.push(
                exe_dir
                    .join("..")
                    .join("..")
                    .join("..")
                    .join("..")
                    .join(".venv"),
            );
        }
    }

    for venv_dir in &venv_candidates {
        let venv_python = venv_dir.join("Scripts").join("python.exe");

        if tokio::fs::try_exists(&venv_python).await.unwrap_or(false) {
            let path_str = to_normalized_path(&venv_python);
            log::info!("找到虚拟环境 Python: {}", path_str);
            return Ok(path_str);
        }
    }

    // ---- 策略2：在系统 PATH 中搜索 ----
    // 尝试多个可能的 Python 命令名
    let python_names = vec!["python.exe", "python3.exe", "python"];

    for name in &python_names {
        let check_cmd = Command::new("where").arg(name).output().await;

        if let Ok(output) = check_cmd {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !path.is_empty() {
                    let version_check = Command::new(&path).arg("--version").output().await;

                    if let Ok(ver_output) = version_check {
                        if ver_output.status.success() {
                            let version = String::from_utf8_lossy(&ver_output.stdout);
                            log::info!("找到系统 Python: {} ({})", path, version.trim());
                            return Ok(path);
                        }
                    }
                }
            }
        }
    }

    // 所有策略都失败了
    Err(AppError::Asr(
        "未找到可用的 Python 解释器。请安装 Python 3.8+ 或在项目目录创建 .venv 虚拟环境（推荐使用 uv）。"
            .to_string(),
    ))
}
