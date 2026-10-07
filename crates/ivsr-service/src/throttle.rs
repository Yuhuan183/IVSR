//! Rate-limits progress so observers (terminal, webview) stay cheap.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ivsr_core::{LogLevel, Progress, Reporter};

pub struct Throttled<R> {
    inner: R,
    interval: Duration,
    last: Mutex<Option<(Instant, Progress)>>,
}

impl<R: Reporter> Throttled<R> {
    pub fn new(inner: R, interval: Duration) -> Self {
        Self { inner, interval, last: Mutex::new(None) }
    }
}

impl<R: Reporter> Reporter for Throttled<R> {
    fn progress(&self, progress: Progress) {
        let mut last = self.last.lock().unwrap();
        let now = Instant::now();
        let emit = match *last {
            None => true,
            Some((at, prev)) => {
                prev.stage != progress.stage || progress.overall >= 1.0 || now.duration_since(at) >= self.interval
            }
        };
        if emit {
            *last = Some((now, progress));
            drop(last);
            self.inner.progress(progress);
        }
    }

    fn log(&self, level: LogLevel, message: &str) {
        self.inner.log(level, message);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ivsr_core::Stage;

    use super::*;

    #[derive(Clone, Default)]
    struct Count(Arc<Mutex<Vec<Progress>>>);
    impl Reporter for Count {
        fn progress(&self, p: Progress) {
            self.0.lock().unwrap().push(p);
        }
        fn log(&self, _: LogLevel, _: &str) {}
    }

    #[test]
    fn bursts_collapse_but_stage_changes_and_completion_pass() {
        let sink = Count::default();
        let throttled = Throttled::new(sink.clone(), Duration::from_secs(60));
        let at = |stage, overall| Progress { stage, overall, units: None };
        for i in 0..100 {
            throttled.progress(at(Stage::Upscaling, i as f64 / 200.0));
        }
        throttled.progress(at(Stage::Encoding, 0.9));
        throttled.progress(at(Stage::Encoding, 0.95));
        throttled.progress(at(Stage::Finalizing, 1.0));
        let stages: Vec<_> = sink.0.lock().unwrap().iter().map(|p| (p.stage, p.overall)).collect();
        assert_eq!(stages, vec![(Stage::Upscaling, 0.0), (Stage::Encoding, 0.9), (Stage::Finalizing, 1.0)]);
    }
}
