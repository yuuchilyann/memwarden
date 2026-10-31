//! Page file settings and sizes (milestone M7). Read-only: settings are
//! changed in Windows' own Performance Options dialog, which needs
//! administrator rights and validates the input.

use std::io;
use std::path::PathBuf;
use std::process::Command;

use windows::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, GetDiskFreeSpaceExW, WIN32_FIND_DATAW};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_MULTI_SZ, RegGetValueW};
use windows::core::HSTRING;

const MEMORY_MANAGEMENT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management";
const MB: u64 = 1024 * 1024;
const GB: u64 = MB * 1024;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// Windows picks the size and grows the file when commit nears the limit.
    System,
    /// Fixed sizes chosen by the user, in bytes.
    Custom { initial: u64, max: u64 },
}

#[derive(Clone, Debug)]
pub struct PageFile {
    pub mode: Mode,
    /// Current size in bytes.
    pub current: u64,
    /// Largest size it can reach without changing settings, in bytes.
    pub max: u64,
}

/// All page files; empty when there is none.
#[derive(Clone, Debug, Default)]
pub struct PageFiles(pub Vec<PageFile>);

impl PageFiles {
    pub fn current(&self) -> u64 {
        self.0.iter().map(|f| f.current).sum()
    }

    pub fn max(&self) -> u64 {
        self.0.iter().map(|f| f.max).sum()
    }

    /// Whether any page file is managed by Windows, so the commit limit can grow on its own.
    pub fn system_managed(&self) -> bool {
        self.0.iter().any(|f| f.mode == Mode::System)
    }

    /// Commit limit once every page file has reached its maximum.
    pub fn potential_limit(&self, commit_limit: u64) -> u64 {
        commit_limit.saturating_sub(self.current()) + self.max()
    }
}

/// Reads the page file settings; `phys_total` is installed RAM in bytes.
pub fn query(phys_total: u64) -> Option<PageFiles> {
    let entries: Vec<Entry> = read_multi_sz("PagingFiles")?.iter().filter_map(|s| parse_entry(s)).collect();
    let existing: Vec<char> = read_multi_sz("ExistingPageFiles")
        .unwrap_or_default()
        .iter()
        .filter_map(|s| existing_drive(s))
        .collect();
    let files = resolve(&entries, &existing)
        .into_iter()
        .map(|(drive, mode)| {
            let current = file_size(drive).unwrap_or(0);
            let (volume, free) = volume_space(drive).unwrap_or((0, 0));
            let max = match mode {
                Mode::System => system_max(phys_total, volume, current, free),
                Mode::Custom { max, .. } => max.max(current),
            };
            PageFile { mode, current, max }
        })
        .collect();
    Some(PageFiles(files))
}

/// Opens Windows' Performance Options dialog, whose Advanced tab holds the
/// virtual memory settings. Nothing is passed or changed by memwarden.
pub fn open_settings() -> io::Result<()> {
    let root = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    Command::new(root.join("System32").join("SystemPropertiesPerformance.exe")).spawn().map(drop)
}

/// One `PagingFiles` line: `C:\pagefile.sys 0 0` (managed by Windows),
/// `C:\pagefile.sys 4096 8192` (custom, MB) or `?:\pagefile.sys` (Windows manages all drives).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Entry {
    /// `None` for `?:`.
    drive: Option<char>,
    mode: Mode,
}

fn parse_entry(line: &str) -> Option<Entry> {
    let mut parts = line.split_whitespace();
    let path = parts.next()?;
    let mut chars = path.chars();
    let (letter, colon) = (chars.next()?, chars.next()?);
    if colon != ':' {
        return None;
    }
    let drive = match letter {
        '?' => None,
        c if c.is_ascii_alphabetic() => Some(c.to_ascii_uppercase()),
        _ => return None,
    };
    let sizes: Vec<u64> = parts.filter_map(|p| p.parse().ok()).collect();
    let mode = match sizes[..] {
        [initial, max] if max > 0 => Mode::Custom { initial: initial * MB, max: max * MB },
        _ => Mode::System,
    };
    Some(Entry { drive, mode })
}

