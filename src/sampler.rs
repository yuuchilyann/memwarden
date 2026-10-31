//! Background sampling thread, so slow process enumeration never blocks the UI.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::activity::Tracker;
use crate::audio;
use crate::classify::{self, AppUsage};
use crate::growth;
use crate::pagefile::{self, PageFiles};
use crate::priority::Governor;
use crate::process::{self, Collector};
use crate::system::{self, SystemMemory};

pub struct Snapshot {
    pub taken: Instant,
    pub sys: SystemMemory,
    pub apps: Vec<AppUsage>,
    pub denied: usize,
    /// `None` when the settings cannot be read.
    pub pagefile: Option<PageFiles>,
}

/// Starts sampling every `interval`; the UI is woken after each new snapshot.
pub fn spawn(ctx: egui::Context, interval: Duration, governor: Arc<Mutex<Governor>>) -> Receiver<Snapshot> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            audio::init_thread();
            let mut collector = Collector::default();
            let mut tracker = Tracker::new();
            let mut growth = growth::Tracker::default();
            loop {
                if let (Ok(sys), Ok(procs)) = (system::query(), collector.list()) {
                    let now = Instant::now();
                    let (mut apps, denied) = classify::group(&procs);
                    tracker.update(&procs, &mut apps, process::foreground_pid(), &audio::active_pids(), now);
                    growth.update(&mut apps, now);
                    governor.lock().unwrap_or_else(|e| e.into_inner()).sync(&mut apps);
                    let pagefile = pagefile::query(sys.phys_total);
                    let snap = Snapshot { taken: now, sys, apps, denied, pagefile };
                    if tx.send(snap).is_err() {
                        break; // UI closed
                    }
                    ctx.request_repaint();
                }
                thread::sleep(interval);
            }
        })
        .expect("failed to start sampler thread");
    rx
}
