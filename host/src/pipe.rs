//! Both ends of the named pipe between memwarden-host and memwarden.
//!
//! The pipe is opened for overlapped I/O: on a synchronous handle Windows
//! serializes all I/O, so a read blocked waiting for the other side would
//! also block writes from another thread. Each call waits on its own event.

use std::io::{self, Read, Write};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
    HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::core::HSTRING;

use crate::frame::MAX_FRAME;

pub struct Pipe(HANDLE);

// The handle is only used through overlapped calls with per-call state.
unsafe impl Send for Pipe {}
unsafe impl Sync for Pipe {}

impl Pipe {
    /// Client side: connects to an existing pipe; fails when memwarden is not running.
    pub fn connect(name: &str) -> io::Result<Self> {
        let handle = unsafe {
            CreateFileW(
                &HSTRING::from(name),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                // Identification level only: the server may check who we are,
                // but cannot act as this user.
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        }?;
        Ok(Self(handle))
    }

    /// Server side: creates one pipe instance that only the current user can
    /// open, from this machine only. With `first`, creation fails if another
    /// process already owns the name, so a squatter is never trusted.
    pub fn create(name: &str, first: bool) -> io::Result<Self> {
        // Protected DACL granting the current user only; the medium integrity
        // label lets a non-elevated host connect to an elevated memwarden.
        let sddl = format!("D:P(A;;GA;;;{})S:(ML;;NW;;;ME)", crate::current_user_sid()?);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(sddl), SDDL_REVISION_1, &mut sd, None)
        }?;
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
        if first {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let handle = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(name),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                MAX_FRAME as u32,
                MAX_FRAME as u32,
                0,
                Some(&attrs),
            )
        };
        let error = io::Error::last_os_error();
        unsafe {
            LocalFree(Some(HLOCAL(sd.0)));
        }
        if handle.is_invalid() { Err(error) } else { Ok(Self(handle)) }
    }

    /// Server side: waits until a client connects to this instance.
    pub fn accept(&self) -> io::Result<()> {
        match self.wait(|o| unsafe { ConnectNamedPipe(self.0, Some(o)) }) {
            Ok(_) => Ok(()),
            // The client connected before we started waiting.
            // (Errors converted from windows::core::Error carry the HRESULT.)
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED.to_hresult().0) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Server side: PID of the connected client process.
    pub fn client_pid(&self) -> io::Result<u32> {
        let mut pid = 0;
        unsafe { GetNamedPipeClientProcessId(self.0, &mut pid) }?;
        Ok(pid)
    }

    /// Runs one overlapped operation and waits for it to finish.
    fn wait(&self, start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>) -> io::Result<usize> {
        let event = unsafe { CreateEventW(None, true, false, None) }?;
        let mut overlapped = OVERLAPPED { hEvent: event, ..Default::default() };
        let mut done = 0u32;
        let result = match start(&mut overlapped) {
            Err(e) if e.code() != ERROR_IO_PENDING.to_hresult() => Err(e),
            _ => unsafe { GetOverlappedResult(self.0, &overlapped, &mut done, true) },
        };
        unsafe {
            let _ = CloseHandle(event);
        }
        match result {
            Ok(()) => Ok(done as usize),
            // The other side closed its end: report end of stream.
            Err(e) if e.code() == ERROR_BROKEN_PIPE.to_hresult() => Ok(0),
            Err(e) => Err(e.into()),
        }
    }
}

impl Read for &Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.wait(|o| unsafe { ReadFile(self.0, Some(buf), None, Some(o)) })
    }
}

impl Write for &Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.wait(|o| unsafe { WriteFile(self.0, Some(buf), None, Some(o)) })? {
            0 if !buf.is_empty() => Err(io::ErrorKind::BrokenPipe.into()),
            n => Ok(n),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{read_frame, write_frame};

    #[test]
    fn server_and_client_exchange_frames() {
        let name = format!(r"\\.\pipe\memwarden-test-{}", std::process::id());
        let server = Pipe::create(&name, true).unwrap();
        // A second owner of the same name is refused.
        assert!(Pipe::create(&name, true).is_err());

        let client = std::thread::spawn({
            let name = name.clone();
            move || {
                let pipe = Pipe::connect(&name).unwrap();
                write_frame(&mut &pipe, b"ping").unwrap();
                read_frame(&mut &pipe).unwrap().unwrap()
            }
        });
        server.accept().unwrap();
        assert_eq!(server.client_pid().unwrap(), std::process::id());
        assert_eq!(read_frame(&mut &server).unwrap().unwrap(), b"ping");
        write_frame(&mut &server, b"pong").unwrap();
        assert_eq!(client.join().unwrap(), b"pong");
        // The client dropped its end: clean end of stream.
        assert!(read_frame(&mut &server).unwrap().is_none());
    }
}
