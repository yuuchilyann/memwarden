//! Background sampling thread, so slow process enumeration never blocks the UI.

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::process::{self, AppUsage};
use crate::system::{self, SystemMemory};

pub struct Snapshot {
    pub taken: Instant,
    pub sys: SystemMemory,
    pub apps: Vec<AppUsage>,
    pub denied: usize,
}

/// Starts sampling every `interval`; the UI is woken after each new snapshot.
pub fn spawn(ctx: egui::Context, interval: Duration) -> Receiver<Snapshot> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            loop {
                if let (Ok(sys), Ok((apps, denied))) = (system::query(), process::grouped()) {
                    let snap = Snapshot { taken: Instant::now(), sys, apps, denied };
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
