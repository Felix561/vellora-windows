//! Help uses structured, non-secret metadata and fixed destinations only.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

use crate::types::{AppHealthSnapshot, DictationState, StartupPhase};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneStatus {
    pub available: bool,
    pub device_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportInfo {
    pub version: &'static str,
    pub platform: &'static str,
    pub architecture: &'static str,
    pub startup_phase: StartupPhase,
    pub ready: bool,
    pub single_instance: bool,
    pub key_configured: bool,
    pub microphone_available: bool,
    pub dictation_state: DictationState,
}

impl SupportInfo {
    pub fn new(
        health: AppHealthSnapshot,
        dictation_state: DictationState,
        key_configured: bool,
        microphone_available: bool,
    ) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            platform: "windows",
            architecture: std::env::consts::ARCH,
            startup_phase: health.startup_phase,
            ready: health.ready,
            single_instance: health.single_instance,
            key_configured,
            microphone_available,
            dictation_state,
        }
    }
}

static MICROPHONE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

struct MicrophoneCheckGuard;

impl Drop for MicrophoneCheckGuard {
    fn drop(&mut self) {
        MICROPHONE_CHECK_RUNNING.store(false, Ordering::Release);
    }
}

pub async fn check_microphone() -> Result<MicrophoneStatus, String> {
    MICROPHONE_CHECK_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "A microphone check is already running. Please wait.".to_string())?;
    // A timeout cannot cancel a blocked driver. The worker keeps its guard until
    // it exits, preventing repeated clicks from creating unbounded workers.
    let guard = MicrophoneCheckGuard;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(device) = cpal::default_host().default_input_device() else {
            return Ok(MicrophoneStatus {
                available: false,
                device_name: None,
            });
        };
        device.default_input_config().map_err(|_| {
            "Windows could not read the default microphone configuration. Check your input device and microphone privacy settings.".to_string()
        })?;
        Ok(MicrophoneStatus {
            available: true,
            device_name: device.name().ok(),
        })
    });
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .map_err(|_| {
            "The microphone check timed out. Check your Windows input device.".to_string()
        })?
        .map_err(|_| "The microphone check could not finish. Please try again.".to_string())?
}

pub fn help_destination(id: &str) -> Result<&'static str, String> {
    match id {
        "repository" => Ok("https://github.com/Felix561/vellora-windows"),
        "readme" => Ok("https://github.com/Felix561/vellora-windows#readme"),
        "troubleshooting" => {
            Ok("https://github.com/Felix561/vellora-windows/blob/main/docs/TROUBLESHOOTING.md")
        }
        "privacy" => Ok("https://github.com/Felix561/vellora-windows/blob/main/docs/PRIVACY.md"),
        "license" => Ok("https://github.com/Felix561/vellora-windows/blob/main/LICENSE"),
        "openaiKeys" => Ok("https://platform.openai.com/api-keys"),
        "openaiBilling" => Ok("https://platform.openai.com/settings/organization/billing/overview"),
        "windowsMicrophone" => Ok("ms-settings:privacy-microphone"),
        _ => Err("Unknown help link.".to_string()),
    }
}

pub fn open_help_link(id: &str) -> Result<(), String> {
    let destination = help_destination(id)?;
    #[cfg(windows)]
    {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let destination: Vec<u16> = destination.encode_utf16().chain(Some(0)).collect();
        // The main-window command runs on Tauri's Windows UI thread. No shell
        // strings, executable paths, or caller-provided URLs reach this API.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(destination.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize <= 32 {
            return Err("Windows could not open this help link. Check your default browser or open Windows Settings manually.".to_string());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = destination;
        Err("Vellora help links are supported on Windows only.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_links_reject_arbitrary_urls_and_commands() {
        for id in [
            "https://example.com",
            "file:///C:/secret",
            "cmd.exe",
            "repository & calc",
            "",
        ] {
            assert!(help_destination(id).is_err());
        }
        assert_eq!(
            help_destination("windowsMicrophone").unwrap(),
            "ms-settings:privacy-microphone"
        );
        assert!(help_destination("troubleshooting")
            .unwrap()
            .ends_with("/docs/TROUBLESHOOTING.md"));
        for id in [
            "repository",
            "readme",
            "troubleshooting",
            "privacy",
            "license",
        ] {
            let destination = help_destination(id).unwrap();
            let suffix = destination
                .strip_prefix("https://github.com/Felix561/vellora-windows")
                .expect("Public help must use the source repository");
            assert!(suffix.is_empty() || suffix.starts_with('/') || suffix.starts_with('#'));
        }
    }

    #[test]
    fn support_metadata_does_not_serialize_freeform_health_details() {
        let health = AppHealthSnapshot {
            ready: true,
            startup_phase: StartupPhase::Ready,
            last_error: None,
            active_operation: Some("private transcript or C:\\Users\\example".to_string()),
            backend_started_at: "private timestamp".to_string(),
            single_instance: true,
        };
        let value =
            serde_json::to_value(SupportInfo::new(health, DictationState::Idle, true, false))
                .unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 9);
        assert!(!value.to_string().contains("private"));
        assert!(!object.contains_key("lastError"));
        assert!(!object.contains_key("deviceName"));
        assert_eq!(object["keyConfigured"], true);
    }
}
