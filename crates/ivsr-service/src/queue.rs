//! A FIFO job queue with one worker: upscaling saturates the GPU, so jobs
//! run one at a time while frontends observe a stream of events.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use ivsr_core::{CancelToken, JobOutcome, JobSpec, LogLevel, Progress, Reporter, Stage};
use serde::Serialize;

use crate::Service;
use crate::throttle::Throttled;

pub type JobId = u64;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    Started { id: JobId },
    Progress {
        id: JobId,
        stage: Stage,
        overall: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        units: Option<(u64, u64)>,
    },
    Log { id: JobId, level: LogLevel, message: String },
    Completed { id: JobId, outcome: JobOutcome, elapsed_ms: u64 },
    Failed { id: JobId, error: String },
    Cancelled { id: JobId },
}

pub type EventSink = Arc<dyn Fn(JobEvent) + Send + Sync>;

struct Queued {
    id: JobId,
    service: Arc<Service>,
    engine: String,
    spec: JobSpec,
    cancel: CancelToken,
}

pub struct JobQueue {
    tx: mpsc::Sender<Queued>,
    active: Arc<Mutex<HashMap<JobId, CancelToken>>>,
    next: AtomicU64,
}

struct EventReporter {
    id: JobId,
    sink: EventSink,
}

impl Reporter for EventReporter {
    fn progress(&self, p: Progress) {
        (self.sink)(JobEvent::Progress { id: self.id, stage: p.stage, overall: p.overall, units: p.units });
    }

    fn log(&self, level: LogLevel, message: &str) {
        if level != LogLevel::Debug {
            (self.sink)(JobEvent::Log { id: self.id, level, message: message.into() });
        }
    }
}

impl JobQueue {
    pub fn new(sink: EventSink) -> Self {
        let (tx, rx) = mpsc::channel::<Queued>();
        let active: Arc<Mutex<HashMap<JobId, CancelToken>>> = Arc::default();
        let worker_active = active.clone();
        std::thread::Builder::new()
            .name("ivsr-jobs".into())
            .spawn(move || {
                for job in rx {
                    let id = job.id;
                    let event = if job.cancel.is_cancelled() {
                        JobEvent::Cancelled { id }
                    } else {
                        sink(JobEvent::Started { id });
                        let reporter =
                            Throttled::new(EventReporter { id, sink: sink.clone() }, Duration::from_millis(100));
                        let started = Instant::now();
                        match job.service.run(&job.spec, &job.engine, &reporter, &job.cancel) {
                            Ok(outcome) => {
                                JobEvent::Completed { id, outcome, elapsed_ms: started.elapsed().as_millis() as u64 }
                            }
                            Err(e) if e.is_cancelled() => JobEvent::Cancelled { id },
                            Err(e) => JobEvent::Failed { id, error: e.to_string() },
                        }
                    };
                    worker_active.lock().unwrap().remove(&id);
                    sink(event);
                }
            })
            .expect("spawn job worker");
        Self { tx, active, next: AtomicU64::new(1) }
    }

    /// Enqueues a job run against `service` (a snapshot of the configuration at submit time).
    pub fn submit(&self, service: Arc<Service>, engine: String, spec: JobSpec) -> JobId {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let cancel = CancelToken::new();
        self.active.lock().unwrap().insert(id, cancel.clone());
        let _ = self.tx.send(Queued { id, service, engine, spec, cancel });
        id
    }

    /// Cancels a queued or running job. Returns false if it already finished.
    pub fn cancel(&self, id: JobId) -> bool {
        match self.active.lock().unwrap().get(&id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    pub fn cancel_all(&self) {
        for token in self.active.lock().unwrap().values() {
            token.cancel();
        }
    }

    /// Jobs queued or running.
    pub fn pending(&self) -> usize {
        self.active.lock().unwrap().len()
    }
}
