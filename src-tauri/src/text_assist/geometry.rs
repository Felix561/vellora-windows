//! Text Assist geometry (spec sections 2.4, 2.5 and the M1 findings): where the halo is drawn.
//!
//! `inspect_target` finds an anchor for the target window, trying in order
//! 1. UI Automation selection rectangles (box around the selection; a caret if only narrow
//!    rectangles come back, as in VS Code), skipped for Chromium/Electron windows,
//! 2. the MSAA caret (`OBJID_CARET`), then the GUI-thread caret (`GetGUIThreadInfo`),
//! 3. the mouse pointer,
//! 4. the top center of the primary monitor's work area.
//!
//! `layout` turns an anchor into the screen rectangle of the transparent halo window plus
//! window-relative coordinates for the box and the progress ring. All coordinates are physical
//! pixels. The Win32/COM code is kept thin; every decision lives in pure, unit-tested functions.

// Wired into the halo window and the job runner in M2/M3; until then some items are unused.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use serde::Serialize;

// ------------------------------------------------------------------------------------------
// Public types
// ------------------------------------------------------------------------------------------

/// Rectangle in physical screen pixels (or window-relative pixels where documented).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointPx {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    Selection,
    Caret,
    Pointer,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub kind: AnchorKind,
    pub rect: RectPx,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FocusInfo {
    /// `GetGUIThreadInfo(target thread).hwndFocus`; 0 when unknown.
    pub focus_hwnd: isize,
    /// Runtime id of the UIA focused element (None for Chromium windows or on failure).
    pub uia_runtime_id: Option<Vec<i32>>,
    /// `IsPassword` of the UIA focused element (None for Chromium windows or on failure).
    pub is_password: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HaloMode {
    Box,
    Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BadgeSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaloLayout {
    /// Screen rect of the transparent halo window (physical px).
    pub window: RectPx,
    pub mode: HaloMode,
    /// Window-relative physical px; Some in Box mode.
    pub box_rect: Option<RectPx>,
    /// Window-relative: box top-right corner (Box) or caret top-right (Point).
    pub point: PointPx,
    /// Which side of `point` the label chip extends to.
    pub badge_side: BadgeSide,
    /// Monitor scale factor used for the layout.
    pub scale: f64,
}

// ------------------------------------------------------------------------------------------
// Rect helpers (pure)
// ------------------------------------------------------------------------------------------

impl RectPx {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn from_ltrb(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self::new(left, top, right - left, bottom - top)
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub const fn top_right(&self) -> PointPx {
        PointPx {
            x: self.right(),
            y: self.y,
        }
    }

    pub const fn inflate(&self, by: i32) -> Self {
        Self::from_ltrb(
            self.x - by,
            self.y - by,
            self.right() + by,
            self.bottom() + by,
        )
    }

    pub fn union(&self, other: &Self) -> Self {
        Self::from_ltrb(
            self.x.min(other.x),
            self.y.min(other.y),
            self.right().max(other.right()),
            self.bottom().max(other.bottom()),
        )
    }

    pub const fn offset(&self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }
}

// ------------------------------------------------------------------------------------------
// Anchor decisions (pure)
// ------------------------------------------------------------------------------------------

/// Selection rectangles narrower than this are carets (VS Code reports a 1 px rectangle).
const MIN_SELECTION_WIDTH: i32 = 3;

/// Window class of Chromium/Electron top-level windows. UIA gives no useful selection
/// rectangles there, and querying it switches on Chromium's accessibility tree.
const CHROMIUM_CLASS: &str = "Chrome_WidgetWin_1";

pub(crate) fn is_chromium_class(class_name: &str) -> bool {
    class_name == CHROMIUM_CLASS
}

/// Converts UIA `GetBoundingRectangles` output (left, top, width, height, ...) to pixel rects.
/// Incomplete trailing groups, non-finite values and negative sizes are dropped.
pub(crate) fn rects_from_doubles(values: &[f64]) -> Vec<RectPx> {
    values
        .chunks_exact(4)
        .filter(|c| c.iter().all(|v| v.is_finite()) && c[2] >= 0.0 && c[3] >= 0.0)
        .filter(|c| c.iter().all(|v| v.abs() < 1.0e9))
        .map(|c| {
            let left = c[0].round() as i32;
            let top = c[1].round() as i32;
            let right = (c[0] + c[2]).round() as i32;
            let bottom = (c[1] + c[3]).round() as i32;
            RectPx::from_ltrb(left, top, right, bottom)
        })
        .collect()
}

/// A caret rectangle counts unless it is zero-size (or has a negative size).
pub(crate) fn is_usable_caret(rect: &RectPx) -> bool {
    rect.width >= 0 && rect.height >= 0 && (rect.width > 0 || rect.height > 0)
}

/// Selection anchor from the bounding rectangles of all selection ranges: the union of the
/// rectangles at least 3 px wide; if there are none, the first rectangle as a caret.
pub(crate) fn anchor_from_selection_rects(rects: &[RectPx]) -> Option<Anchor> {
    let union = rects
        .iter()
        .filter(|r| r.width >= MIN_SELECTION_WIDTH && r.height > 0)
        .fold(None::<RectPx>, |acc, r| {
            Some(acc.map_or(*r, |u| u.union(r)))
        });
    if let Some(rect) = union {
        return Some(Anchor {
            kind: AnchorKind::Selection,
            rect,
        });
    }
    rects.first().and_then(|r| caret_anchor(*r))
}

pub(crate) fn caret_anchor(rect: RectPx) -> Option<Anchor> {
    is_usable_caret(&rect).then_some(Anchor {
        kind: AnchorKind::Caret,
        rect,
    })
}

pub(crate) fn pointer_anchor(point: PointPx) -> Anchor {
    Anchor {
        kind: AnchorKind::Pointer,
        rect: RectPx::new(point.x, point.y, 1, 1),
    }
}

/// Top center of the given (primary monitor) work area, 1x1.
pub(crate) fn fallback_anchor(work_area: RectPx) -> Anchor {
    Anchor {
        kind: AnchorKind::Fallback,
        rect: RectPx::new(work_area.x + work_area.width / 2, work_area.y, 1, 1),
    }
}

/// DPI to scale factor (96 DPI = 1.0); None for 0 (the API's failure value).
pub(crate) fn scale_from_dpi(dpi: u32) -> Option<f64> {
    (dpi > 0).then(|| f64::from(dpi) / 96.0)
}

// ------------------------------------------------------------------------------------------
// Layout (pure)
// ------------------------------------------------------------------------------------------

// All in CSS px; multiplied by the monitor scale.
/// Box padding around the selection union.
const BOX_PAD: f64 = 6.0;
/// Window margin around the box on the left, right and bottom.
const BOX_MARGIN_SIDE: f64 = 14.0;
/// Window margin above the box (the ring sits on the box's top-right corner).
const BOX_MARGIN_TOP: f64 = 24.0;
/// Minimum window width measured from the box's right edge + margin (room for the chip).
const BOX_MIN_WIDTH: f64 = 320.0;
/// Point mode window size.
const POINT_WINDOW_WIDTH: f64 = 340.0;
const POINT_WINDOW_HEIGHT: f64 = 44.0;
/// Point mode: distance of the point from the window's top and from its near side edge.
const POINT_TOP: f64 = 22.0;
const POINT_SIDE: f64 = 8.0;

fn px(css: f64, scale: f64) -> i32 {
    (css * scale).round() as i32
}

/// Clamps `v` to `lo..=hi`; if the range is empty, `lo` wins.
fn clamp_lo(v: i32, lo: i32, hi: i32) -> i32 {
    v.min(hi).max(lo)
}

/// Shifts a span into `lo..hi`; a span longer than the range is cut to the range.
fn clamp_span(start: i32, len: i32, lo: i32, hi: i32) -> (i32, i32) {
    if len >= hi - lo {
        (lo, hi - lo)
    } else if start < lo {
        (lo, len)
    } else if start + len > hi {
        (hi - len, len)
    } else {
        (start, len)
    }
}

/// Moves `window` inside `work_area` (shift only; it is only cut where it is larger than the
/// work area itself). An empty work area leaves the window unchanged.
fn clamp_window(window: RectPx, work_area: RectPx) -> RectPx {
    if work_area.width <= 0 || work_area.height <= 0 {
        return window;
    }
    let (x, width) = clamp_span(window.x, window.width, work_area.x, work_area.right());
    let (y, height) = clamp_span(window.y, window.height, work_area.y, work_area.bottom());
    RectPx::new(x, y, width, height)
}

/// Pure layout. `work_area` is the work area of the monitor containing the anchor; `scale` its
/// DPI scale (1.0 = 96 DPI).
pub fn layout(anchor: &Anchor, work_area: RectPx, scale: f64) -> HaloLayout {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    match anchor.kind {
        AnchorKind::Selection => layout_box(anchor.rect, work_area, scale),
        AnchorKind::Caret | AnchorKind::Pointer | AnchorKind::Fallback => {
            layout_point(anchor.rect.top_right(), work_area, scale)
        }
    }
}

fn layout_box(selection: RectPx, work_area: RectPx, scale: f64) -> HaloLayout {
    let side = px(BOX_MARGIN_SIDE, scale);
    let min_width = px(BOX_MIN_WIDTH, scale);
    let box_screen = selection.inflate(px(BOX_PAD, scale));
    let corner = box_screen.top_right();

    let mut left = box_screen.x - side;
    let top = box_screen.y - px(BOX_MARGIN_TOP, scale);
    let mut right = box_screen.right() + side;
    let bottom = box_screen.bottom() + side;

    // The chip grows leftwards from the ring. If the work area has no room for it on the left
    // but more room on the right, let it grow rightwards instead.
    let room_left = right - work_area.x;
    let room_right = work_area.right() - (corner.x - side);
    let badge_side = if work_area.width > 0 && room_left < min_width && room_right > room_left {
        BadgeSide::Right
    } else {
        BadgeSide::Left
    };
    match badge_side {
        BadgeSide::Left => left = left.min(right - min_width),
        BadgeSide::Right => right = right.max(corner.x - side + min_width),
    }

    let window = clamp_window(RectPx::from_ltrb(left, top, right, bottom), work_area);
    let point = PointPx {
        x: clamp_lo(corner.x - window.x, 0, window.width),
        y: clamp_lo(corner.y - window.y, 0, window.height),
    };
    HaloLayout {
        window,
        mode: HaloMode::Box,
        box_rect: Some(box_screen.offset(-window.x, -window.y)),
        point,
        badge_side,
        scale,
    }
}

fn layout_point(point: PointPx, work_area: RectPx, scale: f64) -> HaloLayout {
    let width = px(POINT_WINDOW_WIDTH, scale);
    let height = px(POINT_WINDOW_HEIGHT, scale);
    let top = px(POINT_TOP, scale);
    let side = px(POINT_SIDE, scale);

    let mut badge_side = BadgeSide::Right;
    let mut left = point.x - side;
    if work_area.width > 0 && left + width > work_area.right() {
        // Ring then chip to the left of the point.
        badge_side = BadgeSide::Left;
        left = point.x + side - width;
    }
    let window = clamp_window(RectPx::new(left, point.y - top, width, height), work_area);
    // Keep the ring inside the window even when the anchor lies outside the work area
    // (e.g. the pointer on the taskbar).
    let relative = PointPx {
        x: clamp_lo(point.x - window.x, side, window.width - side),
        y: clamp_lo(point.y - window.y, top, window.height - top),
    };
    HaloLayout {
        window,
        mode: HaloMode::Point,
        box_rect: None,
        point: relative,
        badge_side,
        scale,
    }
}

// ------------------------------------------------------------------------------------------
// Win32 / COM (thin; not unit-testable)
// ------------------------------------------------------------------------------------------

use windows::core::{Interface, VARIANT};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayGetVartype, SafeArrayUnaccessData,
};
use windows::Win32::System::Variant::{VARENUM, VT_I4, VT_R8};
use windows::Win32::UI::Accessibility::{
    AccessibleObjectFromWindow, CUIAutomation, CUIAutomation8, IAccessible, IUIAutomation,
    IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetCursorPos, GetGUIThreadInfo, GetSystemMetrics,
    GetWindowThreadProcessId, SendMessageTimeoutW, WindowFromPoint, GA_ROOT, GUITHREADINFO,
    OBJID_CARET, SMTO_ABORTIFHUNG, SMTO_BLOCK, SM_CXSCREEN, SM_CYSCREEN, WM_NULL,
};

/// UIA connection and transaction timeouts.
const UIA_TIMEOUT_MS: u32 = 500;
/// Responsiveness probe before any cross-process accessibility call.
const HUNG_PROBE_MS: u32 = 200;
/// Overall budget after which remaining UIA/MSAA queries are skipped.
const INSPECT_BUDGET: Duration = Duration::from_millis(800);
/// Upper bound on selection ranges inspected (multi-cursor editors).
const MAX_SELECTION_RANGES: i32 = 64;

/// DPI functions that live outside the `windows` features enabled in Cargo.toml
/// (`Win32_UI_HiDpi`). Both are in the Windows SDK import libraries.
mod ffi {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::HMONITOR;

    /// `MONITOR_DPI_TYPE::MDT_EFFECTIVE_DPI`.
    pub const MDT_EFFECTIVE_DPI: i32 = 0;

    #[link(name = "shcore")]
    extern "system" {
        pub fn GetDpiForMonitor(
            hmonitor: HMONITOR,
            dpitype: i32,
            dpix: *mut u32,
            dpiy: *mut u32,
        ) -> HRESULT;
    }

    /// `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`.
    pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

    #[link(name = "user32")]
    extern "system" {
        pub fn GetDpiForWindow(hwnd: HWND) -> u32;
        /// Takes and returns a `DPI_AWARENESS_CONTEXT` handle (0 on failure).
        pub fn SetThreadDpiAwarenessContext(context: isize) -> isize;
    }
}

fn hwnd(value: isize) -> HWND {
    HWND(value as *mut core::ffi::c_void)
}

/// Initializes COM (MTA) on the calling thread for its lifetime and balances it on drop. If
/// the thread already is an STA, COM stays usable and nothing is balanced.
struct ComGuard {
    initialized: bool,
}

impl ComGuard {
    fn new() -> Self {
        // S_OK and S_FALSE both need a matching CoUninitialize; RPC_E_CHANGED_MODE does not.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self {
            initialized: hr.is_ok(),
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { CoUninitialize() };
        }
    }
}

/// Makes the calling thread per-monitor DPI aware (v2) while alive, so window, caret, monitor
/// and UIA coordinates are all physical pixels even in a process that is not DPI aware.
struct DpiGuard {
    previous: isize,
}

impl DpiGuard {
    fn new() -> Self {
        let previous = unsafe {
            ffi::SetThreadDpiAwarenessContext(ffi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        };
        Self { previous }
    }
}

impl Drop for DpiGuard {
    fn drop(&mut self) {
        if self.previous != 0 {
            unsafe { ffi::SetThreadDpiAwarenessContext(self.previous) };
        }
    }
}

/// Facts about the target window that need no cross-process calls.
struct Target {
    root: HWND,
    thread_id: u32,
    process_id: u32,
    is_chromium: bool,
    gui: Option<GUITHREADINFO>,
}

impl Target {
    fn new(target_hwnd: isize) -> Self {
        let window = hwnd(target_hwnd);
        let root = unsafe { GetAncestor(window, GA_ROOT) };
        let root = if root.is_invalid() { window } else { root };
        let mut process_id = 0u32;
        let thread_id = unsafe { GetWindowThreadProcessId(root, Some(&mut process_id)) };
        let is_chromium = is_chromium_class(&class_name(root));
        let gui = (thread_id != 0)
            .then(|| gui_thread_info(thread_id))
            .flatten();
        Self {
            root,
            thread_id,
            process_id,
            is_chromium,
            gui,
        }
    }

    fn focus_hwnd(&self) -> HWND {
        self.gui
            .map(|g| g.hwndFocus)
            .filter(|h| !h.is_invalid())
            .unwrap_or(self.root)
    }

    /// False when the target does not answer a no-op message quickly (hung or busy); then no
    /// cross-process accessibility call is made.
    fn responds(&self) -> bool {
        if self.thread_id == 0 {
            return false;
        }
        let result = unsafe {
            SendMessageTimeoutW(
                self.root,
                WM_NULL,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                HUNG_PROBE_MS,
                None,
            )
        };
        result.0 != 0
    }
}

fn class_name(window: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetClassNameW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.clamp(0, 256) as usize])
}

