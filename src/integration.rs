//! Browser integration switch (milestone M6).
//!
//! Chrome and Edge on Windows find a Native Messaging host only through the
//! registry, so enabling the integration writes, for the current user only:
//! - `%LOCALAPPDATA%\memwarden\com.memwarden.host.json`, the host manifest
//!   naming `memwarden-host.exe` next to `memwarden.exe` and allowing only the
//!   memwarden extension;
//! - `HKCU\Software\{Google\Chrome,Microsoft\Edge}\NativeMessagingHosts\com.memwarden.host`,
//!   pointing at that manifest.
//!
//! Done only when the user turns the switch on; turning it off removes all of
//! it. Nothing else here touches the registry.
//!
//! When memwarden runs inside an MSIX container (for example started from the
//! Claude desktop app) these writes are virtualized and browsers do not see them.

use std::io;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyW, RegGetValueW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

const HOST_NAME: &str = "com.memwarden.host";
/// Extensions allowed to start the host. The unpacked extension's ID is fixed
/// by the key in extension/manifest.json; store IDs are added at release (M7).
const EXTENSION_IDS: &[&str] = &["jmngjfeoiaihgaabjklldhfmnaejnkga"];
const BROWSER_KEYS: [&str; 2] = [
    r"Software\Google\Chrome\NativeMessagingHosts\com.memwarden.host",
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.memwarden.host",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Enabled,
    Disabled,
    /// Registered, but for another copy of memwarden or only partly; turning
    /// the switch on points it at this copy.
    Elsewhere,
}

pub struct Integration {
    /// `None` when memwarden-host.exe is not next to this executable.
    host_exe: Option<PathBuf>,
    manifest: Option<PathBuf>,
    keys: Vec<String>,
}

impl Integration {
    pub fn new() -> Self {
        let host_exe = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("memwarden-host.exe")))
            .filter(|path| path.is_file());
        let manifest = std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("memwarden").join(format!("{HOST_NAME}.json")));
        Self { host_exe, manifest, keys: BROWSER_KEYS.iter().map(|k| k.to_string()).collect() }
    }

    /// Whether the switch can be turned on at all.
    pub fn available(&self) -> bool {
        self.host_exe.is_some() && self.manifest.is_some()
    }

    pub fn state(&self) -> State {
        let values: Vec<Option<String>> = self.keys.iter().map(|k| read_default(k)).collect();
        if values.iter().all(Option::is_none) {
            return State::Disabled;
        }
        let ours = self.manifest.as_ref().map(|m| m.to_string_lossy().into_owned());
        let current = match (&self.host_exe, &self.manifest) {
            (Some(exe), Some(manifest)) => std::fs::read(manifest).is_ok_and(|text| text == manifest_json(exe)),
            _ => false,
        };
        if current && values.iter().all(|v| v.is_some() && *v == ours) { State::Enabled } else { State::Elsewhere }
    }

    pub fn enable(&self) -> io::Result<()> {
        let (Some(exe), Some(manifest)) = (&self.host_exe, &self.manifest) else {
            return Err(io::ErrorKind::NotFound.into());
        };
        if let Some(dir) = manifest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(manifest, manifest_json(exe))?;
        for key in &self.keys {
            write_default(key, &manifest.to_string_lossy())?;
        }
        Ok(())
    }

    /// Removes this copy's registration. Keys pointing at another copy's
    /// manifest are left alone.
    pub fn disable(&self) -> io::Result<()> {
        let ours = self.manifest.as_ref().map(|m| m.to_string_lossy().into_owned());
        for key in &self.keys {
            if read_default(key).is_some() && read_default(key) == ours {
                delete_key(key)?;
            }
        }
        if let Some(manifest) = &self.manifest {
            match std::fs::remove_file(manifest) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }

    /// Where the manifest is written, for the explanation in the UI.
    pub fn manifest_path(&self) -> Option<&Path> {
        self.manifest.as_deref()
    }
}

