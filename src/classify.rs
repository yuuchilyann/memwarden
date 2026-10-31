//! Groups processes into applications and assigns each a category.

use std::collections::HashMap;

use crate::activity::Status;
use crate::process::ProcessInfo;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Category {
    System,
    Security,
    VirtualMachine,
    Service,
    Browser,
    WebApp,
    Application,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Application,
        Category::WebApp,
        Category::Browser,
        Category::Service,
        Category::VirtualMachine,
        Category::Security,
        Category::System,
    ];

    /// Translation key of the category name.
    pub fn label_key(self) -> &'static str {
        match self {
            Category::System => "category.system",
            Category::Security => "category.security",
            Category::VirtualMachine => "category.vm",
            Category::Service => "category.service",
            Category::Browser => "category.browser",
            Category::WebApp => "category.webapp",
            Category::Application => "category.application",
        }
    }

    /// Whether apps of this category may be reported as idle. System parts,
    /// security software, VMs and services are never suggested.
    pub fn idle_candidate(self) -> bool {
        matches!(self, Category::Browser | Category::WebApp | Category::Application)
    }
}

// Lower-case executable names. Extend these lists rather than adding special cases.
const SYSTEM: &[&str] = &[
    "system", "registry", "secure system", "memory compression", "smss.exe", "csrss.exe",
    "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe", "lsaiso.exe", "svchost.exe",
    "dwm.exe", "fontdrvhost.exe", "sihost.exe", "ctfmon.exe", "taskhostw.exe", "runtimebroker.exe",
    "searchindexer.exe", "searchhost.exe", "spoolsv.exe", "conhost.exe", "dllhost.exe",
    "audiodg.exe", "wudfhost.exe", "explorer.exe", "shellexperiencehost.exe",
    "startmenuexperiencehost.exe", "textinputhost.exe", "lockapp.exe", "applicationframehost.exe",
    "systemsettings.exe", "wmiprvse.exe", "dashost.exe", "smartscreen.exe",
];
const SECURITY: &[&str] = &[
    "msmpeng.exe", "nissrv.exe", "mpdefendercoreservice.exe", "securityhealthservice.exe",
    "securityhealthsystray.exe", "avp.exe", "avpui.exe", "adguardsvc.exe", "adguard.exe",
    "mbamservice.exe", "mbam.exe", "ekrn.exe", "bdservicehost.exe", "ccsvchst.exe",
    "glasswire.exe", "gwctlsrv.exe",
];
const VIRTUAL_MACHINE: &[&str] = &[
    "vmmem", "vmmemcmzygote", "vmmemwsl", "vmwp.exe", "vmcompute.exe", "virtualboxvm.exe",
    "vboxsvc.exe", "vmware-vmx.exe", "qemu-system-x86_64.exe",
];
const BROWSER: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe",
    "whale.exe", "arc.exe", "thorium.exe", "librewolf.exe", "waterfox.exe",
];
const WEBVIEW2: &str = "msedgewebview2.exe";

/// All processes of one application (one executable name).
#[derive(Clone)]
pub struct AppUsage {
    pub name: String,
    pub category: Category,
    pub count: usize,
    pub private: u64,
    pub working_set: u64,
    /// Process instances as `(pid, start time)`.
    pub instances: Vec<(u32, u64)>,
    /// Share of total CPU capacity, as in Task Manager. Set by the activity tracker.
    pub cpu_pct: f32,
    pub status: Status,
    /// Memory priority lowered by memwarden. Set by the priority governor.
    pub lowered: bool,
}

struct Group {
    app: AppUsage,
    web_engine: bool,
    all_services: bool,
}

/// Groups readable processes by executable name, sorted by private bytes.
/// WebView2 helpers are counted under the application that hosts them.
/// Also returns how many processes could not be read.
pub fn group(procs: &[ProcessInfo]) -> (Vec<AppUsage>, usize) {
    let by_pid: HashMap<u32, &ProcessInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut denied = 0usize;

    for p in procs {
        let Some(stats) = &p.stats else {
            denied += 1;
            continue;
        };
        let (name, via_webview2) = owner(p, &by_pid);
        let g = groups.entry(name.to_lowercase()).or_insert_with(|| Group {
            app: AppUsage {
                name: name.to_owned(),
                category: Category::Application,
                count: 0,
                private: 0,
                working_set: 0,
                instances: Vec::new(),
                cpu_pct: 0.0,
                status: Status::Active,
                lowered: false,
            },
            web_engine: false,
            all_services: true,
        });
        g.app.count += 1;
        g.app.private += stats.private;
        g.app.working_set += stats.working_set;
        g.app.instances.push((p.pid, stats.created));
        g.web_engine |= stats.chromium_child || via_webview2;
        g.all_services &= p.session == Some(0);
    }

    let mut apps: Vec<AppUsage> = groups
        .into_values()
        .map(|mut g| {
            g.app.category = categorize(&g.app.name, g.web_engine, g.all_services);
            g.app
        })
        .collect();
    apps.sort_by(|a, b| b.private.cmp(&a.private));
    (apps, denied)
}

