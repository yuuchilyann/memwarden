//! Finds applications whose private memory keeps growing (milestone M8).
//!
//! Long-running apps, Electron and WebView2 ones especially, often grow all
//! day through leaks or caches that never shrink. A single reading cannot
//! show that, so each app's lowest private memory per minute (its floor) is
//! kept for a few hours: garbage collection and short-lived allocations cause
//! spikes, while a leak shows as a rising floor. Private memory is commit
//! charge, so restarting such an app lowers commit. Kept in memory only.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::classify::{AppUsage, Category};
use crate::process::Instance;

/// How long floors are kept, in minutes.
const KEEP_MINUTES: u32 = 6 * 60;
/// Observation time, in minutes, before growth is reported.
pub const MIN_OBSERVED_MINUTES: u32 = 60;
/// The current floor is the lowest value of this many recent minutes.
const RECENT_MINUTES: u32 = 10;
/// Minimum increase of the floor worth reporting.
pub const MIN_INCREASE: u64 = 500 * 1024 * 1024;
/// Minimum ratio of the current floor to the earlier lowest floor.
pub const MIN_RATIO: f64 = 1.5;

/// Floor history of a growing app, set on `AppUsage::growth`.
#[derive(Clone, PartialEq, Debug)]
pub struct Growth {
    /// Time covered by the history.
    pub span: Duration,
    /// Lowest floor before the recent minutes.
    pub low: u64,
    /// Lowest private bytes within the recent minutes.
    pub current: u64,
    /// Per-minute floors, oldest first, as `(minute, bytes)`.
    pub floors: Vec<(u32, u64)>,
}

impl Growth {
    /// Roughly what restarting the app gives back.
    pub fn increase(&self) -> u64 {
        self.current.saturating_sub(self.low)
    }
}

/// Whether restarting `app` is suggested: it keeps growing and is not memwarden itself.
/// memwarden's own growth is still shown, to spot leaks in memwarden.
pub fn suggested(app: &AppUsage) -> bool {
    app.growth.is_some() && !app.instances.iter().any(|&(pid, _)| pid == std::process::id())
}

/// Floors of one application since it was last (re)started.
struct History {
    /// Process instances in the latest sample, to notice a restart.
    instances: Vec<Instance>,
    started: Instant,
    /// Completed minutes as `(minutes since started, lowest private bytes)`, oldest first.
    floors: VecDeque<(u32, u64)>,
    /// The minute in progress and its lowest value so far.
    minute: (u32, u64),
}

impl History {
    fn new(now: Instant, app: &AppUsage) -> Self {
        Self { instances: app.instances.clone(), started: now, floors: VecDeque::new(), minute: (0, app.private) }
    }

    fn record(&mut self, now: Instant, private: u64) {
        let m = ((now - self.started).as_secs() / 60) as u32;
        if m == self.minute.0 {
            self.minute.1 = self.minute.1.min(private);
        } else {
            self.floors.push_back(self.minute);
            self.minute = (m, private);
        }
        while self.floors.front().is_some_and(|&(i, _)| m - i >= KEEP_MINUTES) {
            self.floors.pop_front();
        }
    }

    /// The growth if every condition holds. Only floors before the recent
    /// minutes count as the low point, so the lowest floor always comes
    /// before the growth and an app still loading does not count.
    fn growth(&self) -> Option<Growth> {
        let now = self.minute.0;
        if now < MIN_OBSERVED_MINUTES {
            return None;
        }
        let recent = |i: u32| i + RECENT_MINUTES > now;
        let all = || self.floors.iter().copied().chain([self.minute]);
        let current = all().filter(|&(i, _)| recent(i)).map(|(_, b)| b).min()?;
        let low = all().filter(|&(i, _)| !recent(i)).map(|(_, b)| b).min()?;
        let grown = current.saturating_sub(low) >= MIN_INCREASE && current as f64 >= low as f64 * MIN_RATIO;
        grown.then(|| {
            let first = self.floors.front().map_or(now, |&(i, _)| i);
            Growth { span: Duration::from_secs(u64::from(now - first) * 60), low, current, floors: all().collect() }
        })
    }
}

#[derive(Default)]
pub struct Tracker {
    /// Per application, keyed by lower-case name.
    apps: HashMap<String, History>,
}

