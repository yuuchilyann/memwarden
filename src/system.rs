//! System-wide memory counters.

use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// All values in bytes.
pub struct SystemMemory {
    pub load_percent: u32,
    pub phys_total: u64,
    pub phys_avail: u64,
    pub commit_total: u64,
    pub commit_limit: u64,
    pub system_cache: u64,
    pub kernel_paged: u64,
    pub kernel_nonpaged: u64,
}

pub fn query() -> windows::core::Result<SystemMemory> {
    let mut ms = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    let mut pi = PERFORMANCE_INFORMATION::default();
    unsafe {
        GlobalMemoryStatusEx(&mut ms)?;
        GetPerformanceInfo(&mut pi, size_of::<PERFORMANCE_INFORMATION>() as u32)?;
    }
    // PERFORMANCE_INFORMATION reports page counts.
    let page = pi.PageSize as u64;
    Ok(SystemMemory {
        load_percent: ms.dwMemoryLoad,
        phys_total: ms.ullTotalPhys,
        phys_avail: ms.ullAvailPhys,
        commit_total: pi.CommitTotal as u64 * page,
        commit_limit: pi.CommitLimit as u64 * page,
        system_cache: pi.SystemCache as u64 * page,
        kernel_paged: pi.KernelPaged as u64 * page,
        kernel_nonpaged: pi.KernelNonpaged as u64 * page,
    })
}
