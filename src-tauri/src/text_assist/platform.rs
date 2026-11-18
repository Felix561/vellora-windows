//! Windows implementation of [`JobPlatform`] (spec sections 4.1-4.4, M1 findings).

use std::time::Duration;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

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
/// After input during a request, the keyboard and mouse must be idle this long before the
/// selection is re-copied (no key or button held, activity counter unchanged).
const QUIET_PERIOD: Duration = Duration::from_millis(300);
/// Give up waiting for idle input after this long; the result is kept instead.
const QUIET_TIMEOUT: Duration = Duration::from_millis(2_500);

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

/// Console and terminal hosts: Ctrl+C there may interrupt the running program, and Ctrl+V
/// types at the prompt instead of replacing the selection.
pub(crate) fn is_terminal(exe_name: &str) -> bool {
    const TERMINALS: &[&str] = &[
        "windowsterminal.exe",
        "openconsole.exe",
        "conhost.exe",
        "cmd.exe",
        "powershell.exe",
        "pwsh.exe",
        "wt.exe",
        "wezterm-gui.exe",
        "alacritty.exe",
        "mintty.exe",
        "putty.exe",
        "kitty.exe",
        "tabby.exe",
        "hyper.exe",
    ];
    TERMINALS
        .iter()
        .any(|terminal| exe_name.eq_ignore_ascii_case(terminal))
}

