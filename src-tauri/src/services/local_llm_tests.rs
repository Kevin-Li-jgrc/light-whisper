use super::local_llm::*;
use serde_json::json;

#[test]
fn local_response_rejects_truncation_and_missing_completion() {
    assert!(validate_completion("你好", Some("length"), false).is_err());
    assert!(validate_completion("半截", None, false).is_err());
    assert!(validate_completion("  ", Some("stop"), false).is_err());
    assert_eq!(
        validate_completion("完成", Some("stop"), false).unwrap(),
        "完成"
    );
}

#[test]
fn local_structured_response_rejects_prose_and_broken_json() {
    assert!(validate_completion("{\"polished\":", Some("stop"), true).is_err());
    assert!(validate_completion("这是结果", Some("stop"), true).is_err());
    assert!(validate_completion(
        "{\"polished\":\"你好\",\"corrections\":[],\"key_terms\":[]}",
        Some("stop"),
        true
    )
    .is_ok());
}

#[test]
fn local_request_rejects_images_and_external_tools() {
    assert!(validate_request(&json!({"messages":[{"content":[{"type":"image_url","image_url":{"url":"https://invalid/image"}}]}]})).is_err());
    assert!(validate_request(&json!({"tools":[{"type":"web_search"}]})).is_err());
    assert!(
        validate_request(&json!({"messages":[{"role":"user","content":"中英文 hello"}]})).is_ok()
    );
}

#[test]
fn local_catalog_rejects_paths_and_unknown_models() {
    assert!(model_spec("../../file").is_err());
    assert!(model_spec("cloud-model").is_err());
    assert!(model_spec("qwen3.5-0.8b").is_ok());
    assert!(model_spec("lfm2.5-1.2b").is_ok());
}

#[test]
fn local_polish_schema_requires_nonempty_body_and_learning_arrays() {
    assert!(validate_polish_schema(r#"{"polished":"" ,"corrections":[],"key_terms":[]}"#).is_err());
    assert!(validate_polish_schema(r#"{"polished":"hello"}"#).is_err());
    assert!(
        validate_polish_schema(r#"{"polished":"你好 hello","corrections":[],"key_terms":[]}"#)
            .is_ok()
    );
}

#[test]
fn local_roles_share_selected_model_without_cloud_endpoint() {
    use crate::{services::llm_provider, state::user_profile::LlmProviderConfig};
    let mut config = LlmProviderConfig {
        active: "local".into(),
        assistant_use_separate_model: true,
        assistant_provider: Some("local".into()),
        selection_use_separate_model: true,
        selection_provider: Some("local".into()),
        validation_use_separate_model: true,
        validation_provider: Some("local".into()),
        ..Default::default()
    };
    config.local.model = "lfm2.5-1.2b".into();
    for endpoint in [
        llm_provider::endpoint_for_config(&config),
        llm_provider::assistant_endpoint_for_config(&config),
        llm_provider::selection_endpoint_for_config(&config),
        llm_provider::validation_endpoint_for_config(&config),
    ] {
        assert_eq!(endpoint.provider, "local");
        assert_eq!(endpoint.api_url, "local://managed");
        assert_eq!(endpoint.model, "lfm2.5-1.2b");
    }
}

#[test]
fn local_selection_does_not_follow_cloud_polish_without_a_separate_model_name() {
    let config = crate::state::user_profile::LlmProviderConfig {
        active: "openai".into(),
        selection_use_separate_model: true,
        selection_provider: Some("local".into()),
        selection_model: None,
        ..Default::default()
    };
    assert_eq!(config.resolve_selection_provider(), "local");
    assert_eq!(
        crate::services::llm_provider::selection_endpoint_for_config(&config).provider,
        "local"
    );
}

#[test]
fn old_configuration_does_not_enable_or_download_local_models() {
    let config: crate::state::user_profile::LlmProviderConfig =
        serde_json::from_str(r#"{"active":"cerebras"}"#).unwrap();
    assert_eq!(config.resolve_active_provider(), "cerebras");
    assert!(config.local_cloud_backup.is_none());
    assert!(validate_config(&config.local).is_ok());
}
