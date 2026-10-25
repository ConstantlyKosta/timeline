//! Background jobs (note import, transcription) with progress and cancellation.
//! A job runs on its own thread; the UI polls `job_status` (works the same over Tauri IPC and the
//! dev HTTP bridge, no event plumbing needed).

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub kind: String,
    /// `running` | `done` | `error` | `cancelled`
    pub state: String,
    /// 0.0 – 1.0
    pub progress: f64,
    pub message: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    /// What the job is about (e.g. the media id being transcribed).
    pub subject: Option<String>,
}

struct JobState {
    info: Mutex<JobInfo>,
    cancel: AtomicBool,
}

/// Handle passed to the job body.
#[derive(Clone)]
pub struct JobCtx {
    state: Arc<JobState>,
}

impl JobCtx {
    pub fn progress(&self, p: f64, message: impl Into<String>) {
        let mut i = self.state.info.lock().unwrap_or_else(|e| e.into_inner());
        i.progress = p.clamp(0.0, 1.0);
        i.message = message.into();
    }
    pub fn cancelled(&self) -> bool {
        self.state.cancel.load(Ordering::Relaxed)
    }
    /// A context that is never cancelled and reports nowhere (tests, synchronous callers).
    pub fn detached() -> JobCtx {
        JobCtx {
            state: Arc::new(JobState {
                info: Mutex::new(JobInfo { id: String::new(), kind: String::new(), state: "running".into(), progress: 0.0, message: String::new(), result: None, error: None, subject: None }),
                cancel: AtomicBool::new(false),
            }),
        }
    }
    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.state.cancel
    }
}

#[derive(Default)]
pub struct Jobs {
    map: Mutex<HashMap<String, Arc<JobState>>>,
    order: Mutex<Vec<String>>,
}

impl Jobs {
    /// Starts `body` on a new thread and returns its job id.
    pub fn spawn<F>(&self, kind: &str, subject: Option<String>, body: F) -> String
    where
        F: FnOnce(JobCtx) -> crate::Result<Value> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().to_string();
        let state = Arc::new(JobState {
            info: Mutex::new(JobInfo { id: id.clone(), kind: kind.into(), state: "running".into(), progress: 0.0, message: "Starting…".into(), result: None, error: None, subject }),
            cancel: AtomicBool::new(false),
        });
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), state.clone());
        self.order.lock().unwrap_or_else(|e| e.into_inner()).push(id.clone());
        let ctx = JobCtx { state: state.clone() };
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(ctx.clone())));
            let mut i = state.info.lock().unwrap_or_else(|e| e.into_inner());
            match outcome {
                Ok(Ok(v)) => {
                    i.state = if state.cancel.load(Ordering::Relaxed) { "cancelled" } else { "done" }.into();
                    i.progress = if i.state == "done" { 1.0 } else { i.progress };
                    i.result = Some(v);
                }
                Ok(Err(e)) => {
                    i.state = if state.cancel.load(Ordering::Relaxed) { "cancelled" } else { "error" }.into();
                    i.error = Some(e.to_string());
                }
                Err(_) => {
                    i.state = "error".into();
                    i.error = Some("the background task crashed".into());
                }
            }
        });
        id
    }

    pub fn status(&self, id: &str) -> Option<JobInfo> {
        let s = self.map.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()?;
        let i = s.info.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Some(i)
    }

    pub fn cancel(&self, id: &str) -> bool {
        match self.map.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            Some(s) => {
                s.cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// All jobs, newest last; finished ones included (the UI picks what it needs).
    pub fn list(&self) -> Vec<JobInfo> {
        let order = self.order.lock().unwrap_or_else(|e| e.into_inner()).clone();
        order.iter().filter_map(|id| self.status(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(j: &Jobs, id: &str) -> JobInfo {
        for _ in 0..200 {
            let s = j.status(id).unwrap();
            if s.state != "running" {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("job did not finish");
    }

    #[test]
    fn runs_reports_result_and_errors() {
        let j = Jobs::default();
        let id = j.spawn("t", None, |c| {
            c.progress(0.5, "half");
            Ok(serde_json::json!({"ok": 1}))
        });
        let s = wait(&j, &id);
        assert_eq!((s.state.as_str(), s.progress), ("done", 1.0));
        assert_eq!(s.result.unwrap()["ok"], 1);
        let id = j.spawn("t", None, |_| Err(crate::Error::InvalidArgument("boom".into())));
        let s = wait(&j, &id);
        assert_eq!(s.state, "error");
        assert!(s.error.unwrap().contains("boom"));
        let id = j.spawn("t", None, |_| -> crate::Result<Value> { panic!("x") });
        assert_eq!(wait(&j, &id).state, "error");
        assert_eq!(j.list().len(), 3);
    }

    #[test]
    fn cancel_is_observed() {
        let j = Jobs::default();
        let id = j.spawn("t", Some("s".into()), |c| {
            while !c.cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(Value::Null)
        });
        assert!(j.cancel(&id));
        assert_eq!(wait(&j, &id).state, "cancelled");
        assert!(!j.cancel("nope"));
    }
}