fn gui_thread_info(thread_id: u32) -> Option<GUITHREADINFO> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread_id, &mut info) }.ok()?;
    Some(info)
}

fn rect_px(rect: &RECT) -> RectPx {
    RectPx::from_ltrb(rect.left, rect.top, rect.right, rect.bottom)
}

/// Reads a one-dimensional SAFEARRAY of `T` (checked against `vartype`) and destroys it.
///
/// # Safety
/// `psa` must be null or a SAFEARRAY owned by the caller; `T` must match `vartype`.
unsafe fn take_safearray<T: Copy>(psa: *mut SAFEARRAY, vartype: VARENUM) -> Option<Vec<T>> {
    if psa.is_null() {
        return None;
    }
    let read = || -> Option<Vec<T>> {
        if SafeArrayGetDim(psa) != 1 || SafeArrayGetVartype(psa).ok()? != vartype {
            return None;
        }
        let lower = SafeArrayGetLBound(psa, 1).ok()?;
        let upper = SafeArrayGetUBound(psa, 1).ok()?;
        let len = usize::try_from(i64::from(upper) - i64::from(lower) + 1).unwrap_or(0);
        let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
        SafeArrayAccessData(psa, &mut data).ok()?;
        let values = if len == 0 || data.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(data as *const T, len).to_vec()
        };
        let _ = SafeArrayUnaccessData(psa);
        Some(values)
    };
    let values = read();
    let _ = SafeArrayDestroy(psa);
    values
}

