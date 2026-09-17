use std::time::Duration;

use serde_json::Value;

use crate::services::codex_oauth_service;
use crate::services::grok_build_oauth_service;
use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::{ApiFormat, LlmReasoningMode};

use super::protocol::{
    adapt_body_for_backend, cached_output_token_limit_unsupported, ensure_non_empty_llm_content,
    extract_api_error_message, extract_content, has_output_token_limit,
    looks_like_output_token_limit_unsupported_error, remember_output_token_limit_unsupported,
    responses_api_url, strip_output_token_limits, uses_codex_chatgpt_backend,
    uses_deepseek_v4_responses, uses_grok_build_oauth_backend, uses_responses_api,
};
use super::request::LlmRequestOptions;
use super::stream::{
    read_anthropic_sse_stream, read_openai_responses_sse_stream, read_sse_stream,
    stream_progress_timeout, stream_total_timeout,
};

const RETRYABLE_429_DELAYS_MS: &[u64] = &[600, 1200];

pub(crate) fn dynamic_timeout(
    base_secs: u64,
    text_len: usize,
    body: &Value,
    web_search: bool,
) -> Duration {
    let extra = (text_len / 200) as u64;
    let image_context_len = estimate_image_context_len(body);
    let image_extra = (image_context_len / (512 * 1024)) as u64 * 10;
    let tool_extra = if web_search { 45 } else { 0 };
    let total = base_secs
        .saturating_add(extra)
        .saturating_add(image_extra)
        .saturating_add(tool_extra);
    Duration::from_secs(total.min(base_secs.max(240)))
}

fn estimate_image_context_len(value: &Value) -> usize {
    fn visit(key: Option<&str>, value: &Value) -> usize {
        match value {
            Value::String(s) => match key {
                Some("image_url") if s.starts_with("data:image/") => s.len(),
                Some("url") if s.starts_with("data:image/") => s.len(),
                Some("data") if s.len() > 1024 => s.len(),
                _ => 0,
            },
            Value::Array(items) => items.iter().map(|item| visit(None, item)).sum(),
            Value::Object(map) => map
                .iter()
                .map(|(key, value)| visit(Some(key.as_str()), value))
                .sum(),
            _ => 0,
        }
    }

    visit(None, value)
}
pub(crate) fn is_retryable_overload_error(status: reqwest::StatusCode, message: &str) -> bool {
    if status != reqwest::StatusCode::TOO_MANY_REQUESTS {
        return false;
    }

    let normalized = message.to_ascii_lowercase();
    normalized.contains("queue_exceeded")
        || normalized.contains("high traffic")
        || normalized.contains("too many requests")
        || normalized.contains("rate limit")
}

pub(crate) fn request_url_for_backend<'a>(endpoint: &'a LlmEndpoint, api_key: &str) -> &'a str {
    if uses_codex_chatgpt_backend(endpoint, api_key) {
        codex_oauth_service::CHATGPT_CODEX_RESPONSES_URL
    } else if uses_grok_build_oauth_backend(endpoint, api_key) {
        grok_build_oauth_service::GROK_BUILD_RESPONSES_URL
    } else {
        endpoint.api_url.as_str()
    }
}

