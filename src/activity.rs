//! Tracks per-application activity across samples to detect idle applications.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::classify::AppUsage;
use crate::process::ProcessInfo;

/// Average CPU use over `WINDOW`, as a share of one core, that counts as activity.
/// Unused background apps measured up to ~4% (Electron apps, Everything), so the
/// threshold sits above that; apps in use are caught by the foreground check.
const ACTIVE_CPU_PCT_OF_CORE: f64 = 5.0;
/// Average I/O throughput over `WINDOW` that counts as activity.
const ACTIVE_IO_BYTES_PER_SEC: f64 = 512.0 * 1024.0;
/// Averaging window, so periodic background wake-ups (timers, sync) of an
/// otherwise unused app do not count as use.
const WINDOW: Duration = Duration::from_secs(30);
const INACTIVE_AFTER: Duration = Duration::from_secs(60);
pub const IDLE_AFTER: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    /// Activity within the last minute.
    Active,
    /// No activity for 1 to 5 minutes.
    Inactive,
    /// No activity for at least 5 minutes; holds how long.
    Idle(Duration),
}

impl Status {
    fn from_quiet_time(quiet: Duration) -> Self {
        if quiet >= IDLE_AFTER {
            Status::Idle(quiet)
        } else if quiet >= INACTIVE_AFTER {
            Status::Inactive
        } else {
            Status::Active
        }
    }
}

/// Activity history of one application.
struct AppState {
    first_seen: Instant,
    last_active: Instant,
    /// `(sample time, CPU delta, I/O delta)` within `WINDOW`.
    recent: VecDeque<(Instant, u64, u64)>,
}

impl AppState {
    fn new(now: Instant) -> Self {
        Self { first_seen: now, last_active: now, recent: VecDeque::new() }
    }

    /// Records a sample and returns the window averages
    /// `(CPU % of one core, I/O bytes per second)`.
    fn push(&mut self, now: Instant, cpu: u64, io: u64) -> (f64, f64) {
        self.recent.push_back((now, cpu, io));
        while self.recent.front().is_some_and(|&(t, _, _)| now - t > WINDOW) {
            self.recent.pop_front();
        }
        let span = WINDOW.min(now - self.first_seen).as_secs_f64();
        if span <= 0.0 {
            return (0.0, 0.0);
        }
        let (cpu, io) = self.recent.iter().fold((0u64, 0u64), |acc, &(_, c, i)| (acc.0 + c, acc.1 + i));
        // CPU time is in 100 ns units.
        (cpu as f64 / 1e7 / span * 100.0, io as f64 / span)
    }
}