/// Drive letter of an `ExistingPageFiles` line such as `\??\C:\pagefile.sys`.
fn existing_drive(line: &str) -> Option<char> {
    let rest = line.strip_prefix(r"\??\").unwrap_or(line);
    let mut chars = rest.chars();
    let (letter, colon) = (chars.next()?, chars.next()?);
    (letter.is_ascii_alphabetic() && colon == ':').then(|| letter.to_ascii_uppercase())
}

/// Page files per drive. `?:` stands for the files Windows currently keeps.
fn resolve(entries: &[Entry], existing: &[char]) -> Vec<(char, Mode)> {
    let mut out: Vec<(char, Mode)> = Vec::new();
    for entry in entries {
        let drives = entry.drive.map_or_else(|| existing.to_vec(), |d| vec![d]);
        for drive in drives {
            if !out.iter().any(|&(d, _)| d == drive) {
                out.push((drive, entry.mode));
            }
        }
    }
    out
}

/// Largest size of a page file managed by Windows: three times RAM or 4 GB,
/// whichever is larger, but at most an eighth of the volume and no more
/// than the free space allows.
fn system_max(phys_total: u64, volume: u64, current: u64, free: u64) -> u64 {
    let cap = (phys_total * 3).max(4 * GB).min(volume / 8);
    cap.min(current + free).max(current)
}

fn read_multi_sz(value: &str) -> Option<Vec<String>> {
    let (subkey, value) = (HSTRING::from(MEMORY_MANAGEMENT), HSTRING::from(value));
    let mut len = 0u32;
    unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, &subkey, &value, RRF_RT_REG_MULTI_SZ, None, None, Some(&mut len)) }
        .ok()
        .ok()?;
    let mut buf = vec![0u16; (len as usize).div_ceil(2)];
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            RRF_RT_REG_MULTI_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    }
    .ok()
    .ok()?;
    let used = &buf[..(len as usize / 2).min(buf.len())];
    Some(used.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect())
}

/// Size of `X:\pagefile.sys`; the file is in use, so it is looked up rather than opened.
fn file_size(drive: char) -> Option<u64> {
    let path = HSTRING::from(format!(r"{drive}:\pagefile.sys"));
    let mut data = WIN32_FIND_DATAW::default();
    let handle = unsafe { FindFirstFileW(&path, &mut data) }.ok()?;
    unsafe {
        let _ = FindClose(handle);
    }
    Some((data.nFileSizeHigh as u64) << 32 | data.nFileSizeLow as u64)
}

/// `(volume size, free bytes)` of the drive.
fn volume_space(drive: char) -> Option<(u64, u64)> {
    let root = HSTRING::from(format!(r"{drive}:\"));
    let (mut free, mut total) = (0u64, 0u64);
    unsafe { GetDiskFreeSpaceExW(&root, Some(&mut free), Some(&mut total), None) }.ok()?;
    Some((total, free))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paging_files_lines() {
        assert_eq!(parse_entry(r"?:\pagefile.sys"), Some(Entry { drive: None, mode: Mode::System }));
        assert_eq!(parse_entry(r"c:\pagefile.sys 0 0"), Some(Entry { drive: Some('C'), mode: Mode::System }));
        assert_eq!(parse_entry(r"D:\pagefile.sys"), Some(Entry { drive: Some('D'), mode: Mode::System }));
        assert_eq!(
            parse_entry(r"C:\pagefile.sys 4096 8192"),
            Some(Entry { drive: Some('C'), mode: Mode::Custom { initial: 4096 * MB, max: 8192 * MB } })
        );
        assert_eq!(parse_entry(""), None);
        assert_eq!(parse_entry("pagefile.sys"), None);
    }

    #[test]
    fn question_mark_means_the_existing_files() {
        let auto = parse_entry(r"?:\pagefile.sys").unwrap();
        assert_eq!(resolve(&[auto], &['C']), vec![('C', Mode::System)]);
        let custom = parse_entry(r"D:\pagefile.sys 1024 2048").unwrap();
        let both = resolve(&[custom, parse_entry(r"C:\pagefile.sys 0 0").unwrap()], &['C', 'D']);
        assert_eq!(both, vec![('D', custom.mode), ('C', Mode::System)]);
        assert!(resolve(&[], &['C']).is_empty(), "no PagingFiles entry: no page file");
        assert_eq!(existing_drive(r"\??\c:\pagefile.sys"), Some('C'));
    }

    #[test]
    fn system_managed_maximum() {
        // 32 GB RAM on a large, mostly free volume: three times RAM.
        assert_eq!(system_max(32 * GB, 1400 * GB, 19 * GB, 200 * GB), 96 * GB);
        // Small RAM: at least 4 GB.
        assert_eq!(system_max(GB, 1000 * GB, GB, 500 * GB), 4 * GB);
        // An eighth of the volume.
        assert_eq!(system_max(32 * GB, 256 * GB, 10 * GB, 200 * GB), 32 * GB);
        // Free space limits growth, but never below the current size.
        assert_eq!(system_max(32 * GB, 1400 * GB, 19 * GB, 5 * GB), 24 * GB);
        assert_eq!(system_max(32 * GB, 80 * GB, 19 * GB, 0), 19 * GB);
    }

    #[test]
    fn potential_limit_adds_growth_room() {
        let files = PageFiles(vec![PageFile { mode: Mode::System, current: 19 * GB, max: 96 * GB }]);
        assert_eq!(files.potential_limit(49 * GB), 126 * GB);
        assert_eq!(PageFiles::default().potential_limit(31 * GB), 31 * GB);
    }

    /// Reads this machine's settings; prints them for a manual look.
    #[test]
    fn reads_this_machine() {
        let files = query(32 * GB).expect("Memory Management key is readable");
        for f in &files.0 {
            assert!(f.max >= f.current);
            println!("{f:?}");
        }
    }
}
