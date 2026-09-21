use std::path::Path;

/// 本应用的正式包必须嵌入本地首页。Windows 盘符会被 Tauri 优先识别成 URL。
pub fn validate_frontend_dist(
    manifest_dir: &Path,
    value: &serde_json::Value,
) -> Result<(), String> {
    let relative = value
        .as_str()
        .ok_or("正式构建的 frontendDist 必须是相对目录")?;
    if relative.is_empty()
        || relative.contains(':')
        || relative.starts_with(['/', '\\'])
        || Path::new(relative).is_absolute()
    {
        return Err(
            "frontendDist 必须使用相对目录，禁止 URL 或绝对盘符路径，以确保前端嵌入程序".into(),
        );
    }
    let index = manifest_dir.join(relative).join("index.html");
    if !index.is_file() {
        return Err(format!("前端首页不存在: {}，请先构建前端", index.display()));
    }
    Ok(())
}
