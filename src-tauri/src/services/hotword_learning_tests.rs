use super::hotword_learning as learning;
use crate::state::user_profile::{HotWord, HotWordSource, UserProfile};

fn profile() -> UserProfile {
    UserProfile {
        hot_words: ["ordinary", "priority"]
            .into_iter()
            .map(|text| HotWord {
                text: text.into(),
                weight: 3,
                source: HotWordSource::User,
                use_count: 0,
                last_used: 0,
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn matching_counts_longest_terms_once_and_respects_english_boundaries() {
    let mut p = profile();
    for text in ["公差", "形位公差", "PLC", "SPC 工作站"] {
        p.hot_words.push(HotWord {
            text: text.into(),
            weight: 3,
            source: HotWordSource::User,
            use_count: 0,
            last_used: 0,
        });
    }
    let found = learning::matches(&p.hot_words, "形位公差 形位公差 APLC plc SPC   工作站");
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        vec!["plc", "spc 工作站", "形位公差"]
    );
}

#[test]
fn revisions_replace_contribution_and_undo_decayed_points() {
    let mut p = profile();
    let raw = learning::matches(&p.hot_words, "ordinary ordinary");
    let shown = learning::matches(&p.hot_words, "ordinary");
    learning::record(&mut p, 1, raw.clone(), shown.clone(), 100);
    learning::record(&mut p, 1, raw, shown, 100);
    assert_eq!(p.hotword_learning.words["ordinary"].score, 1.0);
    let corrected = learning::matches(&p.hot_words, "priority");
    learning::correct(&mut p, 1, 1, corrected.clone(), 100);
    learning::correct(&mut p, 1, 1, corrected, 100);
    assert_eq!(p.hotword_learning.words["ordinary"].score, 0.0);
    assert_eq!(p.hotword_learning.words["priority"].score, 3.0);
    let reverted = learning::matches(&p.hot_words, "ordinary");
    learning::correct(&mut p, 1, 2, reverted, 100 + learning::HALF_LIFE);
    assert_eq!(p.hotword_learning.words["priority"].score, 0.0);
    assert_eq!(p.hotword_learning.words["priority"].corrections, 0);
    assert_eq!(p.hotword_learning.words["priority"].last_used, 0);
    assert_eq!(p.hotword_learning.words["ordinary"].uses, 1);
}

#[test]
fn resetting_one_word_does_not_invalidate_other_queued_words() {
    let mut p = profile();
    let epoch = p.hotword_learning.epoch;
    let reset_revision = p.hotword_learning.reset_revision;
    let found = learning::matches(&p.hot_words, "priority ordinary");
    learning::reset(&mut p, "priority", 100);
    assert_eq!(p.hotword_learning.epoch, epoch);
    learning::record_queued(&mut p, 1, found.clone(), found.clone(), 101, reset_revision);
    assert_eq!(p.hotword_learning.words["priority"].uses, 0);
    assert_eq!(p.hotword_learning.words["ordinary"].uses, 1);
    learning::correct(&mut p, 1, 1, found, 102);
    assert_eq!(p.hotword_learning.words["priority"].uses, 0);
}

#[test]
fn automatic_replacements_do_not_learn_from_themselves() {
    let mut p = profile();
    let shown = learning::matches(&p.hot_words, "priority");
    learning::record(&mut p, 1, Default::default(), shown.clone(), 100);
    learning::correct(&mut p, 1, 1, shown, 100);
    assert!(p.hotword_learning.words.is_empty());
}

#[test]
fn thresholds_decay_switch_reset_and_serialization() {
    assert_eq!(
        [4.9, 5.0, 14.9, 15.0, 39.9, 40.0].map(|s| learning::effective(2, s, true)),
        [2, 3, 3, 4, 4, 5]
    );
    let mut p = profile();
    for id in 1..=40 {
        let found = learning::matches(&p.hot_words, "priority");
        learning::record(&mut p, id, found.clone(), found, 100);
    }
    assert_eq!(learning::snapshot(&p, 100).words[0].effective_weight, 5);
    learning::reconcile(&mut p, 100 + learning::HALF_LIFE);
    assert_eq!(
        learning::snapshot(&p, 100 + learning::HALF_LIFE).words[0].effective_weight,
        4
    );
    assert_eq!(
        p.hotword_learning.words["priority"].events[0].reason,
        "decay"
    );
    learning::set_enabled(&mut p, false, 100 + learning::HALF_LIFE);
    assert_eq!(
        learning::snapshot(&p, 100 + learning::HALF_LIFE).words[0].effective_weight,
        3
    );
    learning::set_enabled(&mut p, true, 100 + learning::HALF_LIFE);
    let encoded = serde_json::to_string(&p).unwrap();
    assert!(!encoded.contains("contributions"));
    let mut reloaded: UserProfile = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        learning::snapshot(&reloaded, 100 + learning::HALF_LIFE).words[0].effective_weight,
        4
    );
    learning::reset(&mut reloaded, "priority", 100 + learning::HALF_LIFE);
    assert_eq!(reloaded.hotword_learning.words["priority"].score, 0.0);
    assert_eq!(
        reloaded.hotword_learning.words["priority"].events[0].reason,
        "reset"
    );
}

#[test]
fn rank_performance_for_large_vocabularies() {
    for count in [100, 1000, 10000] {
        let mut p = profile();
        p.hot_words = (0..count)
            .map(|i| HotWord {
                text: format!("设备型号{i}"),
                weight: 3,
                source: HotWordSource::User,
                use_count: 0,
                last_used: 0,
            })
            .collect();
        let start = std::time::Instant::now();
        for id in 1..=10 {
            let found = learning::matches(
                &p.hot_words,
                "今天使用设备型号99检查形位公差，明天再使用设备型号99。",
            );
            learning::record(&mut p, id, found.clone(), found, 100);
            assert_eq!(learning::snapshot(&p, 100).words.len(), count);
        }
        eprintln!(
            "hotword benchmark: {count} words, mean {:?}",
            start.elapsed() / 10
        );
    }
}

#[test]
fn learned_priority_changes_the_actual_asr_selection() {
    let mut value = serde_json::to_value(profile()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    value["hotword_learning"] = serde_json::json!({
        "enabled": true,
        "words": {"priority": {"score": 45.0, "updated_at": now, "uses": 45}}
    });
    let profile: UserProfile = serde_json::from_value(value).unwrap();
    assert_eq!(profile.get_hot_word_texts(1), vec!["priority"]);
}
