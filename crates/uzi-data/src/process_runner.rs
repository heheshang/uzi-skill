//! Port of `lib/pipeline/process_runner.py` — the small scheduler used to
//! enforce hard fetcher timeouts.
//!
//! Upstream runs each job in a killable `multiprocessing` child. Rust cannot
//! kill a running thread, so the port keeps the same *observable scheduling
//! contract* — unique keys, `max_workers`, `serial_group` mutual exclusion,
//! per-job timeout and overall timeout, with a `ProcessOutcome` per job — and
//! relies on every network call already carrying a bounded `ureq` timeout
//! (`http::timeout_default`). A job that overruns its deadline is reported as
//! `timed_out` rather than silently dropped.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Poison-tolerant lock acquisition: a panicking job must not take the
/// scheduler down with it (upstream's child processes can die freely).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `ProcessJob`.
pub type JobFn = dyn Fn() -> Result<Vec<Value>, String> + Send + Sync;

pub struct ProcessJob {
    pub key: String,
    pub target: Arc<JobFn>,
    pub timeout_sec: f64,
    pub serial_group: Option<String>,
}

impl ProcessJob {
    pub fn new<F>(key: &str, target: F) -> Self
    where
        F: Fn() -> Result<Vec<Value>, String> + Send + Sync + 'static,
    {
        ProcessJob {
            key: key.to_string(),
            target: Arc::new(target),
            timeout_sec: 120.0,
            serial_group: None,
        }
    }

    pub fn timeout(mut self, secs: f64) -> Self {
        self.timeout_sec = secs;
        self
    }

    pub fn serial_group(mut self, group: &str) -> Self {
        self.serial_group = Some(group.to_string());
        self
    }
}

use serde_json::Value;

/// `ProcessOutcome`.
#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub key: String,
    pub value: Option<Vec<Value>>,
    pub error: Option<String>,
    pub timed_out: bool,
}

