//! Finds processes with active audio sessions (playing or recording).
//!
//! Apps that make or capture sound — music, video, calls — are in use even
//! when they are in the background and use little CPU, so they must never be
//! reported idle, have their priority lowered, or be trimmed.

use std::collections::HashSet;

use windows::Win32::Media::Audio::{
    AudioSessionStateActive, DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    MMDeviceEnumerator, eAll,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::core::Interface;

/// Initializes COM for the calling thread. Call once per thread before `active_pids`.
pub fn init_thread() {
    // S_FALSE (already initialized) is fine; failure only disables audio detection.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// PIDs owning an active audio session on any playback or recording device.
/// Returns an empty set if Core Audio is unavailable.
pub fn active_pids() -> HashSet<u32> {
    let mut pids = HashSet::new();
    let _ = collect(&mut pids);
    pids
}

fn collect(pids: &mut HashSet<u32>) -> windows::core::Result<()> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE)?;
        for i in 0..devices.GetCount()? {
            let Ok(manager) = devices.Item(i)?.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let sessions = manager.GetSessionEnumerator()?;
            for j in 0..sessions.GetCount()? {
                let Ok(session) = sessions.GetSession(j) else { continue };
                if session.GetState().ok() != Some(AudioSessionStateActive) {
                    continue;
                }
                // Fails for sessions spanning several processes; those are skipped.
                if let Ok(pid) = session.cast::<IAudioSessionControl2>().and_then(|s| s.GetProcessId())
                    && pid != 0
                {
                    pids.insert(pid);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diagnostic against the live system: `cargo test audio -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn prints_active_audio_sessions() {
        init_thread();
        println!("active audio PIDs: {:?}", active_pids());
    }
}
