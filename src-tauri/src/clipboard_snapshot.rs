//! Clipboard operations for Text Assist (spec sections 4.2, 4.4, 4.5 and the M1 findings):
//! multi-format snapshot and restore, writes excluded from Win+V history and cloud clipboard,
//! VS Code copy markers, signed copy/paste keys and the sequence-number settle wait.
//!
//! Clipboard contents are never logged or included in errors.

// Consumed by the Text Assist capture/replace code (Task F); remove when wired.
#![allow(dead_code)]

use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
    VK_SHIFT, VK_V,
};

use crate::input_activity::key_input;

const CF_TEXT: u32 = 1;
const CF_BITMAP: u32 = 2;
const CF_METAFILEPICT: u32 = 3;
const CF_OEMTEXT: u32 = 7;
const CF_PALETTE: u32 = 9;
const CF_UNICODETEXT: u32 = 13;
const CF_ENHMETAFILE: u32 = 14;
const CF_OWNERDISPLAY: u32 = 0x80;
const CF_DSPBITMAP: u32 = 0x82;
const CF_DSPMETAFILEPICT: u32 = 0x83;
const CF_DSPENHMETAFILE: u32 = 0x8E;
/// Unassigned virtual key used as a "mask" so an Alt tap does not activate a menu bar.
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// Snapshot size cap. Larger formats are skipped and the snapshot is marked partial.
pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

/// Whether a clipboard format is copied into a snapshot. GDI-handle and owner-display formats
/// are skipped (Windows re-synthesizes them from their HGLOBAL equivalents), as are the
/// private/GDI-object ranges and ANSI/OEM text when Unicode text is present (synthesized).
pub fn keep_format(format: u32, present: &[u32]) -> bool {
    match format {
        CF_BITMAP | CF_METAFILEPICT | CF_PALETTE | CF_ENHMETAFILE | CF_OWNERDISPLAY
        | CF_DSPBITMAP | CF_DSPMETAFILEPICT | CF_DSPENHMETAFILE => false,
        CF_TEXT | CF_OEMTEXT => !present.contains(&CF_UNICODETEXT),
        0x0200..=0x03FF => false,
        _ => true,
    }
}

/// Current clipboard sequence number.
pub fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

struct OpenClipboardGuard;

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

/// Opens the clipboard with bounded retries (another app may briefly hold it).
fn open_clipboard() -> anyhow::Result<OpenClipboardGuard> {
    for _ in 0..30 {
        if unsafe { OpenClipboard(HWND::default()) }.is_ok() {
            return Ok(OpenClipboardGuard);
        }
        thread::sleep(Duration::from_millis(10));
    }
    anyhow::bail!("The clipboard is in use by another app")
}

/// A copy of every HGLOBAL-based clipboard format.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    formats: Vec<(u32, Vec<u8>)>,
    /// True when a format could not be copied (too large or not an HGLOBAL).
    pub partial: bool,
}

impl ClipboardSnapshot {
    pub fn capture() -> anyhow::Result<Self> {
        let _open = open_clipboard()?;
        let mut present = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            present.push(format);
        }
        let mut snapshot = Self::default();
        let mut total = 0usize;
        for &format in &present {
            if !keep_format(format, &present) {
                continue;
            }
            // Formats without data (for example an empty EnterpriseDataProtectionId) are skipped.
            let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
                continue;
            };
            let Some(bytes) =
                (unsafe { read_hglobal(HGLOBAL(handle.0), MAX_SNAPSHOT_BYTES - total) })
            else {
                snapshot.partial = true;
                continue;
            };
            total += bytes.len();
            snapshot.formats.push((format, bytes));
        }
        Ok(snapshot)
    }

    pub fn is_empty(&self) -> bool {
        self.formats.is_empty()
    }

    pub fn format_count(&self) -> usize {
        self.formats.len()
    }

    /// Replaces the clipboard with the snapshot. Non-empty restores carry the history/cloud
    /// exclusion formats so the restore does not appear as a new Win+V entry.
    pub fn restore(&self) -> anyhow::Result<u32> {
        {
            let _open = open_clipboard()?;
            unsafe { EmptyClipboard() }.context("Could not clear the clipboard")?;
            for (format, bytes) in &self.formats {
                put_bytes(*format, bytes)?;
            }
            if !self.formats.is_empty() {
                put_exclusion_formats()?;
            }
        }
        Ok(sequence())
    }
}