/// True when `pid` runs elevated (as administrator) and Vellora does not. Windows then drops
/// Vellora's injected Ctrl+C/Ctrl+V (UIPI) without reporting an error. A token that cannot be
/// opened from a non-elevated process is treated as elevated.
fn target_is_elevated(pid: u32) -> bool {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    fn elevated(process: windows::Win32::Foundation::HANDLE) -> Option<bool> {
        unsafe {
            let mut token = windows::Win32::Foundation::HANDLE::default();
            OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
            let mut info = TOKEN_ELEVATION::default();
            let mut size = 0u32;
            let result = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut info as *mut _ as *mut core::ffi::c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            let _ = CloseHandle(token);
            result.ok()?;
            Some(info.TokenIsElevated != 0)
        }
    }

    if elevated(unsafe { GetCurrentProcess() }) == Some(true) {
        return false;
    }
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let target = elevated(process);
        let _ = CloseHandle(process);
        target.unwrap_or(true)
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
        let exe = exe_name(pid);
        if exe.as_deref().is_some_and(is_terminal) {
            return Err(Failure::new("terminal", "Not in terminals"));
        }
        let (anchor, focus) = geometry::inspect_target(hwnd);
        if let Some(failure) = refuse_target(hwnd, pid, &focus) {
            return Err(failure);
        }
        Ok(TargetSnapshot {
            hwnd,
            pid,
            exe_name: exe,
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
            if target_is_elevated(target.pid) {
                return Err(Failure::new("elevated_app", "App runs as admin"));
            }
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
        original: &str,
        text: &str,
        restore_delay: Duration,
    ) -> Result<(), Failure> {
        if !clip::wait_modifiers_released(MODIFIER_RELEASE_TIMEOUT) {
            return Err(Failure::new("modifiers_held", "Keys were held"));
        }
        let _input = input_lock::blocking_lock();
        let focus = geometry::focus_info(target.hwnd);
        match verify_unchanged(target, foreground().0, input_activity::current(), &focus) {
            Ok(()) => {}
            // Input during the request (a scroll, a stray modifier, a click, or typing). It may
            // not have touched the selection, so check the selection itself once input is idle.
            Err(failure) if failure.code == "user_activity" => {
                confirm_selection_after_input(target, original)?
            }
            Err(failure) => return Err(failure),
        }
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

/// Waits until Enter and Esc are released, so a key that closed the panel cannot auto-repeat
/// into the editor once it has focus again.
fn wait_panel_keys_released(timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while [VK_RETURN, VK_ESCAPE]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(key.0 as i32) } < 0)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Gives the foreground back to the target window. Vellora may do this while its panel owns
/// the foreground (M1: back within 0-3 ms, same focused control). Returns false on failure.
pub fn return_focus(target: &TargetSnapshot) -> bool {
    wait_panel_keys_released(Duration::from_millis(600));
    if foreground().0 != target.hwnd {
        let _ = unsafe { SetForegroundWindow(hwnd(target.hwnd)) };
    }
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    while std::time::Instant::now() < deadline {
        if foreground().0 == target.hwnd {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Pure re-copy decision for the panel (spec 4.3): the editor must still hold exactly the text
/// that was captured when the panel opened.
pub(crate) fn recopy_matches(copied: Option<&str>, original: &str) -> Result<(), Failure> {
    match copied {
        None => Err(Failure::new("selection_lost", "Selection lost")),
        Some(text) if text == original => Ok(()),
        Some(_) => Err(Failure::new("selection_changed", "Selection changed")),
    }
}

/// [`recopy_matches`] plus the VS Code marker: with an empty selection VS Code copies the whole
/// line, which must never count as the original selection.
pub(crate) fn recopy_verdict(
    copied: &clip::CopiedSelection,
    original: &str,
) -> Result<(), Failure> {
    match copied.vscode {
        VsCodeCopy::EmptySelection => Err(Failure::new("selection_lost", "Selection lost")),
        VsCodeCopy::MultiCursor => Err(Failure::new("selection_changed", "Selection changed")),
        VsCodeCopy::Unknown | VsCodeCopy::Selection => {
            recopy_matches(copied.text.as_deref(), original)
        }
    }
}

/// True while any key or mouse button is physically down (as Windows reports it now).
fn any_input_held() -> bool {
    (0x01u16..=0xFE)
        // VK_PACKET carries injected Unicode characters and is never "held".
        .filter(|vk| *vk != 0xE7)
        .any(|vk| unsafe { GetAsyncKeyState(vk as i32) } < 0)
}

/// Waits until no key or button is held and the activity counter stayed unchanged for
/// `quiet`. Returns the counter value at that point, or `None` after `timeout`.
pub(crate) fn wait_for_idle_input_with(
    quiet: Duration,
    timeout: Duration,
    mut activity: impl FnMut() -> u64,
    mut held: impl FnMut() -> bool,
    mut now: impl FnMut() -> std::time::Instant,
    mut sleep: impl FnMut(Duration),
) -> Option<u64> {
    let started = now();
    let mut last = activity();
    let mut quiet_since = started;
    loop {
        let at = now();
        let current = activity();
        if current != last || held() {
            last = current;
            quiet_since = at;
        } else if at.duration_since(quiet_since) >= quiet {
            return Some(current);
        }
        if at.duration_since(started) >= timeout {
            return None;
        }
        sleep(Duration::from_millis(20));
    }
}

/// After input during the request: once input is idle, the window and focused control must be
/// the same and a fresh copy of the selection must equal the original exactly. Otherwise the
/// result is kept and nothing is pasted. The clipboard is restored after the check.
fn confirm_selection_after_input(target: &TargetSnapshot, original: &str) -> Result<(), Failure> {
    let refused = || Failure::new("user_activity", "Input during the request");
    let Some(quiet) = wait_for_idle_input_with(
        QUIET_PERIOD,
        QUIET_TIMEOUT,
        input_activity::current,
        any_input_held,
        std::time::Instant::now,
        std::thread::sleep,
    ) else {
        return Err(refused());
    };
    let focus = geometry::focus_info(target.hwnd);
    let settled_target = TargetSnapshot {
        activity: quiet,
        ..target.clone()
    };
    verify_unchanged(&settled_target, foreground().0, quiet, &focus)?;
    let saved = clip::ClipboardSnapshot::capture()
        .map_err(|_| Failure::new("clipboard_busy", "The clipboard is busy"))?;
    let before = clip::sequence();
    clip::send_copy().map_err(|_| Failure::new("input_blocked", "Copy blocked"))?;
    let Some(settled) = clip::wait_for_copy(before, CopyWait::default()) else {
        // Nothing was copied: the selection is gone. The clipboard is untouched.
        return Err(Failure::new("selection_lost", "Selection lost"));
    };
    let verdict = match clip::read_copied_selection() {
        Ok(copied) => recopy_verdict(&copied, original),
        Err(_) => Err(Failure::new("clipboard_busy", "The clipboard is busy")),
    };
    if clip::sequence() == settled {
        let _ = saved.restore();
    }
    verdict?;
    // Input during the re-copy itself could have changed the selection again.
    if input_activity::current() != quiet {
        return Err(refused());
    }
    Ok(())
}

impl WindowsPlatform {
    /// Rewrite panel Apply (spec 4.3): focus hand-back, re-copy check, then paste. Any failure
    /// leaves the editor untouched; the caller keeps the result.
    pub fn apply_from_panel(
        &self,
        target: &TargetSnapshot,
        original: &str,
        text: &str,
        restore_delay: Duration,
    ) -> Result<(), Failure> {
        if !return_focus(target) {
            return Err(Failure::new("target_changed", "Window changed"));
        }
        // The focused control must come back too (it is restored asynchronously).
        let deadline = std::time::Instant::now() + Duration::from_millis(400);
        loop {
            let focus = geometry::focus_info(target.hwnd);
            if focus.focus_hwnd == target.focus.focus_hwnd {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(Failure::new("focus_changed", "Focus changed"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !clip::wait_modifiers_released(MODIFIER_RELEASE_TIMEOUT) {
            return Err(Failure::new("modifiers_held", "Release keys"));
        }
        let _input = input_lock::blocking_lock();
        let saved = clip::ClipboardSnapshot::capture()
            .map_err(|_| Failure::new("clipboard_busy", "Clipboard busy"))?;
        let before = clip::sequence();
        clip::send_copy().map_err(|_| Failure::new("input_blocked", "Copy blocked"))?;
        let Some(settled) = clip::wait_for_copy(before, CopyWait::default()) else {
            return Err(Failure::new("selection_lost", "Selection lost"));
        };
        let copied = clip::read_copied_selection();
        let verdict = match &copied {
            Ok(copied) => recopy_verdict(copied, original),
            Err(_) => Err(Failure::new("clipboard_busy", "Clipboard busy")),
        };
        if let Err(failure) = verdict {
            if clip::sequence() == settled {
                let _ = saved.restore();
            }
            return Err(failure);
        }
        let written = clip::write_text_excluded(text)
            .map_err(|_| Failure::new("clipboard_busy", "Clipboard busy"))?;
        if clip::send_paste().is_err() {
            let _ = saved.restore();
            return Err(Failure::new("paste_failed", "Paste blocked"));
        }
        std::thread::sleep(restore_delay.max(MIN_RESTORE_DELAY));
        if clip::sequence() == written {
            let _ = saved.restore();
        }
        Ok(())
    }
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
    fn recopy_must_match_the_captured_text_exactly() {
        let original = "a b\r\n";
        assert!(recopy_matches(Some(original), original).is_ok());
        assert_eq!(
            recopy_matches(Some("a b"), original).unwrap_err().code,
            "selection_changed"
        );
        assert_eq!(
            recopy_matches(None, "a").unwrap_err().code,
            "selection_lost"
        );
    }

    #[test]
    fn terminals_are_recognized_case_insensitively() {
        assert!(is_terminal("WindowsTerminal.exe"));
        assert!(is_terminal("pwsh.exe"));
        assert!(is_terminal("CMD.EXE"));
        assert!(!is_terminal("Code.exe"));
        assert!(!is_terminal("T3 Code (Beta).exe"));
        assert!(!is_terminal("notepad.exe"));
    }

    #[test]
    fn this_process_is_not_an_elevated_target() {
        assert!(!target_is_elevated(std::process::id()));
    }

    #[test]
    fn recopy_verdict_rejects_vscode_line_copies() {
        let copied = |text: &str, vscode| clip::CopiedSelection {
            text: Some(text.to_string()),
            vscode,
        };
        assert!(recopy_verdict(&copied("a b", VsCodeCopy::Selection), "a b").is_ok());
        assert!(recopy_verdict(&copied("a b", VsCodeCopy::Unknown), "a b").is_ok());
        // An empty VS Code selection copies the whole line, even when it equals the original.
        assert_eq!(
            recopy_verdict(&copied("a b", VsCodeCopy::EmptySelection), "a b")
                .unwrap_err()
                .code,
            "selection_lost"
        );
        assert_eq!(
            recopy_verdict(&copied("a b", VsCodeCopy::MultiCursor), "a b")
                .unwrap_err()
                .code,
            "selection_changed"
        );
    }

    /// Simulated clock for [`wait_for_idle_input_with`]: each sleep advances it.
    fn idle_wait(
        activity_at: impl Fn(u64) -> u64,
        held_at: impl Fn(u64) -> bool,
    ) -> (Option<u64>, u64) {
        let start = std::time::Instant::now();
        let elapsed = std::cell::Cell::new(0u64);
        let result = wait_for_idle_input_with(
            Duration::from_millis(300),
            Duration::from_millis(2_500),
            || activity_at(elapsed.get()),
            || held_at(elapsed.get()),
            || start + Duration::from_millis(elapsed.get()),
            |step| elapsed.set(elapsed.get() + step.as_millis() as u64),
        );
        (result, elapsed.get())
    }

    #[test]
    fn idle_wait_returns_once_input_is_quiet() {
        // Already idle: returns the current counter after the quiet period.
        let (value, ms) = idle_wait(|_| 5, |_| false);
        assert_eq!(value, Some(5));
        assert!((300..400).contains(&ms), "{ms}");
        // Activity until 500 ms: the quiet period starts after the last change.
        let (value, ms) = idle_wait(|t| if t < 500 { t / 100 } else { 9 }, |_| false);
        assert_eq!(value, Some(9));
        assert!((800..900).contains(&ms), "{ms}");
        // A key held until 1 s also restarts the quiet period.
        let (value, ms) = idle_wait(|_| 1, |t| t < 1_000);
        assert_eq!(value, Some(1));
        assert!((1_250..1_400).contains(&ms), "{ms}");
    }

    #[test]
    fn idle_wait_gives_up_while_input_continues() {
        let (value, ms) = idle_wait(|t| t / 100, |_| false);
        assert_eq!(value, None);
        assert!((2_500..2_600).contains(&ms), "{ms}");
        let (value, _) = idle_wait(|_| 0, |_| true);
        assert_eq!(value, None);
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