pub async fn send_llm_request(
    http_client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
    body: &Value,
    text_len: usize,
    app_handle: Option<&tauri::AppHandle>,
    options: LlmRequestOptions<'_>,
) -> Result<String, String> {
    let deepseek_responses_endpoint = uses_deepseek_v4_responses(endpoint).then(|| LlmEndpoint {
        provider: endpoint.provider.clone(),
        api_url: responses_api_url(&endpoint.api_url),
        model: endpoint.model.clone(),
        timeout_secs: endpoint.timeout_secs,
        api_format: endpoint.api_format.clone(),
    });
    let endpoint = deepseek_responses_endpoint.as_ref().unwrap_or(endpoint);
    let mut headers = llm_provider::build_request_headers(endpoint, api_key, options.session_id)
        .map_err(|e| format!("构建请求头失败: {e}"))?;
    if uses_grok_build_oauth_backend(endpoint, api_key) {
        if let Some(token) = grok_build_oauth_service::decode_grok_build_oauth_access_token(api_key)
        {
            headers = grok_build_oauth_service::grok_cli_request_headers(&token)
                .map_err(|e| format!("构建 Grok Build 请求头失败: {e}"))?;
        }
    }
    if uses_codex_chatgpt_backend(endpoint, api_key) {
        if let Some(session_id) = options.session_id {
            let header = session_id.to_string();
            if let Ok(value) = header.parse::<reqwest::header::HeaderValue>() {
                headers.insert("session_id", value);
            }
        }
    }
    let mut request_body =
        adapt_body_for_backend(endpoint, api_key, body, options.openai_fast_mode);
    if cached_output_token_limit_unsupported(endpoint) {
        strip_output_token_limits(&mut request_body);
    }
    let timeout = dynamic_timeout(
        endpoint.timeout_secs,
        text_len,
        &request_body,
        options.web_search,
    );
    let transport_stream = request_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let requested_stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let mut remember_initial_auto_reasoning_strategy = true;

    async fn dispatch_request(
        http_client: &reqwest::Client,
        endpoint: &LlmEndpoint,
        api_key: &str,
        headers: reqwest::header::HeaderMap,
        body: &Value,
        timeout: Duration,
    ) -> Result<reqwest::Response, String> {
        let request_url = request_url_for_backend(endpoint, api_key);
        let request = http_client.post(request_url).headers(headers);
        tokio::time::timeout(timeout, request.json(body).send())
            .await
            .map_err(|_| format!("请求超时（{} 秒）", timeout.as_secs()))?
            .map_err(|e| format!("请求失败: {}", e))
    }

    struct ReasoningRetryContext<'a> {
        http_client: &'a reqwest::Client,
        endpoint: &'a LlmEndpoint,
        api_key: &'a str,
        headers: &'a reqwest::header::HeaderMap,
        timeout: Duration,
        mode: LlmReasoningMode,
    }

    async fn retry_after_reasoning_rejection(
        ctx: &ReasoningRetryContext<'_>,
        base_body: &Value,
        initial_status: reqwest::StatusCode,
        mut error_message: String,
    ) -> Result<reqwest::Response, String> {
        let endpoint = ctx.endpoint;
        let is_responses_api = uses_responses_api(endpoint);
        if llm_provider::cached_auto_reasoning_strategy(endpoint, is_responses_api, ctx.mode)
            == Some(llm_provider::AutoReasoningStrategy::NoControls)
        {
            return Err(format!(
                "API 返回错误 {}: {}",
                initial_status, error_message
            ));
        }

        let mut output_token_limit_unsupported = false;
        for (strategy, mut fallback_body) in llm_provider::auto_reasoning_fallback_bodies(
            endpoint,
            is_responses_api,
            base_body,
            ctx.mode,
        ) {
            log::warn!(
                "当前模型拒绝推理参数，尝试自动探测策略 {}: provider={}, model={}, err={}",
                strategy.strategy_name(),
                endpoint.provider,
                endpoint.model,
                error_message
            );
            let retry_response = dispatch_request(
                ctx.http_client,
                endpoint,
                ctx.api_key,
                ctx.headers.clone(),
                &fallback_body,
                ctx.timeout,
            )
            .await?;
            if retry_response.status().is_success() {
                llm_provider::remember_auto_reasoning_strategy(
                    endpoint,
                    is_responses_api,
                    ctx.mode,
                    strategy,
                );
                return Ok(retry_response);
            }

            let mut status = retry_response.status();
            let mut body_text = retry_response.text().await.unwrap_or_default();
            error_message = extract_api_error_message(endpoint, &body_text);
            if looks_like_output_token_limit_unsupported_error(&error_message)
                && has_output_token_limit(&fallback_body)
            {
                output_token_limit_unsupported = true;
                log::warn!(
                    "当前后端不支持输出长度参数，已移除后继续推理参数探测: provider={}, model={}, err={}",
                    endpoint.provider,
                    endpoint.model,
                    error_message
                );
                strip_output_token_limits(&mut fallback_body);
                let retry_response = dispatch_request(
                    ctx.http_client,
                    endpoint,
                    ctx.api_key,
                    ctx.headers.clone(),
                    &fallback_body,
                    ctx.timeout,
                )
                .await?;
                if retry_response.status().is_success() {
                    remember_output_token_limit_unsupported(endpoint);
                    llm_provider::remember_auto_reasoning_strategy(
                        endpoint,
                        is_responses_api,
                        ctx.mode,
                        strategy,
                    );
                    return Ok(retry_response);
                }

                status = retry_response.status();
                body_text = retry_response.text().await.unwrap_or_default();
                error_message = extract_api_error_message(endpoint, &body_text);
            }
            if !llm_provider::looks_like_reasoning_unsupported_error(&error_message) {
                return Err(format!("API 返回错误 {}: {}", status, error_message));
            }
        }

        log::warn!(
            "当前模型不支持推理参数，已移除后自动重试: provider={}, model={}, err={}",
            endpoint.provider,
            endpoint.model,
            error_message
        );
        let mut fallback_body = base_body.clone();
        llm_provider::strip_reasoning_controls(&mut fallback_body);
        if output_token_limit_unsupported {
            strip_output_token_limits(&mut fallback_body);
        }
        let retry_response = dispatch_request(
            ctx.http_client,
            endpoint,
            ctx.api_key,
            ctx.headers.clone(),
            &fallback_body,
            ctx.timeout,
        )
        .await?;
        if !retry_response.status().is_success() {
            let mut status = retry_response.status();
            let mut body_text = retry_response.text().await.unwrap_or_default();
            let mut error_message = extract_api_error_message(endpoint, &body_text);
            if looks_like_output_token_limit_unsupported_error(&error_message)
                && has_output_token_limit(&fallback_body)
            {
                log::warn!(
                    "当前后端不支持输出长度参数，已移除后继续无推理参数重试: provider={}, model={}, err={}",
                    endpoint.provider,
                    endpoint.model,
                    error_message
                );
                strip_output_token_limits(&mut fallback_body);
                let retry_response = dispatch_request(
                    ctx.http_client,
                    endpoint,
                    ctx.api_key,
                    ctx.headers.clone(),
                    &fallback_body,
                    ctx.timeout,
                )
                .await?;
                if retry_response.status().is_success() {
                    remember_output_token_limit_unsupported(endpoint);
                    if llm_provider::is_auto_reasoning_endpoint(endpoint, is_responses_api) {
                        llm_provider::remember_auto_reasoning_strategy(
                            endpoint,
                            is_responses_api,
                            ctx.mode,
                            llm_provider::AutoReasoningStrategy::NoControls,
                        );
                    }
                    return Ok(retry_response);
                }
                status = retry_response.status();
                body_text = retry_response.text().await.unwrap_or_default();
                error_message = extract_api_error_message(endpoint, &body_text);
            }
            return Err(format!("API 返回错误 {}: {}", status, error_message));
        }
        if output_token_limit_unsupported {
            remember_output_token_limit_unsupported(endpoint);
        }
        if llm_provider::is_auto_reasoning_endpoint(endpoint, is_responses_api) {
            llm_provider::remember_auto_reasoning_strategy(
                endpoint,
                is_responses_api,
                ctx.mode,
                llm_provider::AutoReasoningStrategy::NoControls,
            );
        }
        Ok(retry_response)
    }

    let mut response = dispatch_request(
        http_client,
        endpoint,
        api_key,
        headers.clone(),
        &request_body,
        timeout,
    )
    .await?;

    if !response.status().is_success() {
        let mut status = response.status();
        let mut body_text = response.text().await.unwrap_or_default();
        let mut error_message = extract_api_error_message(endpoint, &body_text);
        let mut successful_retry: Option<reqwest::Response> = None;
        let reasoning_retry_context = ReasoningRetryContext {
            http_client,
            endpoint,
            api_key,
            headers: &headers,
            timeout,
            mode: options.reasoning_mode,
        };

        if is_retryable_overload_error(status, &error_message) {
            for delay_ms in RETRYABLE_429_DELAYS_MS {
                log::warn!(
                    "LLM 请求遇到可重试的 429，延迟 {}ms 后重试: provider={}, model={}, err={}",
                    delay_ms,
                    endpoint.provider,
                    endpoint.model,
                    error_message
                );
                tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
                let retry_response = dispatch_request(
                    http_client,
                    endpoint,
                    api_key,
                    headers.clone(),
                    &request_body,
                    timeout,
                )
                .await?;
                if retry_response.status().is_success() {
                    successful_retry = Some(retry_response);
                    break;
                }
                status = retry_response.status();
                let retry_body_text = retry_response.text().await.unwrap_or_default();
                error_message = extract_api_error_message(endpoint, &retry_body_text);
                if !is_retryable_overload_error(status, &error_message) {
                    break;
                }
            }
        }

        if let Some(retry_response) = successful_retry {
            response = retry_response;
        } else if looks_like_output_token_limit_unsupported_error(&error_message)
            && has_output_token_limit(&request_body)
        {
            log::warn!(
                "当前后端不支持输出长度参数，已移除后自动重试: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                error_message
            );
            let mut fallback_body = request_body.clone();
            strip_output_token_limits(&mut fallback_body);
            response = dispatch_request(
                http_client,
                endpoint,
                api_key,
                headers.clone(),
                &fallback_body,
                timeout,
            )
            .await?;
            if !response.status().is_success() {
                status = response.status();
                body_text = response.text().await.unwrap_or_default();
                error_message = extract_api_error_message(endpoint, &body_text);
                if options.reasoning_mode != LlmReasoningMode::ProviderDefault
                    && llm_provider::looks_like_reasoning_unsupported_error(&error_message)
                {
                    remember_initial_auto_reasoning_strategy = false;
                    response = retry_after_reasoning_rejection(
                        &reasoning_retry_context,
                        &fallback_body,
                        status,
                        error_message,
                    )
                    .await?;
                    remember_output_token_limit_unsupported(endpoint);
                } else {
                    return Err(format!("API 返回错误 {}: {}", status, error_message));
                }
            } else {
                remember_output_token_limit_unsupported(endpoint);
            }
        } else if options.reasoning_mode != LlmReasoningMode::ProviderDefault
            && llm_provider::looks_like_reasoning_unsupported_error(&error_message)
        {
            remember_initial_auto_reasoning_strategy = false;
            response = retry_after_reasoning_rejection(
                &reasoning_retry_context,
                &request_body,
                status,
                error_message,
            )
            .await?;
        } else {
            return Err(format!("API 返回错误 {}: {}", status, error_message));
        }
    }

    if remember_initial_auto_reasoning_strategy
        && options.reasoning_mode != LlmReasoningMode::ProviderDefault
    {
        let is_responses_api = uses_responses_api(endpoint);
        if llm_provider::is_auto_reasoning_endpoint(endpoint, is_responses_api) {
            if let Some(strategy) = llm_provider::applied_auto_reasoning_strategy(&request_body) {
                llm_provider::remember_auto_reasoning_strategy(
                    endpoint,
                    is_responses_api,
                    options.reasoning_mode,
                    strategy,
                );
            }
        }
    }

    // 根据 body 中实际是否启用了 stream 来决定响应解析方式
    // （build_llm_body 可能因供应商限制而跳过 stream，如 Cerebras json_object 不兼容流式）
    if transport_stream {
        if requested_stream && app_handle.is_none() {
            return Err("流式请求缺少 app_handle".to_string());
        }
        let stream_app_handle = if requested_stream { app_handle } else { None };
        let progress_timeout = stream_progress_timeout(options);
        let total_timeout = stream_total_timeout(options);
        match endpoint.api_format {
            ApiFormat::Anthropic => {
                read_anthropic_sse_stream(
                    endpoint,
                    response,
                    stream_app_handle,
                    options.stream_event,
                    options.session_id,
                    progress_timeout,
                    total_timeout,
                )
                .await
            }
            ApiFormat::OpenaiCompat => {
                if uses_responses_api(endpoint) {
                    read_openai_responses_sse_stream(
                        response,
                        endpoint,
                        stream_app_handle,
                        options.stream_event,
                        options.session_id,
                        progress_timeout,
                        total_timeout,
                    )
                    .await
                } else {
                    read_sse_stream(
                        endpoint,
                        response,
                        stream_app_handle,
                        options.stream_event,
                        options.session_id,
                        progress_timeout,
                        total_timeout,
                    )
                    .await
                }
            }
        }
    } else {
        let json: Value = response
            .json()
            .await
            .map_err(|e| format!("响应解析失败: {}", e))?;
        ensure_non_empty_llm_content(
            extract_content(endpoint, &json).unwrap_or_default(),
            endpoint,
            "non_stream",
        )
    }
}

