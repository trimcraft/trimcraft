//! "Audio volume" actions through Windows Core Audio: the master volume of the default output
//! device (`IAudioEndpointVolume`), or the volume of a program's audio sessions
//! (`ISimpleAudioVolume`) on every active output device, found by executable name.
//!
//! The calls are quick and run on the calling thread (the action thread).

use crate::elevation;
use trimcraft_core::audio::{VolumeCommand, VolumeTarget};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eMultimedia, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::core::Interface;

/// COM on the current thread, once.
fn com() {
    thread_local! {
        static INIT: () = {
            // SAFETY: initialises COM for this thread; an existing (other) mode is fine.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        };
    }
    INIT.with(|_| {});
}

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    com();
    // SAFETY: plain COM object creation.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .map_err(|e| format!("Windows audio is not available: {e}"))
}

/// Runs `command` on `target`. Returns what was done, e.g. "Discord.exe: 2 sessions".
pub fn apply(target: &VolumeTarget, command: VolumeCommand) -> Result<String, String> {
    let devices = enumerator()?;
    match target {
        VolumeTarget::Master => {
            // SAFETY: COM calls on objects created above.
            unsafe {
                let device = devices
                    .GetDefaultAudioEndpoint(eRender, eMultimedia)
                    .map_err(|e| format!("no default output device: {e}"))?;
                let volume: IAudioEndpointVolume = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|e| e.to_string())?;
                let level = volume
                    .GetMasterVolumeLevelScalar()
                    .map_err(|e| e.to_string())?;
                let muted = volume.GetMute().map_err(|e| e.to_string())?.as_bool();
                let change = command.apply(level, muted);
                if let Some(level) = change.level {
                    volume
                        .SetMasterVolumeLevelScalar(level, std::ptr::null())
                        .map_err(|e| e.to_string())?;
                }
                if let Some(mute) = change.mute {
                    volume
                        .SetMute(mute, std::ptr::null())
                        .map_err(|e| e.to_string())?;
                }
                Ok(result_text("master", change))
            }
        }
        VolumeTarget::App { process } => {
            let sessions = sessions(&devices)?;
            let matching: Vec<&(String, ISimpleAudioVolume)> = sessions
                .iter()
                .filter(|(exe, _)| exe.eq_ignore_ascii_case(process))
                .collect();
            if matching.is_empty() {
                return Err(format!(
                    "{process} has no audio session (is it running and playing sound?)"
                ));
            }
            // All of the program's sessions follow the first one, so toggling keeps them in step.
            // SAFETY: COM calls on session objects found above.
            unsafe {
                let first = &matching[0].1;
                let level = first.GetMasterVolume().map_err(|e| e.to_string())?;
                let muted = first.GetMute().map_err(|e| e.to_string())?.as_bool();
                let change = command.apply(level, muted);
                for (_, volume) in &matching {
                    if let Some(level) = change.level {
                        volume
                            .SetMasterVolume(level, std::ptr::null())
                            .map_err(|e| e.to_string())?;
                    }
                    if let Some(mute) = change.mute {
                        volume
                            .SetMute(mute, std::ptr::null())
                            .map_err(|e| e.to_string())?;
                    }
                }
                Ok(format!(
                    "{} ({} session{})",
                    result_text(process, change),
                    matching.len(),
                    if matching.len() == 1 { "" } else { "s" }
                ))
            }
        }
    }
}

fn result_text(who: &str, change: trimcraft_core::audio::VolumeChange) -> String {
    match (change.level, change.mute) {
        (Some(level), _) => format!("{who} at {:.0}%", level * 100.0),
        (None, Some(true)) => format!("{who} muted"),
        (None, Some(false)) => format!("{who} unmuted"),
        (None, None) => who.to_owned(),
    }
}

/// Every audio session on every active output device, with its program's executable name.
fn sessions(devices: &IMMDeviceEnumerator) -> Result<Vec<(String, ISimpleAudioVolume)>, String> {
    let mut out = Vec::new();
    // SAFETY: COM calls on objects created here.
    unsafe {
        let all = devices
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("cannot list output devices: {e}"))?;
        for i in 0..all.GetCount().map_err(|e| e.to_string())? {
            let Ok(device) = all.Item(i) else { continue };
            if let Err(e) = device_sessions(&device, &mut out) {
                log::debug!("volume: skipping an output device: {e}");
            }
        }
    }
    Ok(out)
}

/// # Safety
/// COM must be initialised on this thread.
unsafe fn device_sessions(
    device: &IMMDevice,
    out: &mut Vec<(String, ISimpleAudioVolume)>,
) -> windows::core::Result<()> {
    // SAFETY: COM calls on a valid device.
    unsafe {
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let list = manager.GetSessionEnumerator()?;
        for i in 0..list.GetCount()? {
            let Ok(control) = list.GetSession(i) else {
                continue;
            };
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let Ok(pid) = control2.GetProcessId() else {
                continue;
            };
            // Process 0 is the system sounds session.
            if pid == 0 {
                continue;
            }
            let Some(exe) = elevation::exe_name(pid) else {
                continue;
            };
            if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                out.push((exe, volume));
            }
        }
    }
    Ok(())
}

/// Executable names of programs with an audio session, for the picker.
pub fn session_programs() -> Result<Vec<String>, String> {
    let devices = enumerator()?;
    let mut names: Vec<String> = sessions(&devices)?
        .into_iter()
        .map(|(exe, _)| exe)
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    Ok(names)
}

#[cfg(test)]
mod tests {
    /// Lists programs with audio sessions; changes nothing.
    #[test]
    #[ignore = "needs Windows audio"]
    fn lists_audio_programs() {
        println!("{:#?}", super::session_programs().unwrap());
    }
}
