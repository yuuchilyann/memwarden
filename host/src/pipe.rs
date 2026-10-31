//! Client end of memwarden's named pipe.
//!
//! The pipe is opened for overlapped I/O: on a synchronous handle Windows
//! serializes all I/O, so a read blocked waiting for memwarden would also
//! block writes coming from the extension. Each call waits on its own event.

use std::io::{self, Read, Write};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING, ReadFile, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::CreateEventW;
use windows::core::HSTRING;

pub struct PipeClient(HANDLE);

// The handle is only used through overlapped calls with per-call state.
unsafe impl Send for PipeClient {}
unsafe impl Sync for PipeClient {}

impl PipeClient {
    /// Connects to an existing pipe; fails when memwarden is not running.
    pub fn connect(name: &str) -> io::Result<Self> {
        let handle = unsafe {
            CreateFileW(
                &HSTRING::from(name),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                // Identification level only: memwarden may check who we are,
                // but cannot act as this user.
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        }?;
        Ok(Self(handle))
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
            // memwarden closed its end: report end of stream.
            Err(e) if e.code() == ERROR_BROKEN_PIPE.to_hresult() => Ok(0),
            Err(e) => Err(e.into()),
        }
    }
}

impl Read for &PipeClient {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.wait(|o| unsafe { ReadFile(self.0, Some(buf), None, Some(o)) })
    }
}

impl Write for &PipeClient {
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

impl Drop for PipeClient {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
