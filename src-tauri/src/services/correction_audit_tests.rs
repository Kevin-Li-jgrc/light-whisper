use super::correction_audit::*;
use crate::state::user_profile::{CorrectionPattern, CorrectionSource, UserProfile};

#[tokio::test]
async fn import_is_blocked_while_model_response_is_pending_and_clears_old_reports_afterward() {
    let state = crate::state::AppState::default();
    let running = AuditGuard::acquire(&state.profile.correction_audit_running).unwrap();
    let mut p = UserProfile::default();
    p.correction_patterns.push(rule("测试"));
    let (send, receive) = tokio::sync::oneshot::channel::<String>();
    let mut receive = Some(receive);
    let ctx = context();
    let future = review(&mut p, &ctx, false, 100, |_| {
        let receive = receive.take().unwrap();
        async move { receive.await.map_err(|error| error.to_string()) }
    });
    tokio::pin!(future);
    tokio::select! { _ = &mut future => panic!("模型应仍在等待"), _ = tokio::task::yield_now() => {} }
    let json = serde_json::to_string(&UserProfile::default()).unwrap();
    let blocked: Result<(), String> =
        super::profile_service::import_profile(&state, &json, |_| async {
            panic!("审核期间不能提交导入")
        })
        .await;
    assert!(blocked.is_err());
    send.send(answer("valid")).unwrap();
    let report = future.await;
    drop(running);
    let mut imported = UserProfile::default();
    imported.correction_patterns.push(rule("新词"));
    imported.correction_audit.report = Some(report);
    let json = serde_json::to_string(&imported).unwrap();
    let result = super::profile_service::import_profile(&state, &json, |p| async { Ok(p) })
        .await
        .unwrap();
    assert!(result.correction_audit.report.is_none());
    assert!(result.correction_audit.cache.is_empty());
    assert_eq!(result.correction_patterns[0].original, "新词");
}