/// UIA client with 500 ms connection and transaction timeouts (`CUIAutomation8` provides
/// `IUIAutomation2`; plain `CUIAutomation` is the fallback, without timeouts).
fn uia_client() -> Option<IUIAutomation> {
    unsafe {
        if let Ok(client) =
            CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
        {
            let _ = client.SetConnectionTimeout(UIA_TIMEOUT_MS);
            let _ = client.SetTransactionTimeout(UIA_TIMEOUT_MS);
            return client.cast().ok();
        }
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
    }
}

/// The UIA focused element, if it belongs to the target process (or to the process of the
/// target thread's focus window). Guards against the foreground having changed.
fn uia_focused_element(client: &IUIAutomation, target: &Target) -> Option<IUIAutomationElement> {
    unsafe {
        let element = client.GetFocusedElement().ok()?;
        let pid = u32::try_from(element.CurrentProcessId().ok()?).ok()?;
        if pid == 0 {
            return None;
        }
        let mut focus_pid = 0u32;
        GetWindowThreadProcessId(target.focus_hwnd(), Some(&mut focus_pid));
        (pid == target.process_id || pid == focus_pid).then_some(element)
    }
}

fn uia_selection_rects(element: &IUIAutomationElement, deadline: Instant) -> Vec<RectPx> {
    let mut rects = Vec::new();
    unsafe {
        let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        else {
            return rects;
        };
        let Ok(ranges) = pattern.GetSelection() else {
            return rects;
        };
        let count = ranges.Length().unwrap_or(0).clamp(0, MAX_SELECTION_RANGES);
        for index in 0..count {
            if Instant::now() > deadline {
                break;
            }
            let Ok(range) = ranges.GetElement(index) else {
                continue;
            };
            let Ok(psa) = range.GetBoundingRectangles() else {
                continue;
            };
            if let Some(values) = take_safearray::<f64>(psa, VT_R8) {
                rects.extend(rects_from_doubles(&values));
            }
        }
    }
    rects
}

