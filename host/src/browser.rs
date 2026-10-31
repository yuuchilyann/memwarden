//! Finds the browser process that launched a host.
//! memwarden runs this on the PID of each pipe client instead of trusting
//! anything the client says about itself. Chrome on Windows may start the
//! host through `cmd.exe`, so the walk skips intermediate processes.

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Browsers the extension supports (see PLAN.md 7.1).
const BROWSERS: &[&str] = &["chrome.exe", "msedge.exe"];

/// How many ancestors to inspect before giving up.
const MAX_DEPTH: usize = 8;

/// Process instance: PID plus start time in FILETIME ticks, as in memwarden's `process.rs`.
pub type Instance = (u32, u64);

/// The browser instance that is an ancestor of process `pid`, if any.
pub fn browser_ancestor(pid: u32) -> Option<Instance> {
    let procs = snapshot()?;
    find_browser(pid, &procs, created)
}

/// Walks up from `start` to the first ancestor named like a browser.
/// A parent that started after its child is a reused PID, not the real parent,
/// so the walk stops there instead of reporting an unrelated process.
fn find_browser(
    start: u32,
    procs: &HashMap<u32, (u32, String)>,
    created: impl Fn(u32) -> Option<u64>,
) -> Option<Instance> {
    let mut pid = start;
    let mut child_created = created(pid)?;
    for _ in 0..MAX_DEPTH {
        let parent = procs.get(&pid)?.0;
        if parent == 0 || parent == pid {
            return None;
        }
        let parent_created = created(parent)?;
        if parent_created > child_created {
            return None;
        }
        let name = &procs.get(&parent)?.1;
        if BROWSERS.iter().any(|b| b.eq_ignore_ascii_case(name)) {
            return Some((parent, parent_created));
        }
        pid = parent;
        child_created = parent_created;
    }
    None
}

/// PID → (parent PID, executable name) for every process.
fn snapshot() -> Option<HashMap<u32, (u32, String)>> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let mut out = HashMap::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        out.insert(
            entry.th32ProcessID,
            (entry.th32ParentProcessID, String::from_utf16_lossy(&entry.szExeFile[..len])),
        );
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snap);
    }
    Some(out)
}

/// Start time of a process, opened with limited query rights only.
fn created(pid: u32) -> Option<u64> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let (mut c, mut e, mut k, mut u) = Default::default();
    let ok = unsafe { GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) }.is_ok();
    unsafe {
        let _ = CloseHandle(handle);
    }
    ok.then(|| ticks(c))
}

fn ticks(t: FILETIME) -> u64 {
    (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (pid, parent, name, created)
    fn tree(entries: &[(u32, u32, &str, u64)]) -> (HashMap<u32, (u32, String)>, HashMap<u32, u64>) {
        let procs = entries.iter().map(|&(pid, parent, name, _)| (pid, (parent, name.to_string()))).collect();
        let times = entries.iter().map(|&(pid, _, _, created)| (pid, created)).collect();
        (procs, times)
    }

    #[test]
    fn finds_browser_through_cmd() {
        let (procs, times) =
            tree(&[(1, 0, "explorer.exe", 10), (5, 1, "chrome.exe", 20), (7, 5, "cmd.exe", 30), (9, 7, "memwarden-host.exe", 31)]);
        assert_eq!(find_browser(9, &procs, |p| times.get(&p).copied()), Some((5, 20)));
    }

    #[test]
    fn finds_direct_parent_case_insensitively() {
        let (procs, times) = tree(&[(5, 1, "MSEdge.exe", 20), (9, 5, "memwarden-host.exe", 31)]);
        assert_eq!(find_browser(9, &procs, |p| times.get(&p).copied()), Some((5, 20)));
    }

    #[test]
    fn reused_parent_pid_is_not_followed() {
        // PID 5 now belongs to a chrome.exe started after the host: not our launcher.
        let (procs, times) = tree(&[(5, 1, "chrome.exe", 40), (9, 5, "memwarden-host.exe", 31)]);
        assert_eq!(find_browser(9, &procs, |p| times.get(&p).copied()), None);
    }

    #[test]
    fn no_browser_ancestor() {
        let (procs, times) = tree(&[(1, 0, "explorer.exe", 10), (7, 1, "cmd.exe", 30), (9, 7, "memwarden-host.exe", 31)]);
        assert_eq!(find_browser(9, &procs, |p| times.get(&p).copied()), None);
    }

    #[test]
    fn parent_cycle_terminates() {
        let (procs, times) = tree(&[(7, 9, "cmd.exe", 30), (9, 7, "memwarden-host.exe", 30)]);
        assert_eq!(find_browser(9, &procs, |p| times.get(&p).copied()), None);
    }
}
