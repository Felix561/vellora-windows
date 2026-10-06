use std::{thread, time::Duration};

use anyhow::Context;
use windows::Win32::System::DataExchange::{CountClipboardFormats, GetClipboardSequenceNumber};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub fn copy_text(text: &str) -> anyhow::Result<()> {
    copy_text_with_sequence(text).map(|_| ())
}

pub fn copy_text_with_sequence(text: &str) -> anyhow::Result<u32> {
    let mut clipboard = arboard::Clipboard::new().context("Could not open clipboard")?;
    clipboard
        .set_text(text.to_string())
        .context("Could not write transcript to clipboard")?;
    Ok(unsafe { GetClipboardSequenceNumber() })
}

// A None snapshot represents an empty clipboard; non-text content is never
// overwritten by automatic paste when clipboard preservation was requested.
pub fn capture_text() -> anyhow::Result<Option<String>> {
    let mut clipboard = arboard::Clipboard::new().context("Could not read the clipboard")?;
    match clipboard.get_text() {
        Ok(text) => Ok(Some(text)),
        Err(_) if unsafe { CountClipboardFormats() } == 0 => Ok(None),
        Err(_) => anyhow::bail!("The clipboard contains non-text content. Auto-paste was skipped to preserve it; copy the transcript manually when ready."),
    }
}

pub fn restore_text_if_unchanged(
    text: Option<String>,
    expected_sequence: u32,
) -> anyhow::Result<()> {
    if unsafe { GetClipboardSequenceNumber() } != expected_sequence {
        // A newer clipboard belongs to the user or another app. Leave it alone.
        return Ok(());
    }
    if let Some(text) = text {
        copy_text(&text)
    } else {
        arboard::Clipboard::new()?
            .clear()
            .context("Could not restore an empty clipboard")
    }
}

pub fn foreground_target() -> isize {
    let window = unsafe { GetForegroundWindow() };
    let mut owner = 0;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut owner));
    }
    if owner == 0 || owner == std::process::id() {
        0
    } else {
        window.0 as isize
    }
}

pub fn paste_clipboard() -> anyhow::Result<()> {
    paste_into(None)
}

pub fn paste_into(expected_target: Option<isize>) -> anyhow::Result<()> {
    thread::sleep(Duration::from_millis(80));
    let target = foreground_target();
    if target == 0 || expected_target.is_some_and(|expected| expected != target) {
        anyhow::bail!("The target window changed. Transcript is ready; focus the destination and copy/paste it manually.");
    }
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        anyhow::bail!("Windows SendInput did not send the full paste shortcut");
    }
    Ok(())
}

fn key_input(key: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