impl Tracker {
    /// Records the private memory of each application and fills `growth`.
    /// Browsers are left out (their tab count drives memory; discarding tabs
    /// helps there), as are categories that are never idle candidates.
    pub fn update(&mut self, apps: &mut [AppUsage], now: Instant) {
        let mut seen = HashSet::with_capacity(apps.len());
        for app in apps.iter_mut() {
            app.growth = None;
            if !matches!(app.category, Category::Application | Category::WebApp) {
                continue;
            }
            let key = app.name.to_lowercase();
            let history = self.apps.entry(key.clone()).or_insert_with(|| History::new(now, app));
            // No process in common with the last sample: the app was restarted.
            if !history.instances.iter().any(|i| app.instances.contains(i)) {
                *history = History::new(now, app);
            }
            history.instances.clone_from(&app.instances);
            history.record(now, app.private);
            app.growth = history.growth();
            seen.insert(key);
        }
        self.apps.retain(|name, _| seen.contains(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::Status;

    const MB: u64 = 1024 * 1024;
    const MINUTE: Duration = Duration::from_secs(60);

    fn app(category: Category, private: u64, instances: &[Instance]) -> AppUsage {
        AppUsage {
            name: "app.exe".into(),
            category,
            count: instances.len(),
            private,
            working_set: 0,
            instances: instances.to_vec(),
            cpu_pct: 0.0,
            status: Status::Active,
            lowered: false,
            audio: false,
            growth: None,
        }
    }

    /// Feeds one sample every 30 s for `minutes`, with private bytes from `at(minute)`.
    fn run(t: &mut Tracker, start: Instant, minutes: u32, category: Category, at: impl Fn(u32) -> u64) -> AppUsage {
        let mut apps = [app(category, 0, &[(1, 1)])];
        for half in 0..minutes * 2 {
            apps[0].private = at(half / 2);
            t.update(&mut apps, start + MINUTE / 2 * half);
        }
        apps.into_iter().next().unwrap()
    }

    /// 400 MB for the first hour, then 1.4 GB.
    fn leak(m: u32) -> u64 {
        if m < 60 { 400 * MB } else { 1_400 * MB }
    }

    #[test]
    fn floor_is_the_lowest_value_of_each_minute() {
        let start = Instant::now();
        let mut h = History::new(start, &app(Category::Application, 500 * MB, &[(1, 1)]));
        h.record(start + Duration::from_secs(20), 300 * MB);
        h.record(start + Duration::from_secs(40), 900 * MB);
        h.record(start + Duration::from_secs(70), 800 * MB);
        assert_eq!(h.floors, [(0, 300 * MB)]);
        assert_eq!(h.minute, (1, 800 * MB));
    }

    #[test]
    fn history_is_capped_at_six_hours() {
        let start = Instant::now();
        let mut h = History::new(start, &app(Category::Application, 0, &[(1, 1)]));
        for m in 1..=KEEP_MINUTES + 30 {
            h.record(start + MINUTE * m, u64::from(m));
        }
        assert_eq!(h.floors.len() as u32 + 1, KEEP_MINUTES);
        assert_eq!(h.floors.front().unwrap().0, 31);
    }

    #[test]
    fn steady_growth_is_reported() {
        let mut t = Tracker::default();
        let a = run(&mut t, Instant::now(), 75, Category::WebApp, leak);
        let g = a.growth.as_ref().expect("grew by 1 GB");
        assert_eq!((g.low, g.current, g.increase()), (400 * MB, 1_400 * MB, 1_000 * MB));
        assert_eq!(g.span, MINUTE * 74);
        assert_eq!(g.floors.len(), 75);
        assert!(suggested(&a));
    }

    #[test]
    fn needs_an_hour_of_observation() {
        let mut t = Tracker::default();
        let a = run(&mut t, Instant::now(), MIN_OBSERVED_MINUTES, Category::Application, |m| {
            if m < 30 { 400 * MB } else { 1_400 * MB }
        });
        assert!(a.growth.is_none());
    }

    #[test]
    fn spikes_do_not_count() {
        // Every other sample is 2 GB higher, as with garbage collection.
        let mut t = Tracker::default();
        let start = Instant::now();
        let mut apps = [app(Category::Application, 0, &[(1, 1)])];
        for half in 0..150 {
            apps[0].private = if half % 2 == 0 { 400 * MB } else { 2_400 * MB };
            t.update(&mut apps, start + MINUTE / 2 * half);
        }
        assert!(apps[0].growth.is_none());
    }

    #[test]
    fn small_or_proportionally_small_growth_is_not_reported() {
        let mut t = Tracker::default();
        let small = run(&mut t, Instant::now(), 75, Category::Application, |m| if m < 60 { 100 * MB } else { 590 * MB });
        assert!(small.growth.is_none(), "below MIN_INCREASE");
        let mut t = Tracker::default();
        let big = run(&mut t, Instant::now(), 75, Category::Application, |m| if m < 60 { 2_000 * MB } else { 2_900 * MB });
        assert!(big.growth.is_none(), "below MIN_RATIO");
    }

    #[test]
    fn shrinking_back_clears_the_flag() {
        let mut t = Tracker::default();
        let a = run(&mut t, Instant::now(), 90, Category::Application, |m| if m < 75 { leak(m) } else { 400 * MB });
        assert!(a.growth.is_none(), "the recent floor is back at the start");
    }

    #[test]
    fn only_apps_and_web_apps_are_tracked() {
        for category in [Category::Browser, Category::System, Category::Security, Category::VirtualMachine, Category::Service] {
            let mut t = Tracker::default();
            assert!(run(&mut t, Instant::now(), 75, category, leak).growth.is_none(), "{category:?}");
            assert!(t.apps.is_empty());
        }
    }

    #[test]
    fn restart_clears_the_history() {
        let mut t = Tracker::default();
        let start = Instant::now();
        let mut a = run(&mut t, start, 75, Category::Application, leak);
        assert!(a.growth.is_some());
        // Same name, but none of the old process instances.
        a.instances = vec![(1, 2), (5, 3)];
        t.update(std::slice::from_mut(&mut a), start + MINUTE * 76);
        assert!(a.growth.is_none());
        assert!(t.apps["app.exe"].floors.is_empty());
        // A new helper process alongside an old one is not a restart.
        a.instances = vec![(1, 2), (6, 4)];
        t.update(std::slice::from_mut(&mut a), start + MINUTE * 78);
        assert_eq!(t.apps["app.exe"].floors.len(), 1);
    }

    #[test]
    fn memwarden_itself_is_shown_but_not_suggested() {
        let mut t = Tracker::default();
        let mut a = run(&mut t, Instant::now(), 75, Category::Application, leak);
        a.instances.push((std::process::id(), 0));
        assert!(a.growth.is_some());
        assert!(!suggested(&a));
    }
}