fn uia_focus_fields(element: &IUIAutomationElement, info: &mut FocusInfo) {
    unsafe {
        info.uia_runtime_id = element
            .GetRuntimeId()
            .ok()
            .and_then(|psa| take_safearray::<i32>(psa, VT_I4));
        info.is_password = element.CurrentIsPassword().ok().map(|b| b.as_bool());
    }
}

fn msaa_caret(window: HWND) -> Option<RectPx> {
    unsafe {
        let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
        AccessibleObjectFromWindow(window, OBJID_CARET.0 as u32, &IAccessible::IID, &mut raw)
            .ok()?;
        if raw.is_null() {
            return None;
        }
        let accessible = IAccessible::from_raw(raw);
        let (mut left, mut top, mut width, mut height) = (0, 0, 0, 0);
        // CHILDID_SELF
        let child = VARIANT::from(0i32);
        accessible
            .accLocation(&mut left, &mut top, &mut width, &mut height, &child)
            .ok()?;
        let rect = RectPx::new(left, top, width, height);
        is_usable_caret(&rect).then_some(rect)
    }
}

fn gui_caret(gui: &GUITHREADINFO) -> Option<RectPx> {
    if gui.hwndCaret.is_invalid() {
        return None;
    }
    let client = &gui.rcCaret;
    let mut top_left = POINT {
        x: client.left,
        y: client.top,
    };
    let mut bottom_right = POINT {
        x: client.right,
        y: client.bottom,
    };
    unsafe {
        if !ClientToScreen(gui.hwndCaret, &mut top_left).as_bool()
            || !ClientToScreen(gui.hwndCaret, &mut bottom_right).as_bool()
        {
            return None;
        }
    }
    let rect = RectPx::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);
    is_usable_caret(&rect).then_some(rect)
}

