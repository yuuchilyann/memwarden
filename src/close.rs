//! Suggests closing long-idle applications and closes them on request (milestone M7).
//!
//! Only the private memory an application releases itself lowers commit
//! charge, so for an app opened once and then forgotten, closing it is what
//! helps. Closing posts `WM_CLOSE` to the app's visible top-level windows,
//! the same as the user clicking X: the app decides whether to ask about
//! unsaved work, and may also just hide in the notification area. Processes
//! are never terminated.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsHungAppWindow,
    IsWindowVisible, PostMessageW, WM_CLOSE, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

use crate::activity::Status;
use crate::classify::{AppUsage, Category};
use crate::process::{Instance, is_running, open_instance};

/// Minimum idle time before closing is suggested. Longer than trimming's,
/// because closing cannot be undone.
pub const SUGGEST_AFTER: Duration = Duration::from_secs(30 * 60);
/// Minimum private memory worth suggesting a close for.
pub const SUGGEST_MIN_PRIVATE: u64 = 200 * 1024 * 1024;
/// Windows gone but processes still running this long: the app went to the tray.
const BACKGROUND_AFTER: Duration = Duration::from_secs(10);
/// Windows still open this long: the app is probably asking about unsaved work.
const WAIT: Duration = Duration::from_secs(30);

/// Lower-case executable names never suggested for closing: file sync
/// clients look idle but keep files in sync in the background.
const KEEP_RUNNING: &[&str] = &[
    "onedrive.exe", "dropbox.exe", "googledrivefs.exe", "box.exe", "icloudservices.exe", "iclouddrive.exe",
    "megasync.exe", "nextcloud.exe", "owncloud.exe",
];

/// Whether closing `app` is suggested: a long-idle, large application.
/// Browsers are left out, since discarding tabs keeps the tab strip.
pub fn suggested(app: &AppUsage) -> bool {
    matches!(app.category, Category::Application | Category::WebApp)
        && !KEEP_RUNNING.contains(&app.name.to_lowercase().as_str())
        && matches!(app.status, Status::Idle(quiet) if quiet >= SUGGEST_AFTER)
        && app.private >= SUGGEST_MIN_PRIVATE
        && !app.audio
        && !app.instances.iter().any(|&(pid, _)| pid == std::process::id())
}

/// Why an app cannot be closed from memwarden.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Refusal {
    /// No visible window; only the app's tray icon can end it.
    NoWindow,
    /// A window is not responding to messages.
    NotResponding,
    /// Every window refused the message (an elevated app blocks it).
    Denied,
}

/// What happened after the close request.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Outcome {
    /// Every process of the app has exited.
    Closed,
    /// The windows closed but the app keeps running, usually in the tray.
    InBackground,
    /// Windows are still open, probably asking whether to save.
    StillOpen,
}

/// A close request in progress.
pub struct Closing {
    pub name: String,
    /// Private memory of the app when closing was requested.
    pub private: u64,
    instances: Vec<Instance>,
    sent: Instant,
    /// When the app was first seen without windows.
    windows_gone: Option<Instant>,
}

/// Why `instances` cannot be closed right now, if anything; lets the UI hide the button.
pub fn blocker(instances: &[Instance]) -> Option<Refusal> {
    let windows = top_windows(instances);
    if windows.is_empty() {
        Some(Refusal::NoWindow)
    } else if windows.iter().any(|&w| unsafe { IsHungAppWindow(w) }.as_bool()) {
        Some(Refusal::NotResponding)
    } else {
        None
    }
}

/// Asks `app` to close by posting `WM_CLOSE` to each of its top-level windows.
/// Called only after the user confirmed closing this app.
pub fn request(app: &AppUsage, now: Instant) -> Result<Closing, Refusal> {
    if let Some(refusal) = blocker(&app.instances) {
        return Err(refusal);
    }
    let mut posted = 0;
    for hwnd in top_windows(&app.instances) {
        if unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok() {
            posted += 1;
        }
    }
    if posted == 0 {
        return Err(Refusal::Denied);
    }
    Ok(Closing {
        name: app.name.clone(),
        private: app.private,
        instances: app.instances.clone(),
        sent: now,
        windows_gone: None,
    })
}

impl Closing {
    /// Checks progress; `None` while the app may still be closing.
    pub fn poll(&mut self, now: Instant) -> Option<Outcome> {
        let running: Vec<Instance> = self.instances.iter().copied().filter(|&i| is_running(i)).collect();
        let windows = if running.is_empty() { 0 } else { top_windows(&running).len() };
        if windows > 0 {
            self.windows_gone = None;
        } else if self.windows_gone.is_none() {
            self.windows_gone = Some(now);
        }
        let windowless = self.windows_gone.map_or(Duration::ZERO, |t| now - t);
        decide(running.len(), windows, now - self.sent, windowless)
    }
}

fn decide(running: usize, windows: usize, elapsed: Duration, windowless: Duration) -> Option<Outcome> {
    if running == 0 {
        Some(Outcome::Closed)
    } else if windows == 0 && windowless >= BACKGROUND_AFTER {
        Some(Outcome::InBackground)
    } else if elapsed >= WAIT {
        Some(if windows > 0 { Outcome::StillOpen } else { Outcome::InBackground })
    } else {
        None
    }
}

