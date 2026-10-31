//! Per-process memory enumeration (read-only access rights only).

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

pub struct MemInfo {
    pub private: u64,
    pub working_set: u64,
}

pub struct ProcessInfo {
    pub name: String,
    /// `None` when the process cannot be opened (protected or higher privilege).
    pub mem: Option<MemInfo>,
}

pub fn list() -> windows::core::Result<Vec<ProcessInfo>> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };
    let mut out = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        let pid = entry.th32ProcessID;
        out.push(ProcessInfo {
            name: String::from_utf16_lossy(&entry.szExeFile[..len]),
            mem: if pid == 0 { None } else { memory_of(pid) },
        });
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    unsafe { CloseHandle(snap)? };
    Ok(out)
}

/// Memory of all processes sharing one executable name.
#[derive(Clone)]
pub struct AppUsage {
    pub name: String,
    pub count: usize,
    pub private: u64,
    pub working_set: u64,
}

/// Groups processes by executable name so multi-process apps (Chrome, Electron)
/// appear as one entry, sorted by private bytes. Also returns how many processes
/// could not be read.
pub fn grouped() -> windows::core::Result<(Vec<AppUsage>, usize)> {
    let mut groups: HashMap<String, AppUsage> = HashMap::new();
    let mut denied = 0usize;
    for p in list()? {
        let Some(m) = p.mem else {
            denied += 1;
            continue;
        };
        let g = groups.entry(p.name.clone()).or_insert_with(|| AppUsage {
            name: p.name,
            count: 0,
            private: 0,
            working_set: 0,
        });
        g.count += 1;
        g.private += m.private;
        g.working_set += m.working_set;
    }
    let mut apps: Vec<_> = groups.into_values().collect();
    apps.sort_by(|a, b| b.private.cmp(&a.private));
    Ok((apps, denied))
}

fn memory_of(pid: u32) -> Option<MemInfo> {
    let handle: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut pmc = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let res = unsafe {
        GetProcessMemoryInfo(
            handle,
            &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            pmc.cb,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    res.ok()?;
    Some(MemInfo {
        private: pmc.PrivateUsage as u64,
        working_set: pmc.WorkingSetSize as u64,
    })
}
