//! Lowers and restores the memory priority of idle applications (milestone M4).
//!
//! A lower memory priority is a hint to the Windows memory manager: when
//! physical memory runs short, pages of these processes are reclaimed first.
//! The processes keep running, and commit charge does not change.
//!
//! Every change is reversible. Original values are kept in memory and in a
//! small journal file, so priorities are restored when the app becomes active,
//! when memwarden exits, or on the next start after a crash.

use std::collections::HashMap;
use std::path::PathBuf;

use windows::Win32::System::Threading::{
    GetProcessInformation, MEMORY_PRIORITY, MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_LOW,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, ProcessMemoryPriority, SetProcessInformation,
};

use crate::activity::Status;
use crate::classify::AppUsage;
use crate::process::{Instance, open_instance};

/// Priority applied to idle applications (Windows default is NORMAL = 5).
const LOWERED: MEMORY_PRIORITY = MEMORY_PRIORITY_LOW;

#[derive(Default)]
pub struct Governor {
    /// Lowered applications (lower-case name) → original priority per instance.
    lowered: HashMap<String, HashMap<Instance, u32>>,
    /// Processes that could not be changed in the last action (protected or elevated).
    pub failed: usize,
    /// Crash-recovery journal; `None` disables it.
    journal: Option<PathBuf>,
}

impl Governor {
    pub fn new() -> Self {
        Self::with_journal(default_journal_path())
    }

    /// Restores anything a previous run left lowered, then starts empty.
    fn with_journal(journal: Option<PathBuf>) -> Self {
        if let Some(path) = &journal {
            if let Ok(text) = std::fs::read_to_string(path) {
                for line in text.lines() {
                    let nums: Vec<u64> = line.split_whitespace().filter_map(|n| n.parse().ok()).collect();
                    if let [pid, created, original] = nums[..] {
                        let _ = set_priority((pid as u32, created), original as u32);
                    }
                }
            }
            let _ = std::fs::remove_file(path);
        }
        Self { journal, ..Self::default() }
    }

    pub fn is_lowered(&self, app_name: &str) -> bool {
        self.lowered.contains_key(&app_name.to_lowercase())
    }

    pub fn lowered_count(&self) -> usize {
        self.lowered.len()
    }

    /// Lowers every process of `app`. Returns how many could not be changed.
    pub fn lower(&mut self, app: &AppUsage) -> usize {
        let key = app.name.to_lowercase();
        let entry = self.lowered.entry(key.clone()).or_default();
        let failed = lower_instances(entry, &app.instances);
        if entry.is_empty() {
            self.lowered.remove(&key);
        }
        self.failed = failed;
        self.write_journal();
        failed
    }

    pub fn restore(&mut self, app_name: &str) {
        if let Some(instances) = self.lowered.remove(&app_name.to_lowercase()) {
            restore_instances(&instances);
            self.write_journal();
        }
    }

    pub fn restore_all(&mut self) {
        for instances in self.lowered.values() {
            restore_instances(instances);
        }
        self.lowered.clear();
        self.write_journal();
    }

    /// Called after each sample: restores apps that became active, lowers new
    /// processes of still-lowered apps, forgets exited processes, and marks
    /// `AppUsage::lowered`.
    pub fn sync(&mut self, apps: &mut [AppUsage]) {
        if self.lowered.is_empty() {
            return;
        }
        let mut changed = false;
        let mut alive = HashMap::with_capacity(self.lowered.len());
        for app in apps.iter_mut() {
            let key = app.name.to_lowercase();
            let Some(mut instances) = self.lowered.remove(&key) else { continue };
            changed = true;
            if app.status == Status::Active {
                restore_instances(&instances);
                continue;
            }
            instances.retain(|inst, _| app.instances.contains(inst));
            lower_instances(&mut instances, &app.instances);
            if !instances.is_empty() {
                app.lowered = true;
                alive.insert(key, instances);
            }
        }
        // Apps not in this sample have exited; nothing left to restore.
        self.lowered = alive;
        if changed {
            self.write_journal();
        }
    }

    fn write_journal(&self) {
        let Some(path) = &self.journal else { return };
        if self.lowered.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        // Only numbers: PID, start time, original priority.
        let text: String = self
            .lowered
            .values()
            .flatten()
            .map(|((pid, created), original)| format!("{pid} {created} {original}\n"))
            .collect();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// Lowers instances not yet recorded. Returns how many failed.
fn lower_instances(recorded: &mut HashMap<Instance, u32>, instances: &[Instance]) -> usize {
    let mut failed = 0;
    for &inst in instances {
        if recorded.contains_key(&inst) {
            continue;
        }
        match get_priority(inst) {
            Some(original) if set_priority(inst, LOWERED.0) => {
                recorded.insert(inst, original);
            }
            _ => failed += 1,
        }
    }
    failed
}

fn restore_instances(instances: &HashMap<Instance, u32>) {
    for (&inst, &original) in instances {
        let _ = set_priority(inst, original);
    }
}

fn default_journal_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("memwarden").join("lowered-priorities.txt"))
}