/// Visible, unowned top-level windows of `instances` that would show on the
/// taskbar; tool windows (tray helpers, floating palettes) are left out.
/// A window counts only if its process is still the same instance.
fn top_windows(instances: &[Instance]) -> Vec<HWND> {
    let mut all: Vec<(HWND, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut all as *mut Vec<(HWND, u32)> as isize));
    }
    let live: Vec<u32> = instances
        .iter()
        .filter(|&&inst| open_instance(inst, PROCESS_QUERY_LIMITED_INFORMATION).is_some())
        .map(|&(pid, _)| pid)
        .collect();
    all.into_iter().filter(|(_, pid)| live.contains(pid)).map(|(hwnd, _)| hwnd).collect()
}

unsafe extern "system" fn collect(hwnd: HWND, out: LPARAM) -> BOOL {
    let out = unsafe { &mut *(out.0 as *mut Vec<(HWND, u32)>) };
    let tool = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW.0 != 0;
    let owned = unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok();
    if unsafe { IsWindowVisible(hwnd) }.as_bool() && !tool && !owned {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        out.push((hwnd, pid));
    }
    true.into()
}

#[cfg(test)]
mod tests {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::thread;

    use windows::Win32::System::Threading::{CREATE_NO_WINDOW, GetProcessTimes, OpenProcess};

    use super::*;
    use crate::process::OwnedProcess;

    const MB: u64 = 1024 * 1024;

    fn app(category: Category, status: Status, private: u64) -> AppUsage {
        AppUsage {
            name: "app.exe".into(),
            category,
            count: 1,
            private,
            working_set: 0,
            instances: vec![(1, 1)],
            cpu_pct: 0.0,
            status,
            lowered: false,
            audio: false,
            growth: None,
        }
    }

    #[test]
    fn only_long_idle_large_apps_are_suggested() {
        let idle = Status::Idle(SUGGEST_AFTER);
        assert!(suggested(&app(Category::Application, idle, SUGGEST_MIN_PRIVATE)));
        assert!(suggested(&app(Category::WebApp, idle, 2_000 * MB)));
        assert!(!suggested(&app(Category::Application, Status::Idle(SUGGEST_AFTER - Duration::from_secs(1)), 2_000 * MB)));
        assert!(!suggested(&app(Category::Application, Status::Inactive, 2_000 * MB)));
        assert!(!suggested(&app(Category::Application, idle, SUGGEST_MIN_PRIVATE - 1)));
        for category in [Category::Browser, Category::System, Category::Security, Category::VirtualMachine, Category::Service] {
            assert!(!suggested(&app(category, idle, 2_000 * MB)), "{category:?}");
        }
    }

    #[test]
    fn sync_clients_are_never_suggested() {
        let mut onedrive = app(Category::Application, Status::Idle(SUGGEST_AFTER * 2), 2_000 * MB);
        onedrive.name = "OneDrive.exe".into();
        assert!(!suggested(&onedrive));
    }

    #[test]
    fn audio_and_memwarden_itself_are_never_suggested() {
        let idle = Status::Idle(SUGGEST_AFTER * 2);
        let mut playing = app(Category::Application, idle, 2_000 * MB);
        playing.audio = true;
        assert!(!suggested(&playing));
        let mut me = app(Category::Application, idle, 2_000 * MB);
        me.instances.push((std::process::id(), 0));
        assert!(!suggested(&me));
    }

    #[test]
    fn outcome_follows_processes_and_windows() {
        let s = Duration::from_secs;
        assert_eq!(decide(0, 0, s(1), s(0)), Some(Outcome::Closed));
        assert_eq!(decide(2, 1, s(5), s(0)), None);
        assert_eq!(decide(2, 0, s(5), s(5)), None, "may still be shutting down");
        assert_eq!(decide(2, 0, s(12), BACKGROUND_AFTER), Some(Outcome::InBackground));
        assert_eq!(decide(2, 1, WAIT, s(0)), Some(Outcome::StillOpen));
        assert_eq!(decide(1, 0, WAIT, s(2)), Some(Outcome::InBackground));
    }

    fn instance_of(pid: u32) -> Instance {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.unwrap();
        let probe = OwnedProcess::from_raw(handle);
        let (mut c, mut e, mut k, mut u) = Default::default();
        unsafe { GetProcessTimes(probe.raw(), &mut c, &mut e, &mut k, &mut u) }.unwrap();
        (pid, (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64)
    }

    /// Opens a minimized WinForms window in a child PowerShell and closes it the way the UI would.
    #[test]
    fn closes_a_real_window() {
        let script = "Add-Type -AssemblyName System.Windows.Forms; \
            $f = New-Object Windows.Forms.Form; $f.Text = 'memwarden close test'; \
            $f.WindowState = 'Minimized'; [Windows.Forms.Application]::Run($f)";
        let mut child = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()
            .unwrap();
        let inst = instance_of(child.id());
        let mut test_app = app(Category::Application, Status::Idle(SUGGEST_AFTER), 0);
        test_app.instances = vec![inst];

        let start = Instant::now();
        while blocker(&test_app.instances) == Some(Refusal::NoWindow) {
            assert!(start.elapsed() < Duration::from_secs(30), "test window never appeared");
            thread::sleep(Duration::from_millis(200));
        }

        // A different start time means another process: nothing to close.
        let other = [(inst.0, inst.1 + 1)];
        assert_eq!(blocker(&other), Some(Refusal::NoWindow));

        let now = Instant::now();
        let mut closing = request(&test_app, now).expect("window accepts WM_CLOSE");
        let outcome = loop {
            if let Some(outcome) = closing.poll(Instant::now()) {
                break outcome;
            }
            thread::sleep(Duration::from_millis(200));
        };
        if outcome != Outcome::Closed {
            let _ = child.kill();
        }
        let _ = child.wait();
        assert_eq!(outcome, Outcome::Closed);
        assert!(!is_running(inst));
    }
}
