//! Trims the working sets of long-idle applications (milestone M5).
//!
//! `EmptyWorkingSet` removes a process's pages from physical memory right
//! away: unmodified pages go to the standby list, modified ones are written
//! to the page file or memory compression store. The process keeps running;
//! pages it touches again are read back, so its next actions may be slower.
//! Nothing persists, so there is nothing to restore. Commit charge does not
//! change.

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Threading::PROCESS_SET_QUOTA;

use crate::activity::Status;
use crate::classify::AppUsage;
use crate::process::{Instance, open_instance, working_set};

/// Minimum idle time before an app is offered for trimming.
pub const TRIM_AFTER: Duration = Duration::from_secs(15 * 60);

/// Whether `app` may be trimmed: a user-facing app idle for at least `TRIM_AFTER`.
/// Apps playing or recording audio are never idle, so they are excluded too.
pub fn eligible(app: &AppUsage) -> bool {
    app.category.idle_candidate() && matches!(app.status, Status::Idle(quiet) if quiet >= TRIM_AFTER)
}

#[derive(Default, Debug)]
pub struct Report {
    pub apps: usize,
    /// Working-set bytes moved out of physical memory.
    pub freed: u64,
    /// Processes that could not be trimmed (protected, elevated or exited).
    pub failed: usize,
}

/// Trims every process of `apps` on a background thread, so large working
/// sets do not freeze the UI. The report arrives on the returned channel.
pub fn spawn(apps: Vec<AppUsage>) -> Receiver<Report> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("trim".into())
        .spawn(move || {
            let mut report = Report::default();
            for app in &apps {
                let (freed, failed) = trim_instances(&app.instances);
                report.apps += 1;
                report.freed += freed;
                report.failed += failed;
            }
            let _ = tx.send(report);
        })
        .expect("failed to start trim thread");
    rx
}

/// Returns `(bytes moved out of RAM, processes that failed)`.
fn trim_instances(instances: &[Instance]) -> (u64, usize) {
    let mut freed = 0;
    let mut failed = 0;
    for &inst in instances {
        let Some(process) = open_instance(inst, PROCESS_SET_QUOTA) else {
            failed += 1;
            continue;
        };
        let before = working_set(&process).unwrap_or(0);
        if unsafe { EmptyWorkingSet(process.raw()) }.is_err() {
            failed += 1;
            continue;
        }
        let after = working_set(&process).unwrap_or(before);
        freed += before.saturating_sub(after);
    }
    (freed, failed)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use windows::Win32::System::Threading::{GetProcessTimes, PROCESS_QUERY_LIMITED_INFORMATION};

    use super::*;
    use crate::classify::Category;
    use crate::process::OwnedProcess;

    fn app(status: Status, category: Category) -> AppUsage {
        AppUsage {
            name: "app.exe".into(),
            category,
            count: 0,
            private: 0,
            working_set: 0,
            instances: Vec::new(),
            cpu_pct: 0.0,
            status,
            lowered: false,
            audio: false,
            growth: None,
        }
    }

    #[test]
    fn only_long_idle_candidates_are_eligible() {
        assert!(eligible(&app(Status::Idle(TRIM_AFTER), Category::WebApp)));
        assert!(!eligible(&app(Status::Idle(TRIM_AFTER - Duration::from_secs(1)), Category::WebApp)));
        assert!(!eligible(&app(Status::Inactive, Category::Application)));
        assert!(!eligible(&app(Status::Idle(TRIM_AFTER), Category::System)));
    }

    #[test]
    fn trims_a_real_process() {
        let mut child = Command::new("cmd").args(["/c", "ping -n 30 127.0.0.1 >nul"]).spawn().unwrap();
        let pid = child.id();
        let handle = unsafe {
            windows::Win32::System::Threading::OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
        }
        .unwrap();
        let probe = OwnedProcess::from_raw(handle);
        let (mut c, mut e, mut k, mut u) = Default::default();
        unsafe { GetProcessTimes(probe.raw(), &mut c, &mut e, &mut k, &mut u) }.unwrap();
        let created = (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64;
        let before = working_set(&probe).unwrap();

        let (freed, failed) = trim_instances(&[(pid, created)]);
        assert_eq!(failed, 0);
        assert!(freed > 0, "a running process has pages to trim");
        assert!(working_set(&probe).unwrap() < before);

        // A different start time means another process: left alone.
        assert_eq!(trim_instances(&[(pid, created + 1)]), (0, 1));
        let _ = child.kill();
    }
}