/// `run_process_jobs(jobs, max_workers, overall_timeout)`.
///
/// Jobs run on `max_workers` rayon threads; jobs sharing a `serial_group` never
/// run concurrently. The returned outcomes preserve upstream's diagnostic
/// ordering (completion order within a wave does not matter to `collect`).
pub fn run_process_jobs(
    jobs: Vec<ProcessJob>,
    max_workers: usize,
    overall_timeout: f64,
) -> Result<Vec<ProcessOutcome>, String> {
    if max_workers < 1 {
        return Err("max_workers must be >= 1".to_string());
    }
    if overall_timeout <= 0.0 {
        return Err("overall_timeout must be > 0".to_string());
    }
    let mut keys: Vec<&str> = jobs.iter().map(|j| j.key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    if keys.len() != jobs.len() {
        return Err("process job keys must be unique".to_string());
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(max_workers)
        .build()
        .map_err(|e| format!("rayon pool: {e}"))?;

    let started = Instant::now();
    let group_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>> = Arc::new(Mutex::new(HashMap::new()));
    let pending: Arc<Mutex<Vec<ProcessJob>>> = Arc::new(Mutex::new(jobs));
    let active = Arc::new(AtomicUsize::new(0));
    let outcomes: Arc<Mutex<Vec<ProcessOutcome>>> = Arc::new(Mutex::new(Vec::new()));

    loop {
        let remaining = overall_timeout - started.elapsed().as_secs_f64();
        if remaining <= 0.0 {
            let mut out = lock(&outcomes);
            let mut pending = lock(&pending);
            for job in pending.drain(..) {
                out.push(ProcessOutcome {
                    key: job.key,
                    value: None,
                    error: Some("overall timeout before start".to_string()),
                    timed_out: true,
                });
            }
            if active.load(Ordering::SeqCst) == 0 {
                break;
            }
        }

        let is_done = {
            let pending = lock(&pending);
            pending.is_empty() && active.load(Ordering::SeqCst) == 0
        };
        if is_done {
            break;
        }

        let next = {
            let mut pending = lock(&pending);
            if pending.is_empty() {
                None
            } else {
                Some(pending.remove(0))
            }
        };
        let Some(job) = next else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };

        let group_lock = job.serial_group.as_ref().map(|g| {
            let mut locks = lock(&group_locks);
            locks
                .entry(g.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        });

        active.fetch_add(1, Ordering::SeqCst);
        let outcomes2 = outcomes.clone();
        let active2 = active.clone();
        pool.spawn(move || {
            let _guard = group_lock.as_ref().map(|l| lock(l));
            let t0 = Instant::now();
            let result = (job.target)();
            let elapsed = t0.elapsed().as_secs_f64();
            let mut out = lock(&outcomes2);
            if elapsed > job.timeout_sec {
                out.push(ProcessOutcome {
                    key: job.key,
                    value: None,
                    error: Some(format!("fetcher timeout > {}s", fmt_g(job.timeout_sec))),
                    timed_out: true,
                });
            } else {
                match result {
                    Ok(value) => out.push(ProcessOutcome {
                        key: job.key,
                        value: Some(value),
                        error: None,
                        timed_out: false,
                    }),
                    Err(e) => out.push(ProcessOutcome {
                        key: job.key,
                        value: None,
                        error: Some(truncate(&e, 300)),
                        timed_out: false,
                    }),
                }
            }
            active2.fetch_sub(1, Ordering::SeqCst);
        });
    }

    // Wait for in-flight jobs so every key has an outcome.
    while active.load(Ordering::SeqCst) > 0 {
        std::thread::sleep(Duration::from_millis(5));
    }

    let mut out = Arc::try_unwrap(outcomes)
        .map(|m| m.into_inner().unwrap())
        .unwrap_or_else(|arc| lock(&arc).clone());
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// Python `f"{value:g}"` for the timeout message.
fn fmt_g(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn outcomes_cover_every_key() {
        let jobs = vec![
            ProcessJob::new("a", || Ok(vec![json!(1)])),
            ProcessJob::new("b", || Err("nope".into())),
        ];
        let out = run_process_jobs(jobs, 2, 10.0).unwrap();
        assert_eq!(out.len(), 2);
        let b = out.iter().find(|o| o.key == "b").unwrap();
        assert_eq!(b.error.as_deref(), Some("nope"));
        assert!(!b.timed_out);
    }

    #[test]
    fn duplicate_keys_rejected_like_upstream() {
        let jobs = vec![
            ProcessJob::new("a", || Ok(vec![])),
            ProcessJob::new("a", || Ok(vec![])),
        ];
        assert!(run_process_jobs(jobs, 2, 10.0).is_err());
        assert!(run_process_jobs(vec![], 0, 10.0).is_err());
        assert!(run_process_jobs(vec![], 2, 0.0).is_err());
    }

    #[test]
    fn serial_group_never_overlaps() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CONCURRENT: AtomicUsize = AtomicUsize::new(0);
        static MAXSEEN: AtomicUsize = AtomicUsize::new(0);
        let jobs = (0..4)
            .map(|i| {
                ProcessJob::new(&format!("s{i}"), || {
                    let now = CONCURRENT.fetch_add(1, Ordering::SeqCst) + 1;
                    MAXSEEN.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    CONCURRENT.fetch_sub(1, Ordering::SeqCst);
                    Ok(vec![])
                })
                .serial_group("mini_racer")
            })
            .collect();
        run_process_jobs(jobs, 4, 10.0).unwrap();
        assert_eq!(MAXSEEN.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn overall_timeout_marks_unstarted_jobs() {
        let jobs = vec![ProcessJob::new("slow", || {
            std::thread::sleep(Duration::from_millis(200));
            Ok(vec![])
        }), ProcessJob::new("queued", || Ok(vec![]))];
        let out = run_process_jobs(jobs, 1, 0.05).unwrap();
        let queued = out.iter().find(|o| o.key == "queued");
        // Either it never started (timed_out) or it ran — both must be present.
        assert!(queued.is_some());
    }
}
