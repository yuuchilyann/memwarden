//! Per-process data from Win32 (read-only access rights only).

use std::collections::{HashMap, HashSet};

use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, STILL_ACTIVE, UNICODE_STRING};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessIoCounters, GetProcessTimes, IO_COUNTERS, OpenProcess, PROCESS_ACCESS_RIGHTS,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Process instance: PID plus start time, so a reused PID is never touched.
pub type Instance = (u32, u64);

/// Process handle closed on drop.
pub struct OwnedProcess(HANDLE);

impl OwnedProcess {
    pub fn raw(&self) -> HANDLE {
        self.0
    }

    #[cfg(test)]
    pub fn from_raw(handle: HANDLE) -> Self {
        Self(handle)
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Opens a process with `access` (plus limited query) only if it is still
/// the same instance, so a reused PID is never acted on.
pub fn open_instance(inst: Instance, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedProcess> {
    let (pid, created) = inst;
    let process = OwnedProcess(unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe { GetProcessTimes(process.raw(), &mut c, &mut e, &mut k, &mut u) }.ok()?;
    (ticks(c) == created).then_some(process)
}

/// Whether the instance is still running. An exited process can still be
/// opened while handles to it remain, so its exit code is checked too.
pub fn is_running(inst: Instance) -> bool {
    let Some(process) = open_instance(inst, PROCESS_QUERY_LIMITED_INFORMATION) else {
        return false;
    };
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(process.raw(), &mut code) }.is_ok() && code == STILL_ACTIVE.0 as u32
}

/// Current working set of an open process, in bytes.
pub fn working_set(process: &OwnedProcess) -> Option<u64> {
    let mut pmc = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    unsafe { GetProcessMemoryInfo(process.raw(), &mut pmc, pmc.cb) }.ok()?;
    Some(pmc.WorkingSetSize as u64)
}

pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
    /// Terminal Services session; session 0 hosts services.
    pub session: Option<u32>,
    /// `None` when the process cannot be opened (protected or higher privilege).
    pub stats: Option<ProcStats>,
}

pub struct ProcStats {
    pub private: u64,
    pub working_set: u64,
    /// Start time in FILETIME ticks; together with the PID it identifies one process instance.
    pub created: u64,
    /// Kernel + user CPU time in 100 ns units.
    pub cpu_time: u64,
    /// Bytes moved by read, write and other I/O operations.
    pub io_bytes: u64,
    /// Chromium-style child process (`--type=` argument), as spawned by Electron and CEF apps.
    pub chromium_child: bool,
}

/// Enumerates processes, caching per-instance facts that never change.
#[derive(Default)]
pub struct Collector {
    chromium_child: HashMap<(u32, u64), bool>,
}

impl Collector {
    pub fn list(&mut self) -> windows::core::Result<Vec<ProcessInfo>> {
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };
        let mut out = Vec::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
        while ok {
            let pid = entry.th32ProcessID;
            // PID 0 is the idle pseudo-process, not a real program.
            if pid != 0 {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let mut session = 0;
                out.push(ProcessInfo {
                    pid,
                    parent_pid: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..len]),
                    session: unsafe { ProcessIdToSessionId(pid, &mut session) }.ok().map(|_| session),
                    stats: self.stats_of(pid),
                });
            }
            ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
        }
        unsafe { CloseHandle(snap)? };

        // Forget processes that have exited.
        let alive: HashSet<(u32, u64)> =
            out.iter().filter_map(|p| p.stats.as_ref().map(|s| (p.pid, s.created))).collect();
        self.chromium_child.retain(|key, _| alive.contains(key));
        Ok(out)
    }

    fn stats_of(&mut self, pid: u32) -> Option<ProcStats> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let stats = self.read_stats(handle, pid);
        unsafe {
            let _ = CloseHandle(handle);
        }
        stats
    }

    fn read_stats(&mut self, handle: HANDLE, pid: u32) -> Option<ProcStats> {
        let mut pmc = PROCESS_MEMORY_COUNTERS_EX {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        unsafe { GetProcessMemoryInfo(handle, &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb) }.ok()?;

        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
        let created = ticks(created);

        let mut io = IO_COUNTERS::default();
        let io_bytes = match unsafe { GetProcessIoCounters(handle, &mut io) } {
            Ok(()) => io.ReadTransferCount + io.WriteTransferCount + io.OtherTransferCount,
            Err(_) => 0,
        };

        let chromium_child =
            *self.chromium_child.entry((pid, created)).or_insert_with(|| has_chromium_type_flag(handle));

        Some(ProcStats {
            private: pmc.PrivateUsage as u64,
            working_set: pmc.WorkingSetSize as u64,
            created,
            cpu_time: ticks(kernel) + ticks(user),
            io_bytes,
            chromium_child,
        })
    }
}

fn ticks(t: FILETIME) -> u64 {
    (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64
}

/// Checks the command line for Chromium's `--type=` switch.
/// The command line can hold tokens or private paths, so it is only scanned
/// here and dropped; it is never stored, logged or displayed.
fn has_chromium_type_flag(handle: HANDLE) -> bool {
    let mut len = 0u32;
    unsafe {
        let _ = NtQueryInformationProcess(handle, ProcessCommandLineInformation, std::ptr::null_mut(), 0, &mut len);
    }
    if len == 0 {
        return false;
    }
    // u64 elements keep the buffer aligned for the UNICODE_STRING header.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    let status = unsafe {
        NtQueryInformationProcess(handle, ProcessCommandLineInformation, buf.as_mut_ptr().cast(), len, &mut len)
    };
    if status.0 < 0 {
        return false;
    }
    let text = unsafe { &*(buf.as_ptr() as *const UNICODE_STRING) };
    if text.Buffer.is_null() {
        return false;
    }
    let chars = unsafe { std::slice::from_raw_parts(text.Buffer.0, text.Length as usize / 2) };
    let needle: Vec<u16> = "--type=".encode_utf16().collect();
    chars.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// Process owning the foreground window, if any.
pub fn foreground_pid() -> Option<u32> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}
