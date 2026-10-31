//! System-wide memory counters.

use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// All values in bytes.
pub struct SystemMemory {
    pub phys_total: u64,
    pub phys_avail: u64,
    pub commit_total: u64,
    pub commit_limit: u64,
    pub system_cache: u64,
    pub kernel_paged: u64,
    pub kernel_nonpaged: u64,
}

impl SystemMemory {
    pub fn phys_used(&self) -> u64 {
        self.phys_total - self.phys_avail
    }

    pub fn phys_pct(&self) -> f64 {
        self.phys_used() as f64 * 100.0 / self.phys_total as f64
    }

    pub fn commit_pct(&self) -> f64 {
        self.commit_total as f64 * 100.0 / self.commit_limit as f64
    }
}

pub fn query() ->windows::core::Result<SystemMemory> {
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
        phys_total: ms.ullTotalPhys,
        phys_avail: ms.ullAvailPhys,
        commit_total: pi.CommitTotal as u64 * page,
        commit_limit: pi.CommitLimit as u64 * page,
        system_cache: pi.SystemCache as u64 * page,
        kernel_paged: pi.KernelPaged as u64 * page,
        kernel_nonpaged: pi.KernelNonpaged as u64 * page,
    })
}