fn get_priority(inst: Instance) -> Option<u32> {
    let process = open_instance(inst, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut info = MEMORY_PRIORITY_INFORMATION::default();
    unsafe {
        GetProcessInformation(
            process.raw(),
            ProcessMemoryPriority,
            &mut info as *mut _ as *mut _,
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    }
    .ok()?;
    Some(info.MemoryPriority.0)
}

fn set_priority(inst: Instance, priority: u32) -> bool {
    let Some(process) = open_instance(inst, PROCESS_SET_INFORMATION) else { return false };
    let info = MEMORY_PRIORITY_INFORMATION { MemoryPriority: MEMORY_PRIORITY(priority) };
    unsafe {
        SetProcessInformation(
            process.raw(),
            ProcessMemoryPriority,
            &info as *const _ as *const _,
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    }
    .is_ok()
}
#[cfg(test)]
mod tests {
    use std::process::{Child, Command};
    use std::time::Duration;

    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess};

    use super::*;
    use crate::classify::Category;

    /// A short-lived child process this test owns.
    fn spawn_child() -> (Child, Instance) {
        let child = Command::new("cmd").args(["/c", "ping -n 30 127.0.0.1 >nul"]).spawn().unwrap();
        let pid = child.id();
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.unwrap();
        let (mut c, mut e, mut k, mut u) = Default::default();
        unsafe { GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) }.unwrap();
        unsafe { CloseHandle(handle) }.unwrap();
        let created = (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64;
        (child, (pid, created))
    }

    fn app(inst: Instance, status: Status) -> AppUsage {
        AppUsage {
            name: "cmd.exe".into(),
            category: Category::Application,
            count: 1,
            private: 0,
            working_set: 0,
            instances: vec![inst],
            cpu_pct: 0.0,
            status,
            lowered: false,
            audio: false,
        }
    }

    fn journal() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("memwarden-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("journal-{:?}.txt", std::thread::current().id()))
    }

    #[test]
    fn lower_and_restore_real_process() {
        let (mut child, inst) = spawn_child();
        let original = get_priority(inst).unwrap();
        let mut gov = Governor::with_journal(Some(journal()));

        assert_eq!(gov.lower(&app(inst, Status::Idle(Duration::ZERO))), 0);
        assert_eq!(get_priority(inst), Some(LOWERED.0));
        assert!(gov.is_lowered("CMD.EXE"));

        gov.restore("cmd.exe");
        assert_eq!(get_priority(inst), Some(original));
        assert_eq!(gov.lowered_count(), 0);
        let _ = child.kill();
    }

    #[test]
    fn becoming_active_restores_automatically() {
        let (mut child, inst) = spawn_child();
        let original = get_priority(inst).unwrap();
        let mut gov = Governor::with_journal(Some(journal()));
        gov.lower(&app(inst, Status::Idle(Duration::ZERO)));

        let mut apps = [app(inst, Status::Active)];
        gov.sync(&mut apps);
        assert_eq!(get_priority(inst), Some(original));
        assert!(!apps[0].lowered);
        assert_eq!(gov.lowered_count(), 0);
        let _ = child.kill();
    }

    #[test]
    fn journal_restores_after_crash() {
        let (mut child, inst) = spawn_child();
        let original = get_priority(inst).unwrap();
        let path = journal();
        let mut gov = Governor::with_journal(Some(path.clone()));
        gov.lower(&app(inst, Status::Idle(Duration::ZERO)));
        assert!(path.exists());
        std::mem::forget(gov); // simulate a crash: no restore

        let _next_run = Governor::with_journal(Some(path.clone()));
        assert_eq!(get_priority(inst), Some(original));
        assert!(!path.exists());
        let _ = child.kill();
    }

    #[test]
    fn reused_pid_is_not_touched() {
        let (mut child, (pid, created)) = spawn_child();
        // Same PID, different start time: must be treated as another process.
        assert!(!set_priority((pid, created + 1), LOWERED.0));
        assert_ne!(get_priority((pid, created)), Some(LOWERED.0));
        let _ = child.kill();
    }
}