/// Name of the application a process belongs to; for WebView2 helpers this is
/// the first non-WebView2 ancestor. The flag tells whether attribution happened.
fn owner<'a>(p: &'a ProcessInfo, by_pid: &HashMap<u32, &'a ProcessInfo>) -> (&'a str, bool) {
    if !p.name.eq_ignore_ascii_case(WEBVIEW2) {
        return (&p.name, false);
    }
    let mut cur = p;
    for _ in 0..16 {
        let Some(&parent) = by_pid.get(&cur.parent_pid) else { break };
        // A parent started after its child means the PID was reused by another process.
        let reused = match (&parent.stats, &cur.stats) {
            (Some(ps), Some(cs)) => ps.created > cs.created,
            _ => false,
        };
        if parent.pid == cur.pid || reused {
            break;
        }
        if !parent.name.eq_ignore_ascii_case(WEBVIEW2) {
            return (&parent.name, true);
        }
        cur = parent;
    }
    (&p.name, false)
}

fn categorize(name: &str, web_engine: bool, all_services: bool) -> Category {
    let n = name.to_ascii_lowercase();
    let is = |list: &[&str]| list.contains(&n.as_str());
    if is(SYSTEM) {
        Category::System
    } else if is(SECURITY) {
        Category::Security
    } else if is(VIRTUAL_MACHINE) {
        Category::VirtualMachine
    } else if is(BROWSER) {
        Category::Browser
    } else if all_services {
        Category::Service
    } else if web_engine {
        Category::WebApp
    } else {
        Category::Application
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcStats;

    fn proc(pid: u32, parent: u32, name: &str, session: u32, created: u64, chromium: bool) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent_pid: parent,
            name: name.into(),
            session: Some(session),
            stats: Some(ProcStats {
                private: 100,
                working_set: 50,
                created,
                cpu_time: 0,
                io_bytes: 0,
                chromium_child: chromium,
            }),
        }
    }

    fn find<'a>(apps: &'a [AppUsage], name: &str) -> Option<&'a AppUsage> {
        apps.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    #[test]
    fn webview2_is_counted_under_its_host() {
        let procs = [
            proc(10, 1, "ms-teams.exe", 1, 100, false),
            proc(11, 10, "msedgewebview2.exe", 1, 110, false),
            proc(12, 11, "msedgewebview2.exe", 1, 120, false),
        ];
        let (apps, denied) = group(&procs);
        assert_eq!(denied, 0);
        assert!(find(&apps, WEBVIEW2).is_none());
        let teams = find(&apps, "ms-teams.exe").unwrap();
        assert_eq!(teams.count, 3);
        assert_eq!(teams.private, 300);
        assert_eq!(teams.category, Category::WebApp);
    }

    #[test]
    fn reused_parent_pid_is_not_an_owner() {
        // The "parent" started after the child, so PID 10 was reused.
        let procs = [proc(10, 1, "notepad.exe", 1, 500, false), proc(11, 10, "msedgewebview2.exe", 1, 110, false)];
        let (apps, _) = group(&procs);
        assert!(find(&apps, WEBVIEW2).is_some());
        assert_eq!(find(&apps, "notepad.exe").unwrap().count, 1);
    }

    #[test]
    fn electron_children_make_a_web_app() {
        let procs = [proc(20, 1, "Claude.exe", 1, 1, false), proc(21, 20, "claude.exe", 1, 2, true)];
        let (apps, _) = group(&procs);
        assert_eq!(apps.len(), 1, "names are grouped case-insensitively");
        assert_eq!(apps[0].category, Category::WebApp);
    }

    #[test]
    fn categories_follow_lists_then_session() {
        assert_eq!(categorize("svchost.exe", false, true), Category::System);
        assert_eq!(categorize("MsMpEng.exe", false, true), Category::Security);
        assert_eq!(categorize("vmmem", false, false), Category::VirtualMachine);
        assert_eq!(categorize("chrome.exe", true, false), Category::Browser);
        assert_eq!(categorize("someservice.exe", false, true), Category::Service);
        assert_eq!(categorize("notepad.exe", false, false), Category::Application);
    }

    #[test]
    fn only_user_facing_categories_are_idle_candidates() {
        let candidates: Vec<_> = Category::ALL.into_iter().filter(|c| c.idle_candidate()).collect();
        assert_eq!(candidates, [Category::Application, Category::WebApp, Category::Browser]);
    }

    #[test]
    fn unreadable_processes_are_counted() {
        let mut p = proc(30, 1, "protected.exe", 0, 1, false);
        p.stats = None;
        let (apps, denied) = group(&[p]);
        assert!(apps.is_empty());
        assert_eq!(denied, 1);
    }
}