fn cursor_pos() -> Option<PointPx> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some(PointPx {
        x: point.x,
        y: point.y,
    })
}

fn work_area_of(monitor: HMONITOR) -> Option<RectPx> {
    if monitor.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }
        .as_bool()
        .then(|| rect_px(&info.rcWork))
}

fn screen_rect() -> RectPx {
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    if width > 0 && height > 0 {
        RectPx::new(0, 0, width, height)
    } else {
        RectPx::new(0, 0, 1920, 1080)
    }
}

fn primary_work_area() -> RectPx {
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    work_area_of(monitor).unwrap_or_else(screen_rect)
}

/// Inspect the target (foreground) window: anchor + focus info. Runs on any worker thread
/// (initializes COM as MTA on the calling thread if needed and balances it; the thread is
/// per-monitor DPI aware for the duration of the call). Finishes within
/// about 1 s even if the target app hangs: a 200 ms responsiveness probe gates all
/// cross-process calls, UIA uses 500 ms connection/transaction timeouts, and later UIA/MSAA
/// steps are skipped once 800 ms have passed.
pub fn inspect_target(target_hwnd: isize) -> (Anchor, FocusInfo) {
    let _dpi = DpiGuard::new();
    let _com = ComGuard::new();
    inspect_target_inner(target_hwnd)
}

fn inspect_target_inner(target_hwnd: isize) -> (Anchor, FocusInfo) {
    let deadline = Instant::now() + INSPECT_BUDGET;
    let target = Target::new(target_hwnd);
    let mut info = FocusInfo {
        focus_hwnd: target.gui.map_or(0, |g| g.hwndFocus.0 as isize),
        ..Default::default()
    };
    let responds = target.responds();

    // 1. UI Automation selection (not for Chromium/Electron).
    let mut anchor = None;
    if responds && !target.is_chromium {
        if let Some(element) = uia_client().and_then(|c| uia_focused_element(&c, &target)) {
            uia_focus_fields(&element, &mut info);
            if Instant::now() <= deadline {
                anchor = anchor_from_selection_rects(&uia_selection_rects(&element, deadline));
            }
        }
    }

    // 2. Caret: MSAA, then the GUI-thread caret.
    if anchor.is_none() && responds && Instant::now() <= deadline {
        anchor = msaa_caret(target.focus_hwnd()).and_then(caret_anchor);
    }
    if anchor.is_none() {
        anchor = target
            .gui
            .as_ref()
            .and_then(gui_caret)
            .and_then(caret_anchor);
    }

    // 3. Pointer, 4. fallback.
    let anchor = anchor
        .or_else(|| cursor_pos().map(pointer_anchor))
        .unwrap_or_else(|| fallback_anchor(primary_work_area()));
    (anchor, info)
}

/// Focus info only (for re-verification right before pasting). Same threading rules.
pub fn focus_info(target_hwnd: isize) -> FocusInfo {
    let _com = ComGuard::new();
    focus_info_inner(target_hwnd)
}

fn focus_info_inner(target_hwnd: isize) -> FocusInfo {
    let target = Target::new(target_hwnd);
    let mut info = FocusInfo {
        focus_hwnd: target.gui.map_or(0, |g| g.hwndFocus.0 as isize),
        ..Default::default()
    };
    if !target.is_chromium && target.responds() {
        if let Some(element) = uia_client().and_then(|c| uia_focused_element(&c, &target)) {
            uia_focus_fields(&element, &mut info);
        }
    }
    info
}

/// Work area and scale of the monitor containing a point (nearest monitor if none does).
/// Scale comes from `GetDpiForMonitor` (effective DPI), else from `GetDpiForWindow` of the
/// window under the point, else 1.0.
pub fn monitor_for(point: PointPx) -> (RectPx, f64) {
    let _dpi = DpiGuard::new();
    let pt = POINT {
        x: point.x,
        y: point.y,
    };
    let monitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let work_area = work_area_of(monitor).unwrap_or_else(screen_rect);

    let monitor_scale = (!monitor.is_invalid())
        .then(|| {
            let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
            let hr = unsafe {
                ffi::GetDpiForMonitor(monitor, ffi::MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)
            };
            if hr.is_ok() {
                scale_from_dpi(dpi_x)
            } else {
                None
            }
        })
        .flatten();
    let scale = monitor_scale
        .or_else(|| {
            let window = unsafe { WindowFromPoint(pt) };
            (!window.is_invalid())
                .then(|| scale_from_dpi(unsafe { ffi::GetDpiForWindow(window) }))
                .flatten()
        })
        .unwrap_or(1.0);
    (work_area, scale)
}

