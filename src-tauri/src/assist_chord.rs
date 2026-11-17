// `set_sender`, `hold_reached` and `configure` are called by the Text Assist
// worker once it is wired (Task G). Remove this allow when that wiring lands.
#![allow(dead_code)]

//! Shift+Win chord detection for Text Assist.
//!
//! [`AssistChordMachine`] is a pure state machine fed from the low-level keyboard
//! hook in `hotkey.rs`. It runs independently of the Ctrl+Win recording machine
//! and never swallows keys: it only observes them and emits [`AssistChordEvent`]s
//! to the worker registered with [`set_sender`]. Tap and Hold are emitted on full
//! release, so Shift and Win are already up before Vellora injects Ctrl+C.

use std::{
    sync::{mpsc::Sender, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

pub const DEFAULT_HOLD_THRESHOLD_MS: u64 = 350;

const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_LWIN: u8 = 0x5B;
const VK_RWIN: u8 = 0x5C;
const VK_LSHIFT: u8 = 0xA0;
const VK_RSHIFT: u8 = 0xA1;
const VK_LCONTROL: u8 = 0xA2;
const VK_RCONTROL: u8 = 0xA3;
const VK_LMENU: u8 = 0xA4;
const VK_RMENU: u8 = 0xA5;

const SHIFT_KEYS: [u8; 2] = [VK_LSHIFT, VK_RSHIFT];
const WIN_KEYS: [u8; 2] = [VK_LWIN, VK_RWIN];
const CTRL_KEYS: [u8; 2] = [VK_LCONTROL, VK_RCONTROL];
const ALT_KEYS: [u8; 2] = [VK_LMENU, VK_RMENU];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistChordEvent {
    /// Shift+Win both down, no Ctrl/Alt: the worker starts its hold-cue timer.
    Armed { press_id: u64 },
    /// Abandoned: hide any cue.
    Cancelled { press_id: u64 },
    /// Emitted on FULL release, first release < threshold.
    Tap { press_id: u64 },
    /// Emitted on FULL release, first release >= threshold.
    Hold { press_id: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyGroup {
    Shift,
    Win,
    Ctrl,
    Alt,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Armed {
        press_id: u64,
        armed_at: Instant,
        first_release: Option<Instant>,
    },
    /// Shift+Win formed with Ctrl/Alt, or an armed chord was cancelled. Nothing
    /// arms again until no Shift and no Win key is down.
    Blocked,
}

/// Down state of every virtual key (0..=255).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct KeySet([u64; 4]);

impl KeySet {
    fn is_down(&self, vk: u8) -> bool {
        self.0[usize::from(vk >> 6)] & (1 << (vk & 63)) != 0
    }

    fn set(&mut self, vk: u8, down: bool) {
        let word = &mut self.0[usize::from(vk >> 6)];
        if down {
            *word |= 1 << (vk & 63);
        } else {
            *word &= !(1 << (vk & 63));
        }
    }

    fn any_down(&self, keys: &[u8]) -> bool {
        keys.iter().any(|&vk| self.is_down(vk))
    }
}

/// Maps a virtual key to its group and the tracked key bits it changes. Generic
/// Shift/Ctrl/Alt codes change both sides.
fn classify(vk: u32) -> (KeyGroup, &'static [u8]) {
    static OTHER_KEYS: [[u8; 1]; 256] = {
        let mut keys = [[0u8; 1]; 256];
        let mut index = 0;
        while index < 256 {
            keys[index] = [index as u8];
            index += 1;
        }
        keys
    };
    match vk {
        VK_SHIFT => (KeyGroup::Shift, &SHIFT_KEYS),
        VK_CONTROL => (KeyGroup::Ctrl, &CTRL_KEYS),
        VK_MENU => (KeyGroup::Alt, &ALT_KEYS),
        0x5B => (KeyGroup::Win, &WIN_KEYS[..1]),
        0x5C => (KeyGroup::Win, &WIN_KEYS[1..]),
        0xA0 => (KeyGroup::Shift, &SHIFT_KEYS[..1]),
        0xA1 => (KeyGroup::Shift, &SHIFT_KEYS[1..]),
        0xA2 => (KeyGroup::Ctrl, &CTRL_KEYS[..1]),
        0xA3 => (KeyGroup::Ctrl, &CTRL_KEYS[1..]),
        0xA4 => (KeyGroup::Alt, &ALT_KEYS[..1]),
        0xA5 => (KeyGroup::Alt, &ALT_KEYS[1..]),
        0..=255 => (KeyGroup::Other, &OTHER_KEYS[vk as usize]),
        // Not a real virtual key: never tracked, so never treated as a repeat.
        _ => (KeyGroup::Other, &[]),
    }
}

fn group_keys(group: KeyGroup) -> &'static [u8] {
    match group {
        KeyGroup::Shift => &SHIFT_KEYS,
        KeyGroup::Win => &WIN_KEYS,
        KeyGroup::Ctrl => &CTRL_KEYS,
        KeyGroup::Alt => &ALT_KEYS,
        KeyGroup::Other => &[],
    }
}

#[derive(Debug)]
pub struct AssistChordMachine {
    keys: KeySet,
    phase: Phase,
    last_press_id: u64,
    hold_threshold: Duration,
    enabled: bool,
}

impl AssistChordMachine {
    pub fn new(hold_threshold: Duration) -> Self {
        Self {
            keys: KeySet::default(),
            phase: Phase::Idle,
            last_press_id: 0,
            hold_threshold,
            enabled: true,
        }
    }

    /// Disabling resets to idle silently. Key bookkeeping continues while
    /// disabled, so re-enabling with keys held never arms a half-seen chord.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.phase = Phase::Idle;
        }
    }

    pub fn set_hold_threshold(&mut self, threshold: Duration) {
        self.hold_threshold = threshold;
    }

    /// One key event that Vellora did not inject. `down` false means key-up.
    pub fn update(&mut self, vk: u32, down: bool, now: Instant) -> Option<AssistChordEvent> {
        let (group, keys) = classify(vk);
        if down {
            self.key_down(group, keys, now)
        } else {
            self.key_up(group, keys, now)
        }
    }

    fn key_down(&mut self, group: KeyGroup, keys: &[u8], now: Instant) -> Option<AssistChordEvent> {
        // Auto-repeat: a key-down for a key already down is ignored in every phase.
        if !keys.is_empty() && keys.iter().all(|&vk| self.keys.is_down(vk)) {
            return None;
        }
        let chord_before = self.chord_down();
        for &vk in keys {
            self.keys.set(vk, true);
        }
        if !self.enabled {
            return None;
        }
        match self.phase {
            Phase::Idle => {
                if chord_before || !self.chord_down() {
                    return None;
                }
                if self.ctrl_or_alt_down() {
                    self.phase = Phase::Blocked;
                    return None;
                }
                self.last_press_id = self.last_press_id.wrapping_add(1);
                self.phase = Phase::Armed {
                    press_id: self.last_press_id,
                    armed_at: now,
                    first_release: None,
                };
                Some(AssistChordEvent::Armed {
                    press_id: self.last_press_id,
                })
            }
            Phase::Armed { press_id, .. } => match group {
                // The other-side Shift/Win (or re-pressing one) is not a cancel.
                KeyGroup::Shift | KeyGroup::Win => None,
                KeyGroup::Ctrl | KeyGroup::Alt | KeyGroup::Other => {
                    self.phase = Phase::Blocked;
                    Some(AssistChordEvent::Cancelled { press_id })
                }
            },
            Phase::Blocked => None,
        }
    }

    fn key_up(&mut self, group: KeyGroup, keys: &[u8], now: Instant) -> Option<AssistChordEvent> {
        let is_chord_key = matches!(group, KeyGroup::Shift | KeyGroup::Win);
        if is_chord_key && !keys.iter().any(|&vk| self.keys.is_down(vk)) {
            // A Shift/Win key-up for a side not tracked as down means the side
            // bookkeeping is wrong (missed event or a resync side guess). Clear
            // the whole group so a phantom key can never arm a later Win tap.
            for &vk in group_keys(group) {
                self.keys.set(vk, false);
            }
        }
        for &vk in keys {
            self.keys.set(vk, false);
        }
        if !self.enabled || !is_chord_key {
            return None;
        }
        let released = !self.any_shift() && !self.any_win();
        match &mut self.phase {
            Phase::Armed {
                press_id,
                armed_at,
                first_release,
            } => {
                let first_release = *first_release.get_or_insert(now);
                if !released {
                    return None;
                }
                let press_id = *press_id;
                let held = first_release.saturating_duration_since(*armed_at);
                self.phase = Phase::Idle;
                Some(if held >= self.hold_threshold {
                    AssistChordEvent::Hold { press_id }
                } else {
                    AssistChordEvent::Tap { press_id }
                })
            }
            Phase::Blocked => {
                if released {
                    self.phase = Phase::Idle;
                }
                None
            }
            Phase::Idle => None,
        }
    }

    /// After a long input gap: physical modifier state from GetAsyncKeyState.
    ///
    /// A modifier reported up clears both sides. A modifier reported down keeps
    /// the sides already tracked; if none was tracked, the left side is assumed
    /// (a later key-up of the right side clears the group, see `key_up`).
    /// Non-modifier keys are cleared, since their key-ups may have been missed.
    pub fn resync(
        &mut self,
        shift: bool,
        win: bool,
        ctrl: bool,
        alt: bool,
    ) -> Option<AssistChordEvent> {
        let modifiers = [
            (SHIFT_KEYS, shift),
            (WIN_KEYS, win),
            (CTRL_KEYS, ctrl),
            (ALT_KEYS, alt),
        ];
        let mut synced = KeySet::default();
        for (keys, down) in modifiers {
            if !down {
                continue;
            }
            if self.keys.any_down(&keys) {
                for vk in keys {
                    synced.set(vk, self.keys.is_down(vk));
                }
            } else {
                synced.set(keys[0], true);
            }
        }
        self.keys = synced;

        if !self.enabled {
            self.phase = Phase::Idle;
            return None;
        }
        let shift_or_win = self.any_shift() || self.any_win();
        match self.phase {
            Phase::Armed { press_id, .. } => {
                if self.chord_down() && !self.ctrl_or_alt_down() {
                    return None;
                }
                self.phase = if shift_or_win {
                    Phase::Blocked
                } else {
                    Phase::Idle
                };
                Some(AssistChordEvent::Cancelled { press_id })
            }
            Phase::Blocked => {
                if !shift_or_win {
                    self.phase = Phase::Idle;
                }
                None
            }
            Phase::Idle => None,
        }
    }

    /// For the worker's hold-cue timer: still armed, no key released yet, threshold reached.
    pub fn hold_reached(&self, press_id: u64, now: Instant) -> bool {
        match self.phase {
            Phase::Armed {
                press_id: armed_id,
                armed_at,
                first_release: None,
            } => {
                armed_id == press_id
                    && now.saturating_duration_since(armed_at) >= self.hold_threshold
            }
            _ => false,
        }
    }

    fn any_shift(&self) -> bool {
        self.keys.any_down(&SHIFT_KEYS)
    }

    fn any_win(&self) -> bool {
        self.keys.any_down(&WIN_KEYS)
    }

    fn chord_down(&self) -> bool {
        self.any_shift() && self.any_win()
    }

    fn ctrl_or_alt_down(&self) -> bool {
        self.keys.any_down(&CTRL_KEYS) || self.keys.any_down(&ALT_KEYS)
    }
}

static SENDER: OnceLock<Sender<AssistChordEvent>> = OnceLock::new();
static MACHINE: OnceLock<Mutex<AssistChordMachine>> = OnceLock::new();

fn machine() -> MutexGuard<'static, AssistChordMachine> {
    let machine = MACHINE.get_or_init(|| {
        Mutex::new(AssistChordMachine::new(Duration::from_millis(
            DEFAULT_HOLD_THRESHOLD_MS,
        )))
    });
    // The hook callback must never panic; the machine stays consistent even if
    // a holder panicked, because every update is a single small mutation.
    machine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn dispatch(event: Option<AssistChordEvent>) {
    if let (Some(event), Some(sender)) = (event, SENDER.get()) {
        let _ = sender.send(event);
    }
}

/// Global glue used by hotkey.rs. Events go to the sender registered with set_sender;
/// until then they are dropped.
pub fn set_sender(sender: Sender<AssistChordEvent>) -> anyhow::Result<()> {
    SENDER
        .set(sender)
        .map_err(|_| anyhow::anyhow!("Assist chord sender is already initialized"))
}

/// Called from the keyboard hook for every event Vellora did not inject.
pub fn on_hook_key(vk: u32, down: bool, up: bool, now: Instant) {
    if down == up {
        return;
    }
    let event = machine().update(vk, down, now);
    dispatch(event);
}

/// Called from the keyboard hook after a long input gap, with the physical
/// modifier state preceding the current event.
pub fn on_stale_resync(shift: bool, win: bool, ctrl: bool, alt: bool) {
    let event = machine().resync(shift, win, ctrl, alt);
    dispatch(event);
}

pub fn hold_reached(press_id: u64, now: Instant) -> bool {
    machine().hold_reached(press_id, now)
}

pub fn configure(enabled: bool, hold_threshold: Duration) {
    let mut machine = machine();
    machine.set_hold_threshold(hold_threshold);
    machine.set_enabled(enabled);
}

#[cfg(test)]
mod tests {
    use super::*;
    use AssistChordEvent::{Armed, Cancelled, Hold, Tap};

    const LSHIFT: u32 = 0xA0;
    const RSHIFT: u32 = 0xA1;
    const SHIFT: u32 = 0x10;
    const LWIN: u32 = 0x5B;
    const RWIN: u32 = 0x5C;
    const LCTRL: u32 = 0xA2;
    const RCTRL: u32 = 0xA3;
    const LALT: u32 = 0xA4;
    const RALT: u32 = 0xA5;
    const KEY_S: u32 = 0x53;
    const KEY_A: u32 = 0x41;
    const LEFT_ARROW: u32 = 0x25;

    const DOWN: bool = true;
    const UP: bool = false;

    fn machine() -> AssistChordMachine {
        AssistChordMachine::new(Duration::from_millis(DEFAULT_HOLD_THRESHOLD_MS))
    }

    /// Feeds (vk, down, ms, expected) steps and checks every emitted event.
    fn run(
        machine: &mut AssistChordMachine,
        start: Instant,
        steps: &[(u32, bool, u64, Option<AssistChordEvent>)],
    ) {
        for (index, &(vk, down, ms, expected)) in steps.iter().enumerate() {
            assert_eq!(
                machine.update(vk, down, start + Duration::from_millis(ms)),
                expected,
                "step {index}: vk {vk:#x} down {down} at {ms} ms"
            );
        }
    }

    #[test]
    fn shift_then_win_tap() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                (LWIN, UP, 100, None),
                (LSHIFT, UP, 120, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn win_then_shift_arms_and_taps() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LWIN, DOWN, 0, None),
                (LSHIFT, DOWN, 10, Some(Armed { press_id: 1 })),
                (LSHIFT, UP, 100, None),
                (LWIN, UP, 120, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn hold_is_emitted_on_full_release() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                (LWIN, UP, 900, None),
                (LSHIFT, UP, 950, Some(Hold { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn exact_threshold_counts_as_hold_and_one_ms_less_as_tap() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 100, Some(Armed { press_id: 1 })),
                (LWIN, UP, 450, None),
                (LSHIFT, UP, 460, Some(Hold { press_id: 1 })),
                (LSHIFT, DOWN, 1000, None),
                (LWIN, DOWN, 1100, Some(Armed { press_id: 2 })),
                (LSHIFT, UP, 1449, None),
                (LWIN, UP, 1460, Some(Tap { press_id: 2 })),
            ],
        );
    }

    #[test]
    fn duration_is_measured_to_the_first_release_not_the_full_release() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 0, Some(Armed { press_id: 1 })),
                (LSHIFT, UP, 100, None),
                (LWIN, UP, 2000, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn left_right_mixes_arm() {
        for (shift, win) in [
            (LSHIFT, RWIN),
            (RSHIFT, LWIN),
            (RSHIFT, RWIN),
            (SHIFT, LWIN),
        ] {
            let mut m = machine();
            run(
                &mut m,
                Instant::now(),
                &[
                    (shift, DOWN, 0, None),
                    (win, DOWN, 10, Some(Armed { press_id: 1 })),
                    (win, UP, 50, None),
                    (shift, UP, 60, Some(Tap { press_id: 1 })),
                ],
            );
        }
    }

    #[test]
    fn other_side_shift_or_win_while_armed_is_not_a_cancel_and_must_also_release() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                (RSHIFT, DOWN, 20, None),
                (RWIN, DOWN, 30, None),
                (LSHIFT, UP, 100, None),
                (LWIN, UP, 110, None),
                (RWIN, UP, 120, None),
                (RSHIFT, UP, 130, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn generic_shift_sets_and_clears_both_sides() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (SHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                // The generic key-down marked both sides; a left key-up alone
                // leaves the right side down.
                (LSHIFT, UP, 50, None),
                (LWIN, UP, 60, None),
                (SHIFT, UP, 70, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn repeats_are_ignored_in_every_phase() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                // Idle: repeats do not arm twice or change anything.
                (LSHIFT, DOWN, 0, None),
                (LSHIFT, DOWN, 30, None),
                (LWIN, DOWN, 40, Some(Armed { press_id: 1 })),
                // Armed: repeats of either key do not cancel or re-arm.
                (LWIN, DOWN, 70, None),
                (LSHIFT, DOWN, 100, None),
                (LWIN, UP, 150, None),
                (LSHIFT, DOWN, 160, None),
                (LSHIFT, UP, 170, Some(Tap { press_id: 1 })),
            ],
        );
        // Blocked: Ctrl repeats do nothing, and releasing still unblocks.
        run(
            &mut m,
            Instant::now(),
            &[
                (LCTRL, DOWN, 0, None),
                (LSHIFT, DOWN, 10, None),
                (LWIN, DOWN, 20, None),
                (LCTRL, DOWN, 50, None),
                (LWIN, DOWN, 60, None),
                (LCTRL, UP, 70, None),
                (LWIN, UP, 80, None),
                (LSHIFT, UP, 90, None),
                (LSHIFT, DOWN, 200, None),
                (LWIN, DOWN, 210, Some(Armed { press_id: 2 })),
            ],
        );
    }

    #[test]
    fn repeat_of_a_key_held_before_arming_does_not_cancel() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (KEY_A, DOWN, 0, None),
                (LSHIFT, DOWN, 10, None),
                (LWIN, DOWN, 20, Some(Armed { press_id: 1 })),
                (KEY_A, DOWN, 50, None),
                (KEY_A, UP, 60, None),
                (LWIN, UP, 100, None),
                (LSHIFT, UP, 110, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn other_key_cancels_and_blocks_until_full_release() {
        for other in [KEY_S, LEFT_ARROW, 0x4D] {
            let mut m = machine();
            run(
                &mut m,
                Instant::now(),
                &[
                    (LWIN, DOWN, 0, None),
                    (LSHIFT, DOWN, 10, Some(Armed { press_id: 1 })),
                    (other, DOWN, 50, Some(Cancelled { press_id: 1 })),
                    (other, UP, 60, None),
                    // Releasing the other key does not re-arm.
                    (KEY_A, DOWN, 70, None),
                    (LSHIFT, UP, 100, None),
                    // Re-pressing Shift with Win still down stays blocked.
                    (LSHIFT, DOWN, 110, None),
                    (LSHIFT, UP, 120, None),
                    (LWIN, UP, 130, None),
                    (LWIN, DOWN, 200, None),
                    (LSHIFT, DOWN, 210, Some(Armed { press_id: 2 })),
                ],
            );
        }
    }

    #[test]
    fn other_key_after_first_release_still_cancels() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                (LWIN, UP, 50, None),
                (KEY_S, DOWN, 60, Some(Cancelled { press_id: 1 })),
                (KEY_S, UP, 70, None),
                (LSHIFT, UP, 80, None),
            ],
        );
    }

    #[test]
    fn unrelated_key_up_is_ignored_while_armed() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                (KEY_A, UP, 20, None),
                (LCTRL, UP, 30, None),
                (LWIN, UP, 50, None),
                (LSHIFT, UP, 60, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn ctrl_or_alt_while_armed_cancels() {
        for modifier in [LCTRL, RCTRL, 0x11, LALT, RALT, 0x12] {
            let mut m = machine();
            run(
                &mut m,
                Instant::now(),
                &[
                    (LSHIFT, DOWN, 0, None),
                    (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                    (modifier, DOWN, 50, Some(Cancelled { press_id: 1 })),
                    (modifier, UP, 60, None),
                    (LWIN, UP, 70, None),
                    (LSHIFT, UP, 80, None),
                ],
            );
        }
    }

    #[test]
    fn ctrl_or_alt_shift_win_is_blocked_and_releasing_the_modifier_first_does_not_arm() {
        for modifier in [LCTRL, RCTRL, LALT, RALT] {
            for chord_order in [[LSHIFT, LWIN], [LWIN, LSHIFT]] {
                let mut m = machine();
                run(
                    &mut m,
                    Instant::now(),
                    &[
                        (modifier, DOWN, 0, None),
                        (chord_order[0], DOWN, 10, None),
                        (chord_order[1], DOWN, 20, None),
                        (modifier, UP, 50, None),
                        // Re-pressing either chord key while the other is still
                        // down must not arm either.
                        (chord_order[0], UP, 60, None),
                        (chord_order[0], DOWN, 70, None),
                        (chord_order[1], UP, 100, None),
                        (chord_order[0], UP, 110, None),
                        (LSHIFT, DOWN, 200, None),
                        (LWIN, DOWN, 210, Some(Armed { press_id: 1 })),
                    ],
                );
            }
        }
    }

    #[test]
    fn ctrl_released_before_chord_forms_arms_normally() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LCTRL, DOWN, 0, None),
                (LSHIFT, DOWN, 10, None),
                (LCTRL, UP, 20, None),
                (LWIN, DOWN, 30, Some(Armed { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn disabled_machine_emits_nothing() {
        let mut m = machine();
        m.set_enabled(false);
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, None),
                (LWIN, UP, 50, None),
                (LSHIFT, UP, 60, None),
            ],
        );
        assert!(!m.hold_reached(1, Instant::now() + Duration::from_secs(5)));
    }

    #[test]
    fn disabling_while_armed_resets_silently_and_reenabling_needs_a_new_press() {
        let mut m = machine();
        let start = Instant::now();
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
            ],
        );
        m.set_enabled(false);
        assert!(!m.hold_reached(1, start + Duration::from_secs(1)));
        m.set_enabled(true);
        run(
            &mut m,
            start,
            &[
                // Keys were tracked while disabled: the held chord does not arm,
                // and its release emits nothing.
                (RSHIFT, DOWN, 20, None),
                (LWIN, UP, 50, None),
                (RSHIFT, UP, 55, None),
                (LSHIFT, UP, 60, None),
                (LSHIFT, DOWN, 200, None),
                (LWIN, DOWN, 210, Some(Armed { press_id: 2 })),
            ],
        );
    }

    #[test]
    fn resync_with_chord_released_cancels_to_idle() {
        let mut m = machine();
        let start = Instant::now();
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
            ],
        );
        assert_eq!(
            m.resync(false, false, false, false),
            Some(Cancelled { press_id: 1 })
        );
        run(
            &mut m,
            start,
            &[
                // The delayed key-ups emit nothing; a fresh press arms again.
                (LWIN, UP, 31_000, None),
                (LSHIFT, UP, 31_010, None),
                (LSHIFT, DOWN, 31_100, None),
                (LWIN, DOWN, 31_110, Some(Armed { press_id: 2 })),
            ],
        );
    }

    #[test]
    fn resync_with_one_chord_key_still_down_cancels_to_blocked() {
        let mut m = machine();
        let start = Instant::now();
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
            ],
        );
        assert_eq!(
            m.resync(false, true, false, false),
            Some(Cancelled { press_id: 1 })
        );
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 31_000, None),
                (LSHIFT, UP, 31_010, None),
                (LWIN, UP, 31_020, None),
                (LSHIFT, DOWN, 31_100, None),
                (LWIN, DOWN, 31_110, Some(Armed { press_id: 2 })),
            ],
        );
    }

    #[test]
    fn resync_with_chord_still_down_keeps_it_armed() {
        let mut m = machine();
        let start = Instant::now();
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 0, None),
                (RWIN, DOWN, 10, Some(Armed { press_id: 1 })),
            ],
        );
        assert_eq!(m.resync(true, true, false, false), None);
        run(
            &mut m,
            start,
            &[
                (RWIN, UP, 100, None),
                (LSHIFT, UP, 110, Some(Tap { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn resync_with_ctrl_or_alt_down_cancels_armed_chord() {
        for (ctrl, alt) in [(true, false), (false, true)] {
            let mut m = machine();
            let start = Instant::now();
            run(
                &mut m,
                start,
                &[
                    (LSHIFT, DOWN, 0, None),
                    (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                ],
            );
            assert_eq!(
                m.resync(true, true, ctrl, alt),
                Some(Cancelled { press_id: 1 })
            );
            run(
                &mut m,
                start,
                &[(LWIN, UP, 100, None), (LSHIFT, UP, 110, None)],
            );
        }
    }

    #[test]
    fn resync_clears_a_stale_shift_so_a_later_win_tap_does_not_arm() {
        let mut m = machine();
        let start = Instant::now();
        // Shift's key-up was missed (for example on the secure desktop).
        run(&mut m, start, &[(LSHIFT, DOWN, 0, None)]);
        assert_eq!(m.resync(false, false, false, false), None);
        run(
            &mut m,
            start,
            &[(LWIN, DOWN, 31_000, None), (LWIN, UP, 31_050, None)],
        );
    }

    #[test]
    fn resync_side_guess_is_corrected_by_the_real_key_up() {
        let mut m = machine();
        let start = Instant::now();
        // Physically Right Shift is held, but nothing was tracked: the resync
        // assumes the left side. Right Shift's key-up must still clear Shift.
        assert_eq!(m.resync(true, false, false, false), None);
        run(
            &mut m,
            start,
            &[
                (RSHIFT, UP, 10, None),
                (LWIN, DOWN, 20, None),
                (LWIN, UP, 30, None),
                (LSHIFT, DOWN, 100, None),
                (LWIN, DOWN, 110, Some(Armed { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn resync_unblocks_when_chord_keys_are_released() {
        let mut m = machine();
        let start = Instant::now();
        run(
            &mut m,
            start,
            &[
                (LCTRL, DOWN, 0, None),
                (LSHIFT, DOWN, 10, None),
                (LWIN, DOWN, 20, None),
            ],
        );
        assert_eq!(m.resync(false, false, false, false), None);
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 31_000, None),
                (LWIN, DOWN, 31_010, Some(Armed { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn hold_reached_only_while_armed_unreleased_and_past_threshold() {
        let mut m = machine();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        assert!(!m.hold_reached(1, at(1000)));
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 100, Some(Armed { press_id: 1 })),
            ],
        );
        assert!(!m.hold_reached(1, at(449)));
        assert!(m.hold_reached(1, at(450)));
        assert!(!m.hold_reached(2, at(450)));
        assert!(!m.hold_reached(0, at(450)));
        // After the first release the cue no longer applies.
        run(&mut m, start, &[(LWIN, UP, 500, None)]);
        assert!(!m.hold_reached(1, at(600)));
        run(
            &mut m,
            start,
            &[(LSHIFT, UP, 510, Some(Hold { press_id: 1 }))],
        );
        assert!(!m.hold_reached(1, at(600)));

        // A cancelled press never reaches hold.
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 1000, None),
                (LWIN, DOWN, 1000, Some(Armed { press_id: 2 })),
                (KEY_S, DOWN, 1100, Some(Cancelled { press_id: 2 })),
            ],
        );
        assert!(!m.hold_reached(2, at(2000)));
    }

    #[test]
    fn hold_threshold_can_be_changed() {
        let mut m = machine();
        m.set_hold_threshold(Duration::from_millis(100));
        let start = Instant::now();
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 0, Some(Armed { press_id: 1 })),
            ],
        );
        assert!(m.hold_reached(1, start + Duration::from_millis(100)));
        run(
            &mut m,
            start,
            &[
                (LWIN, UP, 150, None),
                (LSHIFT, UP, 160, Some(Hold { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn press_id_increments_for_every_arm_including_cancelled_ones() {
        let mut m = machine();
        let start = Instant::now();
        run(
            &mut m,
            start,
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                (LCTRL, DOWN, 20, Some(Cancelled { press_id: 1 })),
                (LCTRL, UP, 30, None),
                (LWIN, UP, 40, None),
                (LSHIFT, UP, 50, None),
                (LWIN, DOWN, 100, None),
                (LSHIFT, DOWN, 110, Some(Armed { press_id: 2 })),
                (LSHIFT, UP, 150, None),
                (LWIN, UP, 160, Some(Tap { press_id: 2 })),
                (LSHIFT, DOWN, 200, None),
                (LWIN, DOWN, 210, Some(Armed { press_id: 3 })),
            ],
        );
    }

    #[test]
    fn out_of_range_virtual_key_is_an_other_key() {
        let mut m = machine();
        run(
            &mut m,
            Instant::now(),
            &[
                (LSHIFT, DOWN, 0, None),
                (LWIN, DOWN, 10, Some(Armed { press_id: 1 })),
                (0x1_0000, DOWN, 20, Some(Cancelled { press_id: 1 })),
            ],
        );
    }

    #[test]
    fn global_glue_routes_events_to_the_registered_sender() {
        // The only test touching the global machine and sender.
        let (sender, receiver) = std::sync::mpsc::channel();
        set_sender(sender).unwrap();
        assert!(set_sender(std::sync::mpsc::channel().0).is_err());
        configure(true, Duration::from_millis(DEFAULT_HOLD_THRESHOLD_MS));
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        on_hook_key(LSHIFT, true, false, at(0));
        on_hook_key(LWIN, true, false, at(10));
        // Neither down nor up (unknown message) is ignored.
        on_hook_key(KEY_S, false, false, at(20));
        assert!(hold_reached(1, at(400)));
        on_hook_key(LWIN, false, true, at(500));
        on_hook_key(LSHIFT, false, true, at(510));
        on_hook_key(LSHIFT, true, false, at(600));
        on_hook_key(LWIN, true, false, at(610));
        on_stale_resync(false, false, false, false);
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            vec![
                Armed { press_id: 1 },
                Hold { press_id: 1 },
                Armed { press_id: 2 },
                Cancelled { press_id: 2 },
            ]
        );
    }
}
