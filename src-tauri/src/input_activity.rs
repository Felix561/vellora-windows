// Several items are consumed only once the Text Assist job orchestrator is wired
// (Task G). Remove this allow when that wiring lands.
#![allow(dead_code)]

//! Counter of real (not Vellora-injected) user input.
//!
//! Quick Polish only replaces text when no key press, click or wheel happened
//! between capture and paste. The keyboard hook in `hotkey.rs` records key-downs;
//! a low-level mouse hook installed on the same thread records clicks and wheel
//! turns. Input that Vellora injects carries [`VELLORA_INPUT_SIGNATURE`] in
//! `dwExtraInfo` and is never counted.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        },
        WindowsAndMessaging::{
            CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK,
            MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEHWHEEL,
            WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
        },
    },
};

/// Marks keyboard/mouse input injected by Vellora (`dwExtraInfo`). "VELL".
pub const VELLORA_INPUT_SIGNATURE: usize = 0x5645_4C4C;

static ACTIVITY: AtomicU64 = AtomicU64::new(0);
static MOUSE_TRACKING: AtomicBool = AtomicBool::new(false);

/// Records a real key-down (not Vellora-injected).
pub fn record_key_down() {
    ACTIVITY.fetch_add(1, Ordering::SeqCst);
}

/// Records a real mouse button-down or wheel turn.
pub fn record_pointer() {
    ACTIVITY.fetch_add(1, Ordering::SeqCst);
}

/// Monotonically increasing activity counter.
pub fn current() -> u64 {
    ACTIVITY.load(Ordering::SeqCst)
}

/// True while the low-level mouse hook is installed. Without it only key-downs
/// count as activity.
pub fn mouse_tracking_active() -> bool {
    MOUSE_TRACKING.load(Ordering::SeqCst)
}

/// True for WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN, WM_XBUTTONDOWN, WM_MOUSEWHEEL,
/// WM_MOUSEHWHEEL. Mouse moves and button-ups are not activity.
pub fn is_counted_mouse_message(message: u32) -> bool {
    matches!(
        message,
        WM_LBUTTONDOWN
            | WM_RBUTTONDOWN
            | WM_MBUTTONDOWN
            | WM_XBUTTONDOWN
            | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL
    )
}

/// Builds a signed keyboard INPUT for SendInput (virtual key, key-up flag).
pub fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: VELLORA_INPUT_SIGNATURE,
            },
        },
    }
}

/// Installs WH_MOUSE_LL on the calling thread, which must run a message loop. Sets
/// mouse_tracking_active on success. The callback only increments the counter.
pub(crate) fn install_mouse_hook() -> windows::core::Result<HHOOK> {
    let hook =
        unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), HINSTANCE::default(), 0)? };
    MOUSE_TRACKING.store(true, Ordering::SeqCst);
    Ok(hook)
}

/// Removes a hook from [`install_mouse_hook`] and clears mouse_tracking_active.
pub(crate) fn uninstall_mouse_hook(hook: HHOOK) {
    MOUSE_TRACKING.store(false, Ordering::SeqCst);
    let _ = unsafe { UnhookWindowsHookEx(hook) };
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && is_counted_mouse_message(wparam.0 as u32) {
        let info = *(lparam.0 as *const MSLLHOOKSTRUCT);
        if info.dwExtraInfo != VELLORA_INPUT_SIGNATURE {
            record_pointer();
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{VK_CONTROL, VK_V},
        WindowsAndMessaging::{
            WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONUP, WM_XBUTTONUP,
        },
    };

    #[test]
    fn counter_increments_on_key_down_and_pointer() {
        // Other tests may bump the shared counter concurrently, so only
        // monotonic growth is asserted.
        let before = current();
        record_key_down();
        let after_key = current();
        assert!(after_key > before);
        record_pointer();
        assert!(current() > after_key);
    }

    #[test]
    fn counted_mouse_messages_are_button_downs_and_wheels_only() {
        for message in [
            WM_LBUTTONDOWN,
            WM_RBUTTONDOWN,
            WM_MBUTTONDOWN,
            WM_XBUTTONDOWN,
            WM_MOUSEWHEEL,
            WM_MOUSEHWHEEL,
        ] {
            assert!(is_counted_mouse_message(message), "{message:#x}");
        }
        for message in [
            WM_MOUSEMOVE,
            WM_LBUTTONUP,
            WM_RBUTTONUP,
            WM_MBUTTONUP,
            WM_XBUTTONUP,
            0,
        ] {
            assert!(!is_counted_mouse_message(message), "{message:#x}");
        }
    }

    #[test]
    fn key_input_is_signed_and_sets_key_up_flag() {
        let down = key_input(VK_CONTROL, false);
        let up = key_input(VK_V, true);
        assert_eq!(down.r#type, INPUT_KEYBOARD);
        unsafe {
            assert_eq!(down.Anonymous.ki.wVk, VK_CONTROL);
            assert_eq!(down.Anonymous.ki.dwExtraInfo, VELLORA_INPUT_SIGNATURE);
            assert_eq!(down.Anonymous.ki.dwFlags.0 & KEYEVENTF_KEYUP.0, 0);
            assert_eq!(up.Anonymous.ki.wVk, VK_V);
            assert_eq!(up.Anonymous.ki.dwExtraInfo, VELLORA_INPUT_SIGNATURE);
            assert_eq!(
                up.Anonymous.ki.dwFlags.0 & KEYEVENTF_KEYUP.0,
                KEYEVENTF_KEYUP.0
            );
        }
    }

    #[test]
    fn signature_spells_vell() {
        assert_eq!((VELLORA_INPUT_SIGNATURE as u32).to_be_bytes(), *b"VELL");
    }
}