/// Copies an HGLOBAL's bytes. `None` when it is not lockable (not an HGLOBAL) or exceeds `limit`.
unsafe fn read_hglobal(handle: HGLOBAL, limit: usize) -> Option<Vec<u8>> {
    let size = GlobalSize(handle);
    if size == 0 || size > limit {
        return None;
    }
    let pointer = GlobalLock(handle);
    if pointer.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(pointer as *const u8, size).to_vec();
    // GlobalUnlock reports an "error" when the lock count reaches zero; that is expected.
    let _ = GlobalUnlock(handle);
    Some(bytes)
}

/// Must be called while the clipboard is open. Ownership of the memory passes to the system.
fn put_bytes(format: u32, bytes: &[u8]) -> anyhow::Result<()> {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))
            .context("Could not allocate clipboard memory")?;
        let pointer = GlobalLock(memory);
        if pointer.is_null() {
            let _ = GlobalFree(memory);
            anyhow::bail!("Could not lock clipboard memory");
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer as *mut u8, bytes.len());
        let _ = GlobalUnlock(memory);
        if SetClipboardData(format, HANDLE(memory.0)).is_err() {
            let _ = GlobalFree(memory);
            anyhow::bail!("Could not write clipboard data");
        }
    }
    Ok(())
}

/// Keeps Vellora's writes out of clipboard monitors, Win+V history and cloud clipboard.
fn put_exclusion_formats() -> anyhow::Result<()> {
    let zero = 0u32.to_le_bytes();
    for name in [
        w!("ExcludeClipboardContentFromMonitorProcessing"),
        w!("CanIncludeInClipboardHistory"),
        w!("CanUploadToCloudClipboard"),
    ] {
        let format = unsafe { RegisterClipboardFormatW(name) };
        if format != 0 {
            put_bytes(format, &zero)?;
        }
    }
    Ok(())
}

fn utf16z_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Writes plain text with the exclusion formats. Returns the sequence number after the write.
pub fn write_text_excluded(text: &str) -> anyhow::Result<u32> {
    {
        let _open = open_clipboard()?;
        unsafe { EmptyClipboard() }.context("Could not clear the clipboard")?;
        put_bytes(CF_UNICODETEXT, &utf16z_bytes(text))?;
        put_exclusion_formats()?;
    }
    Ok(sequence())
}

/// Decodes NUL-terminated UTF-16LE clipboard text. Invalid UTF-16 is rejected rather than
/// replaced, so a later paste can never alter characters the user selected.
pub fn decode_unicode_text(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16(&units).ok()
}

/// What VS Code reported about the copy that produced the clipboard contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VsCodeCopy {
    /// No VS Code metadata (another app, or a VS Code version without it).
    Unknown,
    /// A normal single selection.
    Selection,
    /// VS Code copied the whole line because nothing was selected.
    EmptySelection,
    /// Several cursors/selections were copied.
    MultiCursor,
}

const VSCODE_KEY: &str = "vscode-editor-data";