#[cfg(test)]
mod opencode_go_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn opencode_go_sends_session_headers_and_preserves_them_on_retry() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let length: usize = head
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(request).unwrap());
                let (status, body) = if attempt == 0 {
                    (
                        "429 Too Many Requests",
                        r#"{"error":{"message":"rate limit"}}"#,
                    )
                } else {
                    (
                        "200 OK",
                        r#"{"choices":[{"message":{"content":"整理后的文字。"}}]}"#,
                    )
                };
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        let endpoint = LlmEndpoint {
            provider: "opencode-go".into(),
            api_url: format!("http://{address}/chat/completions"),
            model: "glm-5.2".into(),
            timeout_secs: 5,
            api_format: ApiFormat::OpenaiCompat,
        };
        let output = send_llm_request(
            &reqwest::Client::builder().no_proxy().build().unwrap(),
            &endpoint,
            "test-key",
            &serde_json::json!({"model": "glm-5.2", "messages": [{"role": "user", "content": "原始文字"}]}),
            4, None,
            LlmRequestOptions { session_id: Some(42), ..Default::default() },
        ).await.unwrap();
        assert_eq!(output, "整理后的文字。");
        let requests = server.await.unwrap();
        let session_headers: Vec<_> = requests
            .iter()
            .map(|request| {
                let head = request.split("\r\n\r\n").next().unwrap();
                assert!(head.contains("authorization: Bearer test-key"));
                assert!(head.contains("user-agent: light-whisper/"));
                head.lines()
                    .find(|line| line.starts_with("x-opencode-session: "))
                    .unwrap()
            })
            .collect();
        assert_eq!(session_headers[0], session_headers[1]);
        assert!(session_headers[0].ends_with("-42"));
    }
}