fn manifest_json(host_exe: &Path) -> Vec<u8> {
    let origins: Vec<String> = EXTENSION_IDS.iter().map(|id| format!("chrome-extension://{id}/")).collect();
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "memwarden native messaging host",
        "path": host_exe,
        "type": "stdio",
        "allowed_origins": origins,
    });
    // serde_json writes UTF-8 without a byte order mark, which browsers require.
    serde_json::to_vec_pretty(&manifest).expect("manifest serializes")
}

// ---------- registry (HKCU only) ----------

fn read_default(subkey: &str) -> Option<String> {
    let subkey = HSTRING::from(subkey);
    let mut len = 0u32;
    unsafe { RegGetValueW(HKEY_CURRENT_USER, &subkey, PCWSTR::null(), RRF_RT_REG_SZ, None, None, Some(&mut len)) }
        .ok()
        .ok()?;
    let mut buf = vec![0u16; (len as usize).div_ceil(2)];
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    }
    .ok()
    .ok()?;
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

fn write_default(subkey: &str, value: &str) -> io::Result<()> {
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    }
    .ok()?;
    let data: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    let result = unsafe { RegSetValueExW(key, PCWSTR::null(), None, REG_SZ, Some(&data)) }.ok();
    unsafe {
        let _ = RegCloseKey(key);
    }
    Ok(result?)
}

fn delete_key(subkey: &str) -> io::Result<()> {
    match unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(subkey)) } {
        e if e == ERROR_FILE_NOT_FOUND => Ok(()),
        e => Ok(e.ok()?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::RegDeleteTreeW;

    /// Uses a throwaway key and temp directory, never the browsers' keys.
    struct Sandbox {
        root: String,
        dir: PathBuf,
    }

    impl Sandbox {
        fn new(name: &str) -> (Self, Integration) {
            let root = format!(r"Software\memwarden-test-{}-{name}", std::process::id());
            let dir = std::env::temp_dir().join(format!("memwarden-test-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let integration = Integration {
                host_exe: Some(std::env::current_exe().unwrap()),
                manifest: Some(dir.join("host.json")),
                keys: vec![format!(r"{root}\Chrome"), format!(r"{root}\Edge")],
            };
            (Self { root, dir }, integration)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(&self.root));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(&self.root));
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn enable_and_disable_round_trip() {
        let (_sandbox, i) = Sandbox::new("round-trip");
        assert_eq!(i.state(), State::Disabled);

        i.enable().unwrap();
        assert_eq!(i.state(), State::Enabled);
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(i.manifest_path().unwrap()).unwrap()).unwrap();
        assert_eq!(manifest["name"], HOST_NAME);
        assert_eq!(manifest["type"], "stdio");
        assert_eq!(manifest["path"], std::env::current_exe().unwrap().to_str().unwrap());
        assert_eq!(manifest["allowed_origins"][0], "chrome-extension://jmngjfeoiaihgaabjklldhfmnaejnkga/");

        i.disable().unwrap();
        assert_eq!(i.state(), State::Disabled);
        assert!(!i.manifest_path().unwrap().exists());
        // Disabling twice is harmless.
        i.disable().unwrap();
    }

    #[test]
    fn another_copys_registration_is_not_removed() {
        let (_sandbox, i) = Sandbox::new("elsewhere");
        write_default(&i.keys[0], r"D:\other\com.memwarden.host.json").unwrap();
        assert_eq!(i.state(), State::Elsewhere);

        i.disable().unwrap();
        assert_eq!(read_default(&i.keys[0]).as_deref(), Some(r"D:\other\com.memwarden.host.json"));

        // Turning the switch on takes the registration over.
        i.enable().unwrap();
        assert_eq!(i.state(), State::Enabled);
    }

    #[test]
    fn partial_registration_is_not_enabled() {
        let (_sandbox, i) = Sandbox::new("partial");
        i.enable().unwrap();
        delete_key(&i.keys[1]).unwrap();
        assert_eq!(i.state(), State::Elsewhere);
    }
}
