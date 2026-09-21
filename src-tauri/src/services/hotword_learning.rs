//! 本地热词学习：计分与会话纠错不依赖模型，不保存口述正文。
use crate::state::user_profile::{HotWord, HotWordSource, UserProfile};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const HALF_LIFE: u64 = 30 * 86400;

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn key(text: &str) -> String {
    if text.is_ascii() && !text.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return text.to_ascii_lowercase();
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Debug, Clone, Serialize)]
pub struct Learning {
    pub enabled: bool,
    pub words: BTreeMap<String, WordStats>,
    // 只允许修改本次运行仍在界面中可编辑的会话；不把会话正文写入画像。
    #[serde(skip)]
    sessions: VecDeque<Session>,
    #[serde(skip)]
    pub epoch: u64,
    #[serde(skip)]
    pub reset_revision: u64,
    #[serde(skip)]
    resets: BTreeMap<String, u64>,
}

impl Default for Learning {
    fn default() -> Self {
        Self {
            enabled: true,
            words: BTreeMap::new(),
            sessions: VecDeque::new(),
            epoch: 0,
            reset_revision: 0,
            resets: BTreeMap::new(),
        }
    }
}

impl<'de> Deserialize<'de> for Learning {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn enabled_default() -> bool {
            true
        }
        #[derive(Deserialize)]
        struct Saved {
            #[serde(default = "enabled_default")]
            enabled: bool,
            #[serde(default)]
            words: BTreeMap<String, WordStats>,
        }
        let mut saved = Saved::deserialize(deserializer)?;
        for stats in saved.words.values_mut() {
            stats.settled_last_used = stats.last_used;
        }
        Ok(Self {
            enabled: saved.enabled,
            words: saved.words,
            ..Self::default()
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WordStats {
    pub score: f64,
    pub updated_at: u64,
    pub uses: u32,
    pub corrections: u32,
    pub last_used: u64,
    #[serde(skip)]
    settled_last_used: u64,
    pub seen_weight: Option<u8>,
    pub events: VecDeque<WeightEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightEvent {
    pub at: u64,
    pub before: u8,
    pub after: u8,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
struct Contribution {
    points: u8,
    at: u64,
}

#[derive(Debug, Clone)]
struct Session {
    id: u64,
    revision: u64,
    raw: BTreeSet<String>,
    displayed: BTreeSet<String>,
    ignored: BTreeSet<String>,
    contributions: BTreeMap<String, Contribution>,
}

pub fn decayed(score: f64, since: u64, at: u64) -> f64 {
    if !score.is_finite() || score < 0.0 {
        return 0.0;
    }
    score * 2f64.powf(-(at.saturating_sub(since) as f64) / HALF_LIFE as f64)
}

pub fn effective(base: u8, score: f64, enabled: bool) -> u8 {
    let auto = if !enabled || score < 5.0 {
        1
    } else if score < 15.0 {
        3
    } else if score < 40.0 {
        4
    } else {
        5
    };
    base.clamp(1, 5).max(auto)
}

fn score(profile: &UserProfile, word: &HotWord, at: u64) -> f64 {
    profile
        .hotword_learning
        .words
        .get(&key(&word.text))
        .map(|s| decayed(s.score, s.updated_at, at))
        .unwrap_or(0.0)
}

/// 所有消费者共享一个排序，避免前端排名与实际注入名单不一致。
pub fn ranked(profile: &UserProfile, at: u64) -> Vec<(&HotWord, u8, u32)> {
    let mut rows: Vec<_> = profile
        .hot_words
        .iter()
        .map(|w| {
            let k = key(&w.text);
            let stats = profile.hotword_learning.words.get(&k);
            let uses = stats.map(|s| s.uses).unwrap_or(0);
            let score = stats
                .map(|s| decayed(s.score, s.updated_at, at))
                .unwrap_or(0.0);
            (
                w,
                effective(w.weight, score, profile.hotword_learning.enabled),
                uses,
                k,
            )
        })
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.3.cmp(&b.3)));
    rows.into_iter()
        .map(|(word, weight, uses, _)| (word, weight, uses))
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedWord {
    pub text: String,
    pub source: HotWordSource,
    pub rank: usize,
    pub base_weight: u8,
    pub effective_weight: u8,
    pub in_asr: bool,
    pub score: f64,
    pub uses: u32,
    pub corrections: u32,
    pub last_used: u64,
    pub next_threshold: Option<f64>,
    pub events: VecDeque<WeightEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub enabled: bool,
    pub words: Vec<RankedWord>,
}

pub fn snapshot(profile: &UserProfile, at: u64) -> Snapshot {
    Snapshot {
        enabled: profile.hotword_learning.enabled,
        words: ranked(profile, at)
            .into_iter()
            .enumerate()
            .map(|(i, (w, weight, uses))| {
                let stats = profile
                    .hotword_learning
                    .words
                    .get(&key(&w.text))
                    .cloned()
                    .unwrap_or_default();
                RankedWord {
                    text: w.text.clone(),
                    source: w.source.clone(),
                    rank: i + 1,
                    base_weight: w.weight,
                    effective_weight: weight,
                    in_asr: i < 100,
                    score: decayed(stats.score, stats.updated_at, at),
                    uses,
                    corrections: stats.corrections,
                    last_used: stats.last_used,
                    next_threshold: match weight {
                        1 | 2 => Some(5.0),
                        3 => Some(15.0),
                        4 => Some(40.0),
                        _ => None,
                    },
                    events: stats.events,
                }
            })
            .collect(),
    }
}

fn event(stats: &mut WordStats, before: u8, after: u8, reason: &str, at: u64) {
    if before != after || reason == "reset" {
        stats.events.push_front(WeightEvent {
            at,
            before,
            after,
            reason: reason.into(),
        });
        stats.events.truncate(20);
    }
    stats.seen_weight = Some(after);
}

pub fn reconcile(profile: &mut UserProfile, at: u64) {
    let enabled = profile.hotword_learning.enabled;
    let known: BTreeSet<_> = profile.hot_words.iter().map(|w| key(&w.text)).collect();
    profile
        .hotword_learning
        .words
        .retain(|k, _| known.contains(k));
    for word in &profile.hot_words {
        if let Some(s) = profile.hotword_learning.words.get_mut(&key(&word.text)) {
            let weight = effective(word.weight, decayed(s.score, s.updated_at, at), enabled);
            event(s, s.seen_weight.unwrap_or(weight), weight, "decay", at);
        }
    }
}

pub fn set_enabled(profile: &mut UserProfile, enabled: bool, at: u64) {
    if enabled == profile.hotword_learning.enabled {
        return;
    }
    reconcile(profile, at);
    profile.hotword_learning.enabled = enabled;
    profile.hotword_learning.epoch = profile.hotword_learning.epoch.wrapping_add(1);
    for stats in profile.hotword_learning.words.values_mut() {
        stats.settled_last_used = stats.last_used;
    }
    profile.hotword_learning.sessions.clear();
    profile.hotword_learning.resets.clear();
    for w in &profile.hot_words {
        if let Some(s) = profile.hotword_learning.words.get_mut(&key(&w.text)) {
            let weight = effective(w.weight, decayed(s.score, s.updated_at, at), enabled);
            event(
                s,
                s.seen_weight.unwrap_or(w.weight),
                weight,
                if enabled { "enabled" } else { "disabled" },
                at,
            );
        }
    }
}

pub fn reset(profile: &mut UserProfile, text: &str, at: u64) {
    let k = key(text);
    if let Some(w) = profile.hot_words.iter().find(|w| key(&w.text) == k) {
        let before = effective(
            w.weight,
            score(profile, w, at),
            profile.hotword_learning.enabled,
        );
        let mut s = WordStats::default();
        event(&mut s, before, w.weight, "reset", at);
        profile.hotword_learning.words.insert(k.clone(), s);
        profile.hotword_learning.reset_revision =
            profile.hotword_learning.reset_revision.wrapping_add(1);
        profile
            .hotword_learning
            .resets
            .insert(k.clone(), profile.hotword_learning.reset_revision);
        for session in &mut profile.hotword_learning.sessions {
            session.contributions.remove(&k);
            session.ignored.insert(k.clone());
        }
    }
}

/// 精确匹配既有热词；英文词边界、同位置最长词优先，不做语义关键词抽取。
pub fn matches(words: &[HotWord], text: &str) -> BTreeSet<String> {
    let text = key(text);
    let mut candidates = Vec::new();
    let ascii_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    for word in words {
        let k = key(&word.text);
        if k.is_empty() {
            continue;
        }
        for (start, _) in text.match_indices(&k) {
            let end = start + k.len();
            let left_ok = !k.chars().next().is_some_and(ascii_word)
                || !text[..start].chars().next_back().is_some_and(ascii_word);
            let right_ok = !k.chars().next_back().is_some_and(ascii_word)
                || !text[end..].chars().next().is_some_and(ascii_word);
            if left_ok && right_ok {
                candidates.push((start, end, k.clone()));
            }
        }
    }
    candidates.sort_by(|a, b| {
        (b.1 - b.0)
            .cmp(&(a.1 - a.0))
            .then(a.0.cmp(&b.0))
            .then(a.2.cmp(&b.2))
    });
    let mut occupied = Vec::<(usize, usize)>::new();
    let mut result = BTreeSet::new();
    for (start, end, k) in candidates {
        if !occupied.iter().any(|&(a, b)| start < b && a < end) {
            occupied.push((start, end));
            result.insert(k);
        }
    }
    result
}

fn change(profile: &mut UserProfile, k: &str, old: &Contribution, points: u8, at: u64, id: u64) {
    let Some(word) = profile.hot_words.iter().find(|w| key(&w.text) == k) else {
        return;
    };
    let other_last_used = profile
        .hotword_learning
        .sessions
        .iter()
        .filter(|s| s.id != id)
        .filter_map(|s| s.contributions.get(k))
        .filter(|c| c.points > 0)
        .map(|c| c.at)
        .max()
        .unwrap_or(0);
    let s = profile.hotword_learning.words.entry(k.into()).or_default();
    let before_score = decayed(s.score, s.updated_at, at);
    let before = effective(word.weight, before_score, true);
    s.score = (before_score - decayed(old.points as f64, old.at, at)).max(0.0) + points as f64;
    s.updated_at = at;
    s.uses = s
        .uses
        .saturating_sub(u32::from(old.points > 0))
        .saturating_add(u32::from(points > 0));
    s.corrections = s
        .corrections
        .saturating_sub(u32::from(old.points == 3))
        .saturating_add(u32::from(points == 3));
    s.last_used = s
        .settled_last_used
        .max(other_last_used)
        .max(if points > 0 { at } else { 0 });
    let after = effective(word.weight, s.score, true);
    event(
        s,
        before,
        after,
        if points == 3 {
            "correction"
        } else if points < old.points {
            "revision"
        } else {
            "usage"
        },
        at,
    );
}

#[cfg(test)]
pub fn record(
    profile: &mut UserProfile,
    id: u64,
    raw: BTreeSet<String>,
    displayed: BTreeSet<String>,
    at: u64,
) {
    record_queued(
        profile,
        id,
        raw,
        displayed,
        at,
        profile.hotword_learning.reset_revision,
    );
}

pub fn record_queued(
    profile: &mut UserProfile,
    id: u64,
    mut raw: BTreeSet<String>,
    displayed: BTreeSet<String>,
    at: u64,
    reset_revision: u64,
) {
    if id == 0
        || !profile.hotword_learning.enabled
        || profile.hotword_learning.sessions.iter().any(|s| s.id == id)
    {
        return;
    }
    reconcile(profile, at);
    let ignored: BTreeSet<_> = profile
        .hotword_learning
        .resets
        .iter()
        .filter(|(_, revision)| **revision > reset_revision)
        .map(|(k, _)| k.clone())
        .collect();
    raw.retain(|k| !ignored.contains(k));
    let mut session = Session {
        id,
        revision: 0,
        raw,
        displayed,
        ignored,
        contributions: BTreeMap::new(),
    };
    for k in &session.raw {
        change(profile, k, &Contribution::default(), 1, at, id);
        session
            .contributions
            .insert(k.clone(), Contribution { points: 1, at });
    }
    profile.hotword_learning.sessions.push_back(session);
    while profile.hotword_learning.sessions.len() > 128 {
        if let Some(old) = profile.hotword_learning.sessions.pop_front() {
            for (k, c) in old.contributions {
                if c.points > 0 {
                    if let Some(stats) = profile.hotword_learning.words.get_mut(&k) {
                        stats.settled_last_used = stats.settled_last_used.max(c.at);
                    }
                }
            }
        }
    }
}

/// 最新编辑版本覆盖本会话旧贡献；普通命中升为纠错命中时总计三分。
pub fn correct(
    profile: &mut UserProfile,
    id: u64,
    revision: u64,
    final_words: BTreeSet<String>,
    at: u64,
) {
    if !profile.hotword_learning.enabled {
        return;
    }
    let Some(index) = profile
        .hotword_learning
        .sessions
        .iter()
        .position(|s| s.id == id)
    else {
        return;
    };
    if revision <= profile.hotword_learning.sessions[index].revision {
        return;
    }
    reconcile(profile, at);
    let mut session = profile.hotword_learning.sessions[index].clone();
    let keys: BTreeSet<_> = session
        .contributions
        .keys()
        .cloned()
        .chain(final_words.iter().cloned())
        .collect();
    for k in keys {
        if session.ignored.contains(&k) {
            continue;
        }
        let points = if !final_words.contains(&k) {
            0
        } else if !session.displayed.contains(&k) {
            3
        } else if session.raw.contains(&k) {
            1
        } else {
            0
        };
        let old = session.contributions.get(&k).cloned().unwrap_or_default();
        if old.points == points {
            continue;
        }
        change(profile, &k, &old, points, at, id);
        session.contributions.insert(k, Contribution { points, at });
    }
    session.revision = revision;
    profile.hotword_learning.sessions[index] = session;
}

enum Job {
    Record { raw: String, displayed: String },
    Correction { revision: u64, text: String },
}

struct QueuedJob {
    app: tauri::AppHandle,
    id: u64,
    at: u64,
    epoch: u64,
    reset_revision: u64,
    job: Job,
}

/// 单一后台队列保持交付与后续编辑的顺序；扫描与磁盘保存不阻塞文字交付。
fn enqueue(app: &tauri::AppHandle, id: u64, job: Job) {
    use tauri::Manager;
    static QUEUE: std::sync::OnceLock<std::sync::mpsc::Sender<QueuedJob>> =
        std::sync::OnceLock::new();
    let state = app.state::<crate::state::AppState>();
    let Some((epoch, reset_revision)) = state.with_profile(|p| {
        p.hotword_learning
            .enabled
            .then_some((p.hotword_learning.epoch, p.hotword_learning.reset_revision))
    }) else {
        return;
    };
    let sender = QUEUE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<QueuedJob>();
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                let state = job.app.state::<crate::state::AppState>();
                let words = state.with_profile(|p| p.hot_words.clone());
                let (raw, displayed, revision) = match job.job {
                    Job::Record { raw, displayed } => {
                        (matches(&words, &raw), matches(&words, &displayed), None)
                    }
                    Job::Correction { revision, text } => {
                        (Default::default(), matches(&words, &text), Some(revision))
                    }
                };
                // 保存调度依赖 Tokio runtime；在 Tauri 的 runtime 中更新共享画像。
                tauri::async_runtime::block_on(async {
                    super::profile_service::update_profile_and_schedule(state.inner(), |p| {
                        if p.hotword_learning.epoch != job.epoch {
                            return;
                        }
                        if let Some(revision) = revision {
                            correct(p, job.id, revision, displayed, job.at);
                        } else {
                            record_queued(p, job.id, raw, displayed, job.at, job.reset_revision);
                        }
                    });
                });
            }
        });
        tx
    });
    if sender
        .send(QueuedJob {
            app: app.clone(),
            id,
            at: now(),
            epoch,
            reset_revision,
            job,
        })
        .is_err()
    {
        log::warn!("热词学习队列不可用，本次跳过统计");
    }
}

pub fn queue_record(app: &tauri::AppHandle, id: u64, raw: String, displayed: String) {
    enqueue(app, id, Job::Record { raw, displayed });
}

pub fn queue_correction(app: &tauri::AppHandle, id: u64, revision: u64, text: String) {
    enqueue(app, id, Job::Correction { revision, text });
}