// ------------------------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: RectPx = RectPx::new(0, 0, 1920, 1040);

    fn selection(x: i32, y: i32, width: i32, height: i32) -> Anchor {
        Anchor {
            kind: AnchorKind::Selection,
            rect: RectPx::new(x, y, width, height),
        }
    }

    fn caret(x: i32, y: i32, width: i32, height: i32) -> Anchor {
        Anchor {
            kind: AnchorKind::Caret,
            rect: RectPx::new(x, y, width, height),
        }
    }

    /// The box's screen position must not move when the window is clamped.
    fn assert_box_on_screen(layout: &HaloLayout, expected_box_screen: RectPx) {
        let b = layout.box_rect.expect("box mode");
        assert_eq!(
            b.offset(layout.window.x, layout.window.y),
            expected_box_screen
        );
    }

    fn assert_inside(window: RectPx, work: RectPx) {
        assert!(
            window.x >= work.x && window.y >= work.y,
            "{window:?} in {work:?}"
        );
        assert!(
            window.right() <= work.right() && window.bottom() <= work.bottom(),
            "{window:?} in {work:?}"
        );
    }

    // --- rect helpers and anchor decisions ---

    #[test]
    fn geometry_rect_union_and_inflate() {
        let a = RectPx::new(10, 20, 30, 10);
        let b = RectPx::new(5, 25, 10, 20);
        assert_eq!(a.union(&b), RectPx::from_ltrb(5, 20, 40, 45));
        assert_eq!(a.inflate(6), RectPx::new(4, 14, 42, 22));
        assert_eq!(a.top_right(), PointPx { x: 40, y: 20 });
    }

    #[test]
    fn geometry_rects_from_doubles_rounds_and_drops_bad_groups() {
        let mut values = [
            [10.4, 20.6, 100.2, 18.0],   // ok
            [1.0, f64::NAN, 5.0, 5.0],   // NaN
            [0.0, 0.0, -4.0, 5.0],       // negative width
            [-100.5, -50.0, 10.0, 10.0], // negative coordinates are fine
        ]
        .concat();
        values.extend([7.0, 7.0]); // incomplete group
        assert_eq!(
            rects_from_doubles(&values),
            vec![
                RectPx::from_ltrb(10, 21, 111, 39),
                RectPx::from_ltrb(-101, -50, -91, -40),
            ]
        );
        assert!(rects_from_doubles(&[]).is_empty());
    }

    #[test]
    fn geometry_selection_union_ignores_narrow_rects() {
        let rects = [
            RectPx::new(100, 200, 1, 18), // caret-like, ignored
            RectPx::new(120, 200, 300, 18),
            RectPx::new(40, 218, 200, 18),
        ];
        assert_eq!(
            anchor_from_selection_rects(&rects),
            Some(selection(40, 200, 380, 36))
        );
    }

    #[test]
    fn geometry_only_narrow_rects_become_caret() {
        // VS Code: only a 1 px caret rectangle.
        let rects = [RectPx::new(300, 400, 1, 19), RectPx::new(500, 400, 2, 19)];
        assert_eq!(
            anchor_from_selection_rects(&rects),
            Some(caret(300, 400, 1, 19))
        );
    }

    #[test]
    fn geometry_no_or_zero_rects_give_no_anchor() {
        assert_eq!(anchor_from_selection_rects(&[]), None);
        assert_eq!(
            anchor_from_selection_rects(&[RectPx::new(300, 400, 0, 0)]),
            None
        );
        // Wide but zero height: not a selection, and the first rect is not a caret either.
        assert_eq!(
            anchor_from_selection_rects(&[RectPx::new(300, 400, 0, 0), RectPx::new(1, 1, 50, 0)]),
            None
        );
    }

    #[test]
    fn geometry_caret_usability() {
        assert!(is_usable_caret(&RectPx::new(10, 10, 1, 20)));
        assert!(is_usable_caret(&RectPx::new(10, 10, 0, 20)));
        assert!(!is_usable_caret(&RectPx::new(10, 10, 0, 0)));
        assert!(!is_usable_caret(&RectPx::new(0, 0, 0, 0)));
        assert!(!is_usable_caret(&RectPx::new(10, 10, -1, 20)));
        assert_eq!(caret_anchor(RectPx::new(0, 0, 0, 0)), None);
        assert_eq!(
            caret_anchor(RectPx::new(5, 6, 2, 18)),
            Some(caret(5, 6, 2, 18))
        );
    }

    #[test]
    fn geometry_chromium_detection() {
        assert!(is_chromium_class("Chrome_WidgetWin_1"));
        assert!(!is_chromium_class("Chrome_WidgetWin_0"));
        assert!(!is_chromium_class("Notepad"));
        assert!(!is_chromium_class(""));
    }

    #[test]
    fn geometry_pointer_and_fallback_anchors() {
        assert_eq!(
            pointer_anchor(PointPx { x: -5, y: 7 }),
            Anchor {
                kind: AnchorKind::Pointer,
                rect: RectPx::new(-5, 7, 1, 1)
            }
        );
        assert_eq!(
            fallback_anchor(RectPx::new(-1920, 40, 1920, 1000)),
            Anchor {
                kind: AnchorKind::Fallback,
                rect: RectPx::new(-960, 40, 1, 1)
            }
        );
    }

    #[test]
    fn geometry_scale_from_dpi() {
        assert_eq!(scale_from_dpi(96), Some(1.0));
        assert_eq!(scale_from_dpi(144), Some(1.5));
        assert_eq!(scale_from_dpi(120), Some(1.25));
        assert_eq!(scale_from_dpi(0), None);
    }

    // --- Box layout ---

    #[test]
    fn geometry_box_layout_scale_1() {
        let l = layout(&selection(100, 200, 400, 20), WORK, 1.0);
        // Box = selection + 6; window = box + 14 (left/right/bottom) + 24 (top).
        assert_eq!(l.mode, HaloMode::Box);
        assert_eq!(l.window, RectPx::new(80, 170, 440, 70));
        assert_eq!(l.box_rect, Some(RectPx::new(14, 24, 412, 32)));
        assert_eq!(l.point, PointPx { x: 426, y: 24 });
        assert_eq!(l.badge_side, BadgeSide::Left);
        assert_eq!(l.scale, 1.0);
        assert_box_on_screen(&l, RectPx::new(94, 194, 412, 32));
    }

    #[test]
    fn geometry_box_layout_scale_1_5() {
        // pad 9, side 21, top 36, min width 480.
        let l = layout(&selection(1000, 500, 300, 30), WORK, 1.5);
        assert_eq!(l.window, RectPx::new(850, 455, 480, 105));
        assert_eq!(l.box_rect, Some(RectPx::new(141, 36, 318, 48)));
        assert_eq!(l.point, PointPx { x: 459, y: 36 });
        assert_eq!(l.badge_side, BadgeSide::Left);
        assert_eq!(l.scale, 1.5);
    }

    #[test]
    fn geometry_narrow_box_gets_min_width_to_the_left() {
        let l = layout(&selection(500, 300, 40, 18), WORK, 1.0);
        // Right edge stays at box right + 14; the window grows leftwards to 320 px.
        assert_eq!(l.window, RectPx::new(240, 270, 320, 68));
        assert_eq!(l.window.right(), 546 + 14);
        assert_eq!(l.box_rect, Some(RectPx::new(254, 24, 52, 30)));
        assert_eq!(l.point, PointPx { x: 306, y: 24 });
        assert_eq!(l.badge_side, BadgeSide::Left);
    }

    #[test]
    fn geometry_box_clamped_at_left_edge() {
        let l = layout(&selection(5, 300, 600, 18), WORK, 1.0);
        assert_eq!(l.window, RectPx::new(0, 270, 640, 68));
        assert_eq!(l.box_rect, Some(RectPx::new(-1, 24, 612, 30)));
        assert_eq!(l.point, PointPx { x: 611, y: 24 });
        assert_eq!(l.badge_side, BadgeSide::Left);
        assert_box_on_screen(&l, RectPx::new(-1, 294, 612, 30));
        assert_inside(l.window, WORK);
    }

    #[test]
    fn geometry_narrow_box_at_left_edge_flips_chip_right() {
        // No room for the chip on the left: it grows to the right instead.
        let l = layout(&selection(10, 300, 40, 18), WORK, 1.0);
        assert_eq!(l.badge_side, BadgeSide::Right);
        assert_eq!(l.window, RectPx::new(0, 270, 372, 68));
        assert_eq!(l.point, PointPx { x: 56, y: 24 });
        assert_box_on_screen(&l, RectPx::new(4, 294, 52, 30));
        assert_inside(l.window, WORK);
    }

    #[test]
    fn geometry_box_clamped_at_top_edge() {
        let l = layout(&selection(300, 10, 200, 20), WORK, 1.0);
        assert_eq!(l.window.y, 0);
        assert_eq!(l.window.height, 70); // not shrunk
        assert_eq!(l.box_rect.unwrap().y, 4);
        assert_eq!(l.point.y, 4);
        assert_box_on_screen(&l, RectPx::new(294, 4, 212, 32));
        assert_inside(l.window, WORK);
    }

    #[test]
    fn geometry_box_clamped_at_right_edge() {
        let l = layout(&selection(1800, 500, 110, 20), WORK, 1.0);
        assert_eq!(l.window, RectPx::new(1600, 470, 320, 70));
        assert_eq!(l.box_rect, Some(RectPx::new(194, 24, 122, 32)));
        assert_eq!(l.point, PointPx { x: 316, y: 24 });
        assert_eq!(l.badge_side, BadgeSide::Left);
        assert_inside(l.window, WORK);
    }

    #[test]
    fn geometry_box_clamped_at_bottom_edge() {
        let l = layout(&selection(300, 1030, 200, 20), WORK, 1.0);
        assert_eq!(l.window, RectPx::new(200, 970, 320, 70));
        assert_eq!(l.box_rect.unwrap().y, 54);
        assert_eq!(l.point.y, 54);
        assert_box_on_screen(&l, RectPx::new(294, 1024, 212, 32));
        assert_inside(l.window, WORK);
    }

    #[test]
    fn geometry_box_larger_than_work_area_is_cut_to_it() {
        let l = layout(&selection(-50, -50, 3000, 2000), WORK, 1.0);
        assert_eq!(l.window, WORK);
        assert_box_on_screen(&l, RectPx::new(-56, -56, 3012, 2012));
        // Corner is off-screen to the right and top: the point is kept inside the window.
        assert_eq!(l.point, PointPx { x: 1920, y: 0 });
    }

    // --- Point layout ---

    #[test]
    fn geometry_caret_layout() {
        let l = layout(&caret(500, 400, 1, 20), WORK, 1.0);
        assert_eq!(l.mode, HaloMode::Point);
        assert_eq!(l.box_rect, None);
        assert_eq!(l.window, RectPx::new(493, 378, 340, 44));
        assert_eq!(l.point, PointPx { x: 8, y: 22 });
        assert_eq!(l.badge_side, BadgeSide::Right);
    }

    #[test]
    fn geometry_caret_flips_near_right_edge() {
        let l = layout(&caret(1900, 400, 2, 20), WORK, 1.0);
        assert_eq!(l.badge_side, BadgeSide::Left);
        assert_eq!(l.window, RectPx::new(1570, 378, 340, 44));
        assert_eq!(l.point, PointPx { x: 332, y: 22 });
        assert_inside(l.window, WORK);
    }

    #[test]
    fn geometry_point_clamped_at_top_left_and_bottom() {
        // Pointer at the top-left corner.
        let l = layout(&pointer_anchor(PointPx { x: 0, y: 0 }), WORK, 1.0);
        assert_eq!(l.window, RectPx::new(0, 0, 340, 44));
        assert_eq!(l.point, PointPx { x: 8, y: 22 });
        assert_eq!(l.badge_side, BadgeSide::Right);
        // Pointer on the taskbar below the work area: window shifted up, ring still visible.
        let l = layout(&pointer_anchor(PointPx { x: 800, y: 1060 }), WORK, 1.0);
        assert_eq!(l.window, RectPx::new(793, 996, 340, 44));
        assert_eq!(l.point, PointPx { x: 8, y: 22 });
        assert_inside(l.window, WORK);
    }

    #[test]
    fn geometry_fallback_uses_point_mode() {
        let l = layout(&fallback_anchor(WORK), WORK, 1.0);
        assert_eq!(l.mode, HaloMode::Point);
        assert_eq!(l.window, RectPx::new(953, 0, 340, 44));
        assert_eq!(l.point, PointPx { x: 8, y: 22 });
    }

    #[test]
    fn geometry_negative_coordinates_multi_monitor() {
        // Left monitor at 125 %: pad 8, side 18, top 30, min width 400.
        let work = RectPx::new(-2560, -200, 2560, 1400);
        let l = layout(&selection(-2000, 100, 300, 25), work, 1.25);
        assert_eq!(l.window, RectPx::new(-2074, 62, 400, 89));
        assert_eq!(l.box_rect, Some(RectPx::new(66, 30, 316, 41)));
        assert_eq!(l.point, PointPx { x: 382, y: 30 });
        assert_inside(l.window, work);

        // Caret at the right edge of that monitor (x = 0 is the primary's left edge): flip.
        let l = layout(&caret(-20, 50, 1, 18), work, 1.25);
        assert_eq!(l.badge_side, BadgeSide::Left);
        assert_eq!(l.window, RectPx::new(-434, 22, 425, 55));
        assert_eq!(l.point, PointPx { x: 415, y: 28 });
        assert_inside(l.window, work);
    }

    #[test]
    fn geometry_invalid_scale_falls_back_to_1() {
        let l = layout(&caret(500, 400, 1, 20), WORK, f64::NAN);
        assert_eq!(l.scale, 1.0);
        assert_eq!(l.window, RectPx::new(493, 378, 340, 44));
        assert_eq!(layout(&caret(500, 400, 1, 20), WORK, 0.0).scale, 1.0);
    }

    #[test]
    fn geometry_layout_json_shape() {
        let l = layout(&selection(100, 200, 400, 20), WORK, 1.0);
        let json = serde_json::to_value(l).unwrap();
        assert_eq!(json["mode"], "box");
        assert_eq!(json["badgeSide"], "left");
        assert_eq!(json["boxRect"]["width"], 412);
        assert_eq!(json["window"]["x"], 80);
        assert_eq!(json["point"]["y"], 24);
        let anchor = serde_json::to_value(caret(1, 2, 3, 4)).unwrap();
        assert_eq!(anchor["kind"], "caret");
        assert_eq!(anchor["rect"]["height"], 4);
    }

    /// Live check against the current foreground window (not run in CI):
    /// `cargo test --manifest-path src-tauri/Cargo.toml geometry_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn geometry_live_inspect_foreground() {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let foreground = unsafe { GetForegroundWindow() }.0 as isize;
        let started = Instant::now();
        let (anchor, info) = inspect_target(foreground);
        let elapsed = started.elapsed();
        let again = focus_info(foreground);
        let (work, scale) = monitor_for(anchor.rect.top_right());
        println!("anchor={anchor:?} info={info:?} again={again:?} in {elapsed:?}");
        println!(
            "work={work:?} scale={scale} layout={:?}",
            layout(&anchor, work, scale)
        );
        assert!(elapsed < Duration::from_millis(1500));
        assert!(scale > 0.0 && work.width > 0 && work.height > 0);
        // UIA client (CUIAutomation8 with timeouts) and SAFEARRAY reading on the desktop root.
        {
            let _com = ComGuard::new();
            let client = uia_client().expect("UIA client");
            assert!(client.cast::<IUIAutomation2>().is_ok());
            let root = unsafe { client.GetRootElement() }.expect("root element");
            let mut root_info = FocusInfo::default();
            uia_focus_fields(&root, &mut root_info);
            println!("desktop root: {root_info:?}");
            assert!(root_info.uia_runtime_id.is_some_and(|id| !id.is_empty()));
        }
        // Invalid window: still an anchor, no panic.
        let (anchor, info) = inspect_target(0);
        assert!(matches!(
            anchor.kind,
            AnchorKind::Pointer | AnchorKind::Fallback
        ));
        assert_eq!(info, FocusInfo::default());
    }
}