/// Parses VS Code's `vscode-editor-data` JSON. M1 found it inside the "Chromium Web Custom MIME
/// Data Format" blob (a Chromium pickle of UTF-16 key/value strings), not as its own format.
pub fn parse_vscode_marker(blob: &[u8]) -> VsCodeCopy {
    let json = pickle_value_after_key(blob, VSCODE_KEY).or_else(|| scan_json_after_key(blob));
    let Some(json) = json else {
        return VsCodeCopy::Unknown;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
        return VsCodeCopy::Unknown;
    };
    if value
        .get("multicursorText")
        .is_some_and(|text| !text.is_null())
    {
        return VsCodeCopy::MultiCursor;
    }
    match value
        .get("isFromEmptySelection")
        .and_then(serde_json::Value::as_bool)
    {
        Some(true) => VsCodeCopy::EmptySelection,
        Some(false) => VsCodeCopy::Selection,
        None => VsCodeCopy::Unknown,
    }
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn find_bytes(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

/// Reads the pickle string16 that follows the key: a little-endian u32 length in UTF-16 units
/// at the next 4-byte boundary, then the UTF-16LE text.
fn pickle_value_after_key(blob: &[u8], key: &str) -> Option<String> {
    let needle = utf16le(key);
    let mut search = 0;
    while let Some(position) = find_bytes(blob, &needle, search) {
        search = position + 1;
        if position % 2 != 0 {
            continue;
        }
        let length_at = (position + needle.len()).next_multiple_of(4);
        let length = u32::from_le_bytes(blob.get(length_at..length_at + 4)?.try_into().ok()?);
        let start = length_at + 4;
        let end = start.checked_add((length as usize).checked_mul(2)?)?;
        let units: Vec<u16> = blob
            .get(start..end)?
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let text = String::from_utf16(&units).ok()?;
        if text.trim_start().starts_with('{') {
            return Some(text);
        }
    }
    None
}

/// Fallback: decode the blob as UTF-16LE (and UTF-8) and take the first JSON object after the
/// key that parses. String-aware so braces inside copied text do not confuse the scan.
fn scan_json_after_key(blob: &[u8]) -> Option<String> {
    let utf16: Vec<u16> = blob
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let decodings = [
        String::from_utf16_lossy(&utf16),
        String::from_utf8_lossy(blob).into_owned(),
    ];
    for decoded in decodings {
        let Some(key_at) = decoded.find(VSCODE_KEY) else {
            continue;
        };
        let rest = &decoded[key_at + VSCODE_KEY.len()..];
        for (offset, _) in rest.match_indices('{').take(8) {
            if let Some(object) = balanced_object(&rest[offset..]) {
                if serde_json::from_str::<serde_json::Value>(object).is_ok() {
                    return Some(object.to_string());
                }
            }
        }
    }
    None
}

/// Returns the balanced `{...}` prefix of `text`, skipping braces inside JSON strings.
fn balanced_object(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if in_string {
            match (escaped, character) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&text[..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The clipboard contents produced by Vellora's injected copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedSelection {
    /// Unicode text, or `None` when the copy had no (valid) text.
    pub text: Option<String>,
    pub vscode: VsCodeCopy,
}

/// Reads the text and VS Code marker currently on the clipboard.
pub fn read_copied_selection() -> anyhow::Result<CopiedSelection> {
    let _open = open_clipboard()?;
    let text = match unsafe { GetClipboardData(CF_UNICODETEXT) } {
        Ok(handle) => unsafe { read_hglobal(HGLOBAL(handle.0), MAX_SNAPSHOT_BYTES) }
            .and_then(|bytes| decode_unicode_text(&bytes)),
        Err(_) => None,
    };
    let chromium = unsafe { RegisterClipboardFormatW(w!("Chromium Web Custom MIME Data Format")) };
    let vscode = if chromium == 0 {
        VsCodeCopy::Unknown
    } else {
        match unsafe { GetClipboardData(chromium) } {
            Ok(handle) => unsafe { read_hglobal(HGLOBAL(handle.0), 4 * 1024 * 1024) }
                .map(|blob| parse_vscode_marker(&blob))
                .unwrap_or(VsCodeCopy::Unknown),
            Err(_) => VsCodeCopy::Unknown,
        }
    };
    Ok(CopiedSelection { text, vscode })
}

fn send_keys(inputs: &[INPUT]) -> anyhow::Result<()> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    anyhow::ensure!(
        sent == inputs.len() as u32,
        "Windows SendInput did not send the full shortcut"
    );
    Ok(())
}

fn send_ctrl_chord(key: VIRTUAL_KEY) -> anyhow::Result<()> {
    send_keys(&[
        key_input(VK_CONTROL, false),
        key_input(key, false),
        key_input(key, true),
        key_input(VK_CONTROL, true),
    ])
}

/// Injects a Vellora-signed Ctrl+C.
pub fn send_copy() -> anyhow::Result<()> {
    send_ctrl_chord(VK_C)
}

/// Injects a Vellora-signed Ctrl+V.
pub fn send_paste() -> anyhow::Result<()> {
    send_ctrl_chord(VK_V)
}

/// Signed Alt down, mask key, Alt up: lets Vellora call SetForegroundWindow without opening the
/// foreground app's menu bar (M1 finding).
pub fn send_foreground_unlock() -> anyhow::Result<()> {
    send_keys(&[
        key_input(VK_MENU, false),
        key_input(VK_MASK, false),
        key_input(VK_MASK, true),
        key_input(VK_MENU, true),
    ])
}

/// True while Shift, Ctrl, Alt or Win is physically down. Injecting Ctrl+C then would become
/// another shortcut (for example Ctrl+Shift+C).
pub fn modifiers_down() -> bool {
    [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(key.0 as i32) } < 0)
}

/// Waits until no modifier is down, up to `timeout`. Returns false on timeout.
pub fn wait_modifiers_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while modifiers_down() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    true
}

/// Timing for [`wait_for_copy`].
#[derive(Debug, Clone, Copy)]
pub struct CopyWait {
    /// Maximum wait for the first sequence change after the injected copy.
    pub first_change: Duration,
    /// The sequence must stay unchanged this long before reading (OLE delayed rendering).
    pub settle: Duration,
    /// Upper bound for the settle phase.
    pub max_settle: Duration,
}

impl Default for CopyWait {
    fn default() -> Self {
        Self {
            first_change: Duration::from_millis(500),
            settle: Duration::from_millis(50),
            max_settle: Duration::from_millis(250),
        }
    }
}

/// Waits for the clipboard sequence to change from `before` and then settle. Returns the settled
/// sequence number, or `None` when nothing changed (no selection, or an unsupported/elevated app).
pub fn wait_for_copy(before: u32, timing: CopyWait) -> Option<u32> {
    wait_for_copy_with(before, timing, sequence, Instant::now, thread::sleep)
}

pub(crate) fn wait_for_copy_with(
    before: u32,
    timing: CopyWait,
    mut read: impl FnMut() -> u32,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> Option<u32> {
    let started = now();
    let mut last = loop {
        let current = read();
        if current != before {
            break current;
        }
        if now().duration_since(started) >= timing.first_change {
            return None;
        }
        sleep(Duration::from_millis(2));
    };
    let settle_started = now();
    let mut changed_at = settle_started;
    loop {
        sleep(Duration::from_millis(5));
        let current = read();
        let at = now();
        if current != last {
            last = current;
            changed_at = at;
        }
        if at.duration_since(changed_at) >= timing.settle
            || at.duration_since(settle_started) >= timing.max_settle
        {
            return Some(last);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[test]
    fn format_filter_skips_gdi_owner_display_private_and_synthesized_text() {
        let with_unicode = [CF_UNICODETEXT, CF_TEXT, CF_OEMTEXT, CF_BITMAP, 8, 17];
        assert!(keep_format(CF_UNICODETEXT, &with_unicode));
        assert!(!keep_format(CF_TEXT, &with_unicode));
        assert!(!keep_format(CF_OEMTEXT, &with_unicode));
        assert!(keep_format(CF_TEXT, &[CF_TEXT]));
        for skipped in [
            CF_BITMAP,
            CF_METAFILEPICT,
            CF_PALETTE,
            CF_ENHMETAFILE,
            CF_OWNERDISPLAY,
            CF_DSPBITMAP,
            0x0200,
            0x03FF,
        ] {
            assert!(!keep_format(skipped, &with_unicode), "{skipped:#x}");
        }
        // DIB, DIBV5, HDROP, locale and registered formats (HTML Format, PNG) are kept.
        for kept in [8, 17, 15, 16, 0xC0FF, 0xC123] {
            assert!(keep_format(kept, &with_unicode), "{kept:#x}");
        }
    }

    #[test]
    fn unicode_text_round_trips_and_rejects_invalid_utf16() {
        let text = "Grüße, ß – 🙂\r\nzweite Zeile";
        assert_eq!(
            decode_unicode_text(&utf16z_bytes(text)).as_deref(),
            Some(text)
        );
        // Trailing bytes after the terminator are ignored.
        let mut bytes = utf16z_bytes("abc");
        bytes.extend_from_slice(&[0x41, 0x00]);
        assert_eq!(decode_unicode_text(&bytes).as_deref(), Some("abc"));
        // An unpaired surrogate is rejected, never replaced.
        assert_eq!(decode_unicode_text(&[0x00, 0xD8, 0x41, 0x00]), None);
    }

    /// Builds a Chromium-style pickle: u32 payload size, u32 count, then string16 pairs.
    fn chromium_blob(pairs: &[(&str, &str)]) -> Vec<u8> {
        fn string16(out: &mut Vec<u8>, text: &str) {
            let units: Vec<u16> = text.encode_utf16().collect();
            out.extend_from_slice(&(units.len() as u32).to_le_bytes());
            for unit in units {
                out.extend_from_slice(&unit.to_le_bytes());
            }
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
        }
        let mut payload = Vec::new();
        payload.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
        for (key, value) in pairs {
            string16(&mut payload, key);
            string16(&mut payload, value);
        }
        let mut blob = (payload.len() as u32).to_le_bytes().to_vec();
        blob.extend(payload);
        blob
    }

    #[test]
    fn vscode_marker_is_read_from_the_chromium_pickle() {
        let selection = r#"{"version":1,"id":"a","isFromEmptySelection":false,"isBlock":false,"multicursorText":null,"mode":null}"#;
        let empty = r#"{"version":1,"id":"b","isFromEmptySelection":true,"isBlock":false,"multicursorText":null,"mode":null}"#;
        let multi = r#"{"version":1,"id":"c","isFromEmptySelection":false,"multicursorText":["a{","b}\""],"mode":null}"#;
        let cases = [
            (selection, VsCodeCopy::Selection),
            (empty, VsCodeCopy::EmptySelection),
            (multi, VsCodeCopy::MultiCursor),
        ];
        for (json, expected) in cases {
            let blob = chromium_blob(&[("text/plain-other", "x"), (VSCODE_KEY, json)]);
            assert_eq!(parse_vscode_marker(&blob), expected, "{json}");
        }
    }

    #[test]
    fn vscode_marker_handles_a_length_prefix_that_looks_like_a_brace() {
        // A JSON value of exactly 123 UTF-16 units has the length prefix 0x7B = '{'.
        let mut json =
            String::from(r#"{"isFromEmptySelection":true,"multicursorText":null,"pad":""#);
        while json.chars().count() < 121 {
            json.push('x');
        }
        json.push_str("\"}");
        assert_eq!(json.encode_utf16().count(), 123);
        let blob = chromium_blob(&[(VSCODE_KEY, &json)]);
        assert_eq!(parse_vscode_marker(&blob), VsCodeCopy::EmptySelection);
        // The scanning fallback alone also skips the fake brace.
        assert_eq!(scan_json_after_key(&blob).as_deref(), Some(json.as_str()));
        // A truncated blob yields Unknown instead of a guess.
        let mut damaged = blob.clone();
        damaged.truncate(blob.len() - 6);
        assert_eq!(parse_vscode_marker(&damaged), VsCodeCopy::Unknown);
    }

    #[test]
    fn vscode_marker_is_unknown_without_key_or_with_bad_json() {
        assert_eq!(parse_vscode_marker(b""), VsCodeCopy::Unknown);
        assert_eq!(
            parse_vscode_marker(&chromium_blob(&[("text/html", "<b>x</b>")])),
            VsCodeCopy::Unknown
        );
        assert_eq!(
            parse_vscode_marker(&chromium_blob(&[(VSCODE_KEY, "{not json")])),
            VsCodeCopy::Unknown
        );
        assert_eq!(
            parse_vscode_marker(&chromium_blob(&[(VSCODE_KEY, r#"{"version":1}"#)])),
            VsCodeCopy::Unknown
        );
    }

    #[test]
    fn balanced_object_ignores_braces_inside_strings() {
        assert_eq!(
            balanced_object(r#"{"a":"}{\"}"} tail"#),
            Some(r#"{"a":"}{\"}"}"#)
        );
        assert_eq!(balanced_object("{unterminated"), None);
    }

    /// Fake clock and clipboard: `script` lists (time offset ms, sequence) changes.
    fn run_wait(script: &[(u64, u32)], timing: CopyWait) -> (Option<u32>, u64) {
        let base = Instant::now();
        let elapsed = Cell::new(0u64);
        let script = RefCell::new(script.to_vec());
        let read = || {
            let at = elapsed.get();
            let mut value = 1;
            for &(offset, sequence) in script.borrow().iter() {
                if offset <= at {
                    value = sequence;
                }
            }
            value
        };
        let now = || base + Duration::from_millis(elapsed.get());
        let sleep = |duration: Duration| elapsed.set(elapsed.get() + duration.as_millis() as u64);
        let result = wait_for_copy_with(1, timing, read, now, sleep);
        (result, elapsed.get())
    }

    #[test]
    fn copy_wait_times_out_when_nothing_changes() {
        let (result, elapsed) = run_wait(&[], CopyWait::default());
        assert_eq!(result, None);
        assert!((500..520).contains(&elapsed), "{elapsed}");
    }

    #[test]
    fn copy_wait_settles_after_ole_delayed_rendering_changes() {
        // First change at 10 ms, then Notepad-like follow-up changes until 60 ms.
        let (result, elapsed) =
            run_wait(&[(10, 2), (20, 3), (35, 5), (60, 8)], CopyWait::default());
        assert_eq!(result, Some(8));
        assert!((110..=130).contains(&elapsed), "{elapsed}");
    }

    #[test]
    fn copy_wait_is_bounded_by_max_settle() {
        let changes: Vec<(u64, u32)> = (0..200).map(|i| (10 + i * 10, 2 + i as u32)).collect();
        let (result, elapsed) = run_wait(&changes, CopyWait::default());
        assert!(result.is_some());
        assert!(elapsed <= 10 + 250 + 10, "{elapsed}");
    }
}
