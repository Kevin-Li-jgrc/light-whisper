use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};

/// 最近一次听写结果只在内存中保留，不随用户配置导出。
#[derive(Default)]
pub struct ReinsertState {
    latest: parking_lot::Mutex<Option<(u64, String)>>,
    processing: Arc<AtomicUsize>,
    in_flight: Arc<AtomicBool>,
    pub output_lock: tokio::sync::Mutex<()>,
    pub notice_generation: AtomicU64,
}

pub struct ProcessingGuard(Arc<AtomicUsize>);
impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl ReinsertState {
    pub fn remember(&self, session_id: u64, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let mut latest = self.latest.lock();
        if latest.as_ref().is_none_or(|(id, _)| session_id > *id) {
            *latest = Some((session_id, text.to_owned()));
        }
    }
    pub fn try_begin(&self) -> Option<ReinsertGuard> {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| ReinsertGuard(self.in_flight.clone()))
    }
    pub fn text(&self) -> Option<String> {
        self.latest.lock().as_ref().map(|(_, text)| text.clone())
    }
    pub fn is_processing(&self) -> bool {
        self.processing.load(Ordering::Acquire) > 0
    }
    pub fn processing(&self) -> ProcessingGuard {
        self.processing.fetch_add(1, Ordering::AcqRel);
        ProcessingGuard(self.processing.clone())
    }
}

pub struct ReinsertGuard(Arc<AtomicBool>);
impl Drop for ReinsertGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_final_text_after_empty_or_older_results() {
        let state = ReinsertState::default();
        state.remember(3, "润色后的最终文字。");
        state.remember(4, "  ");
        state.remember(2, "迟到的旧结果");
        assert_eq!(state.text().as_deref(), Some("润色后的最终文字。"));
    }

    #[test]
    fn new_result_replaces_previous_but_restart_is_empty() {
        let state = ReinsertState::default();
        state.remember(1, "第一段");
        state.remember(2, "第二段\n保留换行");
        assert_eq!(state.text().as_deref(), Some("第二段\n保留换行"));
        assert_eq!(ReinsertState::default().text(), None);
    }

    #[test]
    fn overlapping_processing_and_deferred_output_remain_busy() {
        let state = ReinsertState::default();
        let first = state.processing();
        let second = state.processing();
        assert!(state.is_processing());
        drop(first);
        assert!(state.is_processing());
        drop(second);
        assert!(!state.is_processing());
    }

    #[test]
    fn concurrent_requests_are_rejected_until_the_first_finishes() {
        let state = ReinsertState::default();
        let first = state.try_begin().unwrap();
        assert!(state.try_begin().is_none());
        drop(first);
        assert!(state.try_begin().is_some());
    }
}
