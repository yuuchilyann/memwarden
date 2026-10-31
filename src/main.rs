//! memwarden - Windows memory analysis (read-only in this version).

#[cfg(not(windows))]
compile_error!("memwarden currently supports Windows only");

mod process;
mod system;

use std::collections::HashMap;

const MB: f64 = 1024.0 * 1024.0;
const GB: f64 = MB * 1024.0;

fn main() -> windows::core::Result<()> {
    let sys = system::query()?;
    println!("== System memory ==");
    println!(
        "Physical : {:>6.1} / {:>6.1} GB in use ({}%)",
        (sys.phys_total - sys.phys_avail) as f64 / GB,
        sys.phys_total as f64 / GB,
        sys.load_percent
    );
    println!(
        "Commit   : {:>6.1} / {:>6.1} GB ({:.0}%)",
        sys.commit_total as f64 / GB,
        sys.commit_limit as f64 / GB,
        sys.commit_total as f64 * 100.0 / sys.commit_limit as f64
    );
    println!(
        "Cache    : {:>6.1} GB   Kernel paged/nonpaged: {:.2} / {:.2} GB",
        sys.system_cache as f64 / GB,
        sys.kernel_paged as f64 / GB,
        sys.kernel_nonpaged as f64 / GB
    );

    // Group by executable name so multi-process apps (Chrome, Electron) show as one line.
    let mut groups: HashMap<String, (usize, u64, u64)> = HashMap::new();
    let mut denied = 0usize;
    for p in process::list()? {
        match p.mem {
            Some(m) => {
                let g = groups.entry(p.name).or_default();
                g.0 += 1;
                g.1 += m.private;
                g.2 += m.working_set;
            }
            None => denied += 1,
        }
    }
    let mut rows: Vec<_> = groups.into_iter().collect();
    rows.sort_by(|a, b| b.1.1.cmp(&a.1.1));

    println!("\n== Top processes by private bytes ==");
    println!("{:<32} {:>5} {:>11} {:>11}", "Name", "Count", "Private MB", "WS MB");
    for (name, (count, private, ws)) in rows.iter().take(25) {
        println!(
            "{:<32} {:>5} {:>11.0} {:>11.0}",
            name,
            count,
            *private as f64 / MB,
            *ws as f64 / MB
        );
    }
    if denied > 0 {
        println!("\n({denied} processes not readable; run as administrator for full coverage)");
    }
    Ok(())
}
