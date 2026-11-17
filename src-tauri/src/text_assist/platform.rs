//! Windows implementation of [`JobPlatform`] (spec sections 4.1-4.4, M1 findings).

use std::time::Duration;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::{
    geometry::{self, Anchor, FocusInfo},
    job::{Failure, JobPlatform},
};
use crate::{
    clipboard_snapshot::{self as clip, CopyWait, VsCodeCopy},
    input_activity, input_lock,
};

/// How long to wait for the user to release Shift/Ctrl/Alt/Win before injecting keys.
const MODIFIER_RELEASE_TIMEOUT: Duration = Duration::from_millis(400);
/// M1: VS Code and TeXstudio read the clipboard asynchronously; 25 ms sufficed, keep a floor.
const MIN_RESTORE_DELAY: Duration = Duration::from_millis(50);

/// Everything recorded when the chord fired (spec 4.1).
#[derive(Debug, Clone, PartialEq)]
pub struct TargetSnapshot {
    pub hwnd: isize,
    pub pid: u32,
    pub exe_name: Option<String>,
    pub focus: FocusInfo,
    pub activity: u64,
    pub anchor: Anchor,
}

#[derive(Debug, Default)]
pub struct WindowsPlatform;

fn foreground() -> (isize, u32) {
    let hwnd = unsafe { GetForegroundWindow() };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    (hwnd.0 as isize, pid)
}

/// Executable file name (no path) of a process, for history.
fn exe_name(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit(['\\', '/'])
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    }
}

/// Pure checks applied to a fresh snapshot (unit-tested).
pub(crate) fn refuse_target(hwnd: isize, pid: u32, focus: &FocusInfo) -> Option<Failure> {
    if hwnd == 0 || pid == 0 {
        return Some(Failure::new("no_target", "Click into text"));
    }
    if pid == std::process::id() {
        return Some(Failure::new("vellora_window", "Not in Vellora"));
    }
    if focus.is_password == Some(true) {
        return Some(Failure::new("password_field", "Password field"));
    }
    None
}

/// Pure checks on the copied selection (unit-tested).
pub(crate) fn check_copied(
    text: Option<String>,
    vscode: VsCodeCopy,
    max_chars: usize,
) -> Result<String, Failure> {
    match vscode {
        VsCodeCopy::EmptySelection => {
            return Err(Failure::new("vscode_empty_selection", "Select text first"))
        }
        VsCodeCopy::MultiCursor => {
            return Err(Failure::new("vscode_multicursor", "One cursor only"))
        }
        VsCodeCopy::Unknown | VsCodeCopy::Selection => {}
    }
    let Some(text) = text else {
        return Err(Failure::new("no_text", "No plain text"));
    };
    if text.trim().is_empty() {
        return Err(Failure::new("empty_selection", "Select text first"));
    }
    let count = text.chars().count();
    if count > max_chars {
        return Err(Failure::new(
            "too_long",
            format!("Too long (max {max_chars})"),
        ));
    }
    Ok(text)
}

/// Pure verification right before pasting (spec 4.3, unit-tested).
pub(crate) fn verify_unchanged(
    target: &TargetSnapshot,
    foreground_hwnd: isize,
    activity: u64,
    focus: &FocusInfo,
) -> Result<(), Failure> {
    if foreground_hwnd != target.hwnd {
        return Err(Failure::new("target_changed", "Window changed"));
    }
    if activity != target.activity {
        return Err(Failure::new("user_activity", "Input during the request"));
    }
    if focus.focus_hwnd != target.focus.focus_hwnd {
        return Err(Failure::new("focus_changed", "Focus changed"));
    }
    if let (Some(before), Some(now)) = (&target.focus.uia_runtime_id, &focus.uia_runtime_id) {
        if before != now {
            return Err(Failure::new("focus_changed", "Focus changed"));
        }
    }
    Ok(())
}

impl JobPlatform for WindowsPlatform {
    type Target = TargetSnapshot;

    fn snapshot(&self) -> Result<TargetSnapshot, Failure> {
        // Read the activity counter first so nothing between snapshot and paste is missed.
        let activity = input_activity::current();
        let (hwnd, pid) = foreground();
        if let Some(failure) = refuse_target(hwnd, pid, &FocusInfo::default()) {
            return Err(failure);
        }
        let (anchor, focus) = geometry::inspect_target(hwnd);
        if let Some(failure) = refuse_target(hwnd, pid, &focus) {
            return Err(failure);
        }
        Ok(TargetSnapshot {
            hwnd,
            pid,
            exe_name: exe_name(pid),
            focus,
            activity,
            anchor,
        })
    }

    fn capture(&self, target: &TargetSnapshot, max_chars: usize) -> Result<String, Failure> {
        if !clip::wait_modifiers_released(MODIFIER_RELEASE_TIMEOUT) {
            return Err(Failure::new("modifiers_held", "Release keys"));
        }
        let _input = input_lock::blocking_lock();
        if foreground().0 != target.hwnd {
            return Err(Failure::new("target_changed", "Window changed"));
        }
        let saved = clip::ClipboardSnapshot::capture()
            .map_err(|_| Failure::new("clipboard_busy", "Clipboard busy"))?;
        let before = clip::sequence();
        clip::send_copy().map_err(|_| Failure::new("input_blocked", "Copy blocked"))?;
        let Some(settled) = clip::wait_for_copy(before, CopyWait::default()) else {
            // Nothing was copied, so the clipboard is untouched.
            return Err(Failure::new("no_selection", "Select text first"));
        };
        let copied = clip::read_copied_selection();
        // Restore only if nobody changed the clipboard after the injected copy.
        if clip::sequence() == settled {
            let _ = saved.restore();
        }
        let copied = copied.map_err(|_| Failure::new("clipboard_busy", "Clipboard busy"))?;
        check_copied(copied.text, copied.vscode, max_chars)
    }