pub struct Tracker {
    /// Last CPU time and I/O bytes per process instance `(pid, start time)`.
    procs: HashMap<(u32, u64), (u64, u64)>,
    /// Per application, keyed by lower-case name.
    apps: HashMap<String, AppState>,
    last_sample: Option<Instant>,
    cpus: f64,
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            procs: HashMap::new(),
            apps: HashMap::new(),
            last_sample: None,
            cpus: std::thread::available_parallelism().map_or(1, |n| n.get()) as f64,
        }
    }

    /// Fills `cpu_pct` and `status` of each application.
    /// Applications seen for the first time count as active from `now`, so
    /// nothing is reported idle before it has been observed for `IDLE_AFTER`.
    pub fn update(&mut self, procs: &[ProcessInfo], apps: &mut [AppUsage], foreground: Option<u32>, now: Instant) {
        let secs = self.last_sample.map_or(0.0, |t| (now - t).as_secs_f64());
        self.last_sample = Some(now);

        // Per-process deltas. New processes have no baseline yet and contribute
        // nothing, so short-lived helpers do not look like bursts of activity.
        let mut deltas: HashMap<u32, (u64, u64)> = HashMap::new();
        let mut current = HashMap::with_capacity(procs.len());
        for p in procs {
            let Some(s) = &p.stats else { continue };
            let key = (p.pid, s.created);
            if let Some(&(cpu, io)) = self.procs.get(&key) {
                deltas.insert(p.pid, (s.cpu_time.saturating_sub(cpu), s.io_bytes.saturating_sub(io)));
            }
            current.insert(key, (s.cpu_time, s.io_bytes));
        }
        self.procs = current;

        let mut seen = HashSet::with_capacity(apps.len());
        for app in apps.iter_mut() {
            let (cpu, io) = app
                .instances
                .iter()
                .filter_map(|(pid, _)| deltas.get(pid))
                .fold((0u64, 0u64), |acc, d| (acc.0 + d.0, acc.1 + d.1));
            // Displayed CPU is the latest interval, as a share of all cores like Task Manager.
            let pct_of_core = if secs > 0.0 { cpu as f64 / 1e7 / secs * 100.0 } else { 0.0 };
            app.cpu_pct = (pct_of_core / self.cpus) as f32;

            let key = app.name.to_lowercase();
            let state = self.apps.entry(key.clone()).or_insert_with(|| AppState::new(now));
            let (avg_cpu, avg_io) = state.push(now, cpu, io);
            let active = foreground.is_some_and(|fg| app.instances.iter().any(|&(pid, _)| pid == fg))
                || avg_cpu >= ACTIVE_CPU_PCT_OF_CORE
                || avg_io >= ACTIVE_IO_BYTES_PER_SEC;
            if active {
                state.last_active = now;
            }
            app.status = Status::from_quiet_time(now - state.last_active);
            seen.insert(key);
        }
        self.apps.retain(|name, _| seen.contains(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Category;
    use crate::process::ProcStats;

    fn proc(pid: u32, cpu_time: u64, io_bytes: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent_pid: 0,
            name: "app.exe".into(),
            session: Some(1),
            stats: Some(ProcStats { private: 0, working_set: 0, created: 1, cpu_time, io_bytes, chromium_child: false }),
        }
    }

    fn app(pids: &[u32]) -> AppUsage {
        AppUsage {
            name: "app.exe".into(),
            category: Category::Application,
            count: pids.len(),
            private: 0,
            working_set: 0,
            instances: pids.iter().map(|&pid| (pid, 1)).collect(),
            cpu_pct: 0.0,
            status: Status::Active,
            lowered: false,
        }
    }

    #[test]
    fn status_thresholds() {
        assert_eq!(Status::from_quiet_time(Duration::from_secs(59)), Status::Active);
        assert_eq!(Status::from_quiet_time(Duration::from_secs(60)), Status::Inactive);
        assert_eq!(Status::from_quiet_time(IDLE_AFTER), Status::Idle(IDLE_AFTER));
    }

    #[test]
    fn quiet_app_becomes_idle_and_wakes_on_cpu() {
        let mut t = Tracker::new();
        let start = Instant::now();
        let mut apps = [app(&[1])];

        t.update(&[proc(1, 0, 0)], &mut apps, None, start);
        assert_eq!(apps[0].status, Status::Active, "first sighting counts as active");

        let later = start + IDLE_AFTER + Duration::from_secs(1);
        t.update(&[proc(1, 0, 0)], &mut apps, None, later);
        assert!(matches!(apps[0].status, Status::Idle(_)));

        // 1 s of CPU is a wake-up: about 3% of a core over the 30 s window.
        let blip = later + Duration::from_secs(2);
        t.update(&[proc(1, 10_000_000, 0)], &mut apps, None, blip);
        assert!(matches!(apps[0].status, Status::Idle(_)), "brief wake-up keeps the app idle");

        // 1 s more of CPU brings the window average above 5% of a core.
        let busy = blip + Duration::from_secs(2);
        t.update(&[proc(1, 20_000_000, 0)], &mut apps, None, busy);
        assert_eq!(apps[0].status, Status::Active);
    }

    #[test]
    fn foreground_counts_as_activity() {
        let mut t = Tracker::new();
        let start = Instant::now();
        let mut apps = [app(&[1, 2])];
        t.update(&[proc(1, 0, 0), proc(2, 0, 0)], &mut apps, None, start);
        let later = start + IDLE_AFTER * 2;
        t.update(&[proc(1, 0, 0), proc(2, 0, 0)], &mut apps, Some(2), later);
        assert_eq!(apps[0].status, Status::Active);
    }

    #[test]
    fn new_process_cpu_history_is_not_activity() {
        let mut t = Tracker::new();
        let start = Instant::now();
        let mut apps = [app(&[1])];
        t.update(&[proc(1, 0, 0)], &mut apps, None, start);
        // A helper with a large lifetime CPU total appears after the app went idle.
        let later = start + IDLE_AFTER + Duration::from_secs(1);
        let mut apps = [app(&[1, 2])];
        t.update(&[proc(1, 0, 0), proc(2, 50_000_000, 0)], &mut apps, None, later);
        assert!(matches!(apps[0].status, Status::Idle(_)));
    }
}
