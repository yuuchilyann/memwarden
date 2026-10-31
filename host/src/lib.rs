//! Shared between memwarden-host and memwarden's bridge: message framing,
//! the named pipe and identification of the browser behind a pipe client.

#[cfg(not(windows))]
compile_error!("memwarden-host currently supports Windows only");

pub mod browser;
pub mod frame;
pub mod pipe;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::PWSTR;

/// Protocol version carried in every message as `"v"`.
pub const PROTOCOL_VERSION: u32 = 1;

/// `\\.\pipe\memwarden-<user SID>`: one pipe per user, so another user's
/// memwarden or host never connects to it.
pub fn pipe_name() -> windows::core::Result<String> {
    Ok(format!(r"\\.\pipe\memwarden-{}", current_user_sid()?))
}

pub(crate) fn current_user_sid() -> windows::core::Result<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)? };
    let sid = token_user_sid(token);
    unsafe {
        let _ = CloseHandle(token);
    }
    sid
}

fn token_user_sid(token: HANDLE) -> windows::core::Result<String> {
    let mut len = 0u32;
    // The first call only reports the required size and always fails.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut len) };
    // u64 elements keep the buffer aligned for TOKEN_USER.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len)? };
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };

    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text)? };
    let sid = unsafe { text.to_string() };
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
    }
    sid.map_err(|_| windows::core::Error::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_name_contains_user_sid() {
        let name = pipe_name().unwrap();
        assert!(name.starts_with(r"\\.\pipe\memwarden-S-1-5-"), "{name}");
    }
}