    fn replace(
        &self,
        target: &TargetSnapshot,
        text: &str,
        restore_delay: Duration,
    ) -> Result<(), Failure> {
        if !clip::wait_modifiers_released(MODIFIER_RELEASE_TIMEOUT) {
            return Err(Failure::new("modifiers_held", "Keys were held"));
        }
        let _input = input_lock::blocking_lock();
        let focus = geometry::focus_info(target.hwnd);
        verify_unchanged(target, foreground().0, input_activity::current(), &focus)?;
        let saved = clip::ClipboardSnapshot::capture()
            .map_err(|_| Failure::new("clipboard_busy", "The clipboard is busy"))?;
        let written = clip::write_text_excluded(text)
            .map_err(|_| Failure::new("clipboard_busy", "The clipboard is busy"))?;
        if clip::send_paste().is_err() {
            let _ = saved.restore();
            return Err(Failure::new("paste_failed", "Windows blocked the paste"));
        }
        std::thread::sleep(restore_delay.max(MIN_RESTORE_DELAY));
        if clip::sequence() == written {
            let _ = saved.restore();
        }
        Ok(())
    }

    fn target_app(&self, target: &TargetSnapshot) -> Option<String> {
        target.exe_name.clone()
    }
}

/// HWND helper for callers holding an `isize`.
pub(crate) fn hwnd(value: isize) -> HWND {
    HWND(value as *mut core::ffi::c_void)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_assist::geometry::{AnchorKind, RectPx};

    fn target() -> TargetSnapshot {
        TargetSnapshot {
            hwnd: 100,
            pid: 4,
            exe_name: Some("notepad.exe".to_string()),
            focus: FocusInfo {
                focus_hwnd: 200,
                uia_runtime_id: Some(vec![42, 7]),
                is_password: Some(false),
            },
            activity: 9,
            anchor: Anchor {
                kind: AnchorKind::Pointer,
                rect: RectPx {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            },
        }
    }

    #[test]
    fn refuses_vellora_password_and_missing_targets() {
        let normal = FocusInfo::default();
        assert_eq!(refuse_target(0, 5, &normal).unwrap().code, "no_target");
        assert_eq!(
            refuse_target(10, std::process::id(), &normal).unwrap().code,
            "vellora_window"
        );
        let password = FocusInfo {
            is_password: Some(true),
            ..FocusInfo::default()
        };
        assert_eq!(
            refuse_target(10, 5, &password).unwrap().code,
            "password_field"
        );
        assert!(refuse_target(10, 5, &normal).is_none());
    }

    #[test]
    fn copied_selection_rules() {
        assert_eq!(
            check_copied(Some("x".into()), VsCodeCopy::EmptySelection, 10)
                .unwrap_err()
                .code,
            "vscode_empty_selection"
        );
        assert_eq!(
            check_copied(Some("x".into()), VsCodeCopy::MultiCursor, 10)
                .unwrap_err()
                .code,
            "vscode_multicursor"
        );
        assert_eq!(
            check_copied(None, VsCodeCopy::Unknown, 10)
                .unwrap_err()
                .code,
            "no_text"
        );
        assert_eq!(
            check_copied(Some(" \r\n\t".into()), VsCodeCopy::Unknown, 10)
                .unwrap_err()
                .code,
            "empty_selection"
        );
        assert_eq!(
            check_copied(Some("ääääää".into()), VsCodeCopy::Selection, 5)
                .unwrap_err()
                .code,
            "too_long"
        );
        // The limit counts characters, not bytes.
        assert_eq!(
            check_copied(Some("ääääa".into()), VsCodeCopy::Selection, 5).unwrap(),
            "ääääa"
        );
    }

    #[test]
    fn verification_requires_same_window_focus_and_no_activity() {
        let target = target();
        let same = target.focus.clone();
        assert!(verify_unchanged(&target, 100, 9, &same).is_ok());
        assert_eq!(
            verify_unchanged(&target, 101, 9, &same).unwrap_err().code,
            "target_changed"
        );
        assert_eq!(
            verify_unchanged(&target, 100, 10, &same).unwrap_err().code,
            "user_activity"
        );
        let other_control = FocusInfo {
            focus_hwnd: 201,
            ..same.clone()
        };
        assert_eq!(
            verify_unchanged(&target, 100, 9, &other_control)
                .unwrap_err()
                .code,
            "focus_changed"
        );
        let other_element = FocusInfo {
            uia_runtime_id: Some(vec![42, 8]),
            ..same.clone()
        };
        assert_eq!(
            verify_unchanged(&target, 100, 9, &other_element)
                .unwrap_err()
                .code,
            "focus_changed"
        );
        // A missing runtime ID (Chromium, UIA failure) does not block.
        let unknown = FocusInfo {
            uia_runtime_id: None,
            ..same
        };
        assert!(verify_unchanged(&target, 100, 9, &unknown).is_ok());
    }
}