fn rule(original: &str) -> CorrectionPattern {
    CorrectionPattern {
        original: original.into(),
        corrected: format!("{original}正"),
        count: 3,
        last_seen: 100,
        source: CorrectionSource::Ai,
    }
}
fn context() -> AuditContext {
    context_for(&UserProfile::default().llm_provider)
}
fn answer(verdict: &str) -> String {
    format!(r#"{{"results":[{{"id":1,"verdict":"{verdict}","reason":"测试原因"}}]}}"#)
}

#[test]
fn rejects_malformed_fractional_duplicate_missing_and_unknown_results() {
    for raw in [
        "oops",
        "[]",
        "{}",
        r#"{"results":[{"id":1.5,"verdict":"invalid","reason":"x"}]}"#,
        r#"{"results":[{"id":0,"verdict":"invalid","reason":"x"}]}"#,
        r#"{"results":[{"id":2,"verdict":"invalid","reason":"x"}]}"#,
        r#"{"results":[{"id":1,"verdict":"bad","reason":"x"}]}"#,
        r#"{"results":[{"id":1,"verdict":"invalid","reason":""}]}"#,
        r#"{"results":[{"id":1,"verdict":"valid","reason":"x"},{"id":1,"verdict":"valid","reason":"x"}]}"#,
    ] {
        assert!(parse_results(raw, 1).is_err(), "{raw}");
    }
    assert!(parse_results(&answer("valid"), 2).is_err());
    for verdict in ["valid", "invalid", "uncertain"] {
        assert!(parse_results(&answer(verdict), 1).is_ok());
    }
}

#[tokio::test]
async fn review_only_suggests_and_reuses_cache_but_failure_is_not_success() {
    let mut p = UserProfile::default();
    p.correction_patterns.push(rule("测试"));
    let ctx = context();
    let report = review(&mut p, &ctx, false, 101, |_| async {
        Ok(answer("invalid"))
    })
    .await;
    assert_eq!(report.status, AuditStatus::Complete);
    assert_eq!(report.suggested, 1);
    assert_eq!(p.correction_patterns.len(), 1);
    p.correction_patterns[0].count += 1;
    let cached = review(&mut p, &ctx, false, 102, |_| async {
        panic!("缓存命中不应请求模型")
    })
    .await;
    assert_eq!(cached.reused, 1);
    let failed = review(&mut p, &ctx, true, 103, |_| async { Err("network".into()) }).await;
    assert_eq!(failed.status, AuditStatus::Failed);
    assert_eq!(failed.failed, 1);
    assert!(failed.rows[0].previous);
    assert_eq!(failed.suggested, 0);
    let retry = review(&mut p, &ctx, false, 104, |_| async { Ok(answer("valid")) }).await;
    assert_eq!(retry.checked, 1);
    assert_eq!(retry.suggested, 0);
    let saved = serde_json::to_string(&p).unwrap();
    let mut loaded: UserProfile = serde_json::from_str(&saved).unwrap();
    let report = review(&mut loaded, &ctx, false, 105, |_| async {
        panic!("重启保留缓存")
    })
    .await;
    assert_eq!(report.reused, 1);
}

#[tokio::test]
async fn partial_batches_retry_only_failed_and_context_change_invalidates() {
    let mut p = UserProfile {
        correction_patterns: (0..41).map(|n| rule(&format!("词{n}"))).collect(),
        ..Default::default()
    };
    let ctx = context();
    let mut call = 0;
    let report = review(&mut p, &ctx, false, 200, |_| {
        call += 1;
        let result = if call == 1 { Ok(serde_json::json!({"results": (1..=40).map(|id| serde_json::json!({"id":id,"verdict":"valid","reason":"合理"})).collect::<Vec<_>>()}).to_string()) } else { Err("timeout".into()) };
        async { result }
    }).await;
    assert_eq!(report.status, AuditStatus::Partial);
    assert_eq!(report.failed, 1);
    assert_eq!(report.checked, 40);
    let report = review(&mut p, &ctx, false, 201, |_| async {
        Ok(answer("uncertain"))
    })
    .await;
    assert_eq!(report.reused, 40);
    assert_eq!(report.uncertain, 1);
    let mut changed = ctx.clone();
    changed.model = "another-model".into();
    let report = review(&mut p, &changed, false, 202, |_| async {
        Err("offline".into())
    })
    .await;
    assert_eq!(report.failed, 41);
    assert_eq!(report.reused, 0);
}

#[tokio::test]
async fn deletion_requires_current_report_and_snapshot_then_blocks_and_restores() {
    let mut p = UserProfile::default();
    p.correction_patterns.push(rule("测试"));
    let ctx = context();
    let report = review(&mut p, &ctx, false, 101, |_| async {
        Ok(answer("invalid"))
    })
    .await;
    let pair = RuleKey::from(&p.correction_patterns[0]);
    assert!(delete_selected(&mut p, "stale", std::slice::from_ref(&pair), 102).is_err());
    p.correction_patterns[0].count += 1;
    let skipped = delete_selected(&mut p, &report.id, std::slice::from_ref(&pair), 102).unwrap();
    assert_eq!(skipped.changed, 0);
    assert_eq!(skipped.skipped.len(), 1);
    let report = review(&mut p, &ctx, false, 103, |_| async { panic!("缓存") }).await;
    let removed = delete_selected(&mut p, &report.id, &[pair.clone(), pair.clone()], 104).unwrap();
    assert_eq!(removed.changed, 1);
    assert!(p.correction_patterns.is_empty());
    crate::services::profile_service::learn_from_structured(
        &mut p,
        &[(pair.original.clone(), pair.corrected.clone())],
        &[],
        CorrectionSource::Ai,
    );
    assert!(p.correction_patterns.is_empty());
    restore_deleted(&mut p, &pair, 105).unwrap();
    assert_eq!(p.correction_patterns[0].source, CorrectionSource::User);
    assert!(p.correction_audit.deleted.is_empty());
}

#[test]
fn review_guard_releases_on_drop_and_excludes_concurrent_runs() {
    let lock = std::sync::atomic::AtomicBool::new(false);
    let guard = AuditGuard::acquire(&lock).unwrap();
    assert!(AuditGuard::acquire(&lock).is_err());
    drop(guard);
    assert!(AuditGuard::acquire(&lock).is_ok());
}

#[tokio::test]
async fn user_rules_and_uncertain_or_failed_findings_cannot_authorize_deletion() {
    for verdict in ["valid", "uncertain"] {
        let mut p = UserProfile::default();
        p.correction_patterns.push(rule("测试"));
        let report = review(&mut p, &context(), false, 100, |_| async {
            Ok(answer(verdict))
        })
        .await;
        let key = RuleKey::from(&p.correction_patterns[0]);
        assert_eq!(
            delete_selected(&mut p, &report.id, &[key], 101)
                .unwrap()
                .changed,
            0
        );
    }
    let mut p = UserProfile::default();
    p.correction_patterns.push(rule("测试"));
    let report = review(&mut p, &context(), false, 100, |_| async {
        Ok(answer("invalid"))
    })
    .await;
    let key = RuleKey::from(&p.correction_patterns[0]);
    p.correction_patterns[0].source = CorrectionSource::User;
    assert_eq!(
        delete_selected(&mut p, &report.id, &[key], 101)
            .unwrap()
            .changed,
        0
    );
    let empty = review(&mut p, &context(), false, 102, |_| async {
        panic!("用户规则不发送模型")
    })
    .await;
    assert_eq!(empty.status, AuditStatus::Empty);
}

#[tokio::test]
async fn deletion_block_only_matches_exact_pair_and_user_correction_clears_it() {
    let mut p = UserProfile::default();
    p.correction_patterns.push(rule("测试"));
    let report = review(&mut p, &context(), false, 100, |_| async {
        Ok(answer("invalid"))
    })
    .await;
    let key = RuleKey::from(&p.correction_patterns[0]);
    delete_selected(&mut p, &report.id, std::slice::from_ref(&key), 101).unwrap();
    assert!(allow_learning(
        &mut p,
        "测试",
        "不同",
        &CorrectionSource::Ai
    ));
    crate::services::profile_service::learn_from_correction(
        &mut p,
        "测试",
        "测试正",
        CorrectionSource::Ai,
    );
    assert!(p.correction_patterns.is_empty());
    crate::services::profile_service::learn_from_structured(
        &mut p,
        &[(key.original, key.corrected)],
        &[],
        CorrectionSource::User,
    );
    assert_eq!(p.correction_patterns.len(), 1);
    assert_eq!(p.correction_patterns[0].source, CorrectionSource::User);
    assert!(p.correction_audit.deleted.is_empty());
}

#[tokio::test]
async fn failed_force_review_retains_last_success_time_and_cannot_delete() {
    let mut p = UserProfile::default();
    p.correction_patterns.push(rule("测试"));
    review(&mut p, &context(), false, 100, |_| async {
        Ok(answer("invalid"))
    })
    .await;
    let failed = review(&mut p, &context(), true, 200, |_| async {
        Ok("broken".into())
    })
    .await;
    assert_eq!(p.correction_audit.last_attempt, 200);
    assert_eq!(p.correction_audit.last_success, 100);
    let key = RuleKey::from(&p.correction_patterns[0]);
    assert_eq!(
        delete_selected(&mut p, &failed.id, &[key], 201)
            .unwrap()
            .changed,
        0
    );
}

#[tokio::test]
async fn every_audit_context_dimension_invalidates_reuse() {
    let ctx = context();
    let mut variants = vec![ctx.clone(); 6];
    variants[0].provider += "other";
    variants[1].model += "other";
    variants[2].api_url += "/other";
    variants[3].api_format += "other";
    variants[4].reasoning += "other";
    variants[5].version += 1;
    for changed in variants {
        let mut p = UserProfile::default();
        p.correction_patterns.push(rule("测试"));
        review(&mut p, &ctx, false, 100, |_| async { Ok(answer("valid")) }).await;
        let report = review(&mut p, &changed, false, 101, |_| async {
            Ok(answer("uncertain"))
        })
        .await;
        assert_eq!(report.reused, 0);
        assert_eq!(report.uncertain, 1);
    }
}
