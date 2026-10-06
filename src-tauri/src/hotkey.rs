use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LWIN, VK_RCONTROL, VK_RWIN,
        },
        WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK,
            KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    HoldStarted,
    HoldReleased,
    LockStarted,
    LockStopRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyNotification {
    pub event: HotkeyEvent,
    pub reset_epoch: u64,
}

const DOUBLE_TAP_WINDOW_MS: u64 = 450;
const MIN_TAP_GAP_MS: u64 = 50;
const STALE_EVENT_RESET_MS: u64 = 30_000;

static SENDER: OnceLock<Sender<HotkeyNotification>> = OnceLock::new();
static CTRL_LEFT_DOWN: AtomicBool = AtomicBool::new(false);
static CTRL_RIGHT_DOWN: AtomicBool = AtomicBool::new(false);
static WIN_LEFT_DOWN: AtomicBool = AtomicBool::new(false);
static WIN_RIGHT_DOWN: AtomicBool = AtomicBool::new(false);
static MACHINE: OnceLock<Mutex<HotkeyMachine>> = OnceLock::new();
static LAST_EVENT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

pub fn start_listener(sender: Sender<HotkeyNotification>) -> anyhow::Result<()> {
    SENDER
        .set(sender)
        .map_err(|_| anyhow::anyhow!("Keyboard listener is already initialized"))?;
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || unsafe {
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), HINSTANCE::default(), 0)
        {
            Ok(hook) => hook,
            Err(err) => {
                let _ = ready_sender.send(Err(anyhow::anyhow!(
                    "Could not install keyboard hook: {err}"
                )));
                return;
            }
        };
        if ready_sender.send(Ok(())).is_err() {
            let _ = UnhookWindowsHookEx(hook);
            return;
        }
        let mut msg = MSG::default();
        // GetMessage returns -1 on error and zero on quit.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}
        let _ = UnhookWindowsHookEx(hook);
    });
    ready_receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| anyhow::anyhow!("Keyboard listener did not initialize in time"))?
}

pub fn with_current_epoch<T>(epoch: u64, operation: impl FnOnce() -> T) -> Option<T> {
    let machine = MACHINE.get_or_init(|| Mutex::new(HotkeyMachine::new()));
    // The caller may make a brief session-state change here. Credentials,
    // recording-device operations, and app events must remain outside this guard.
    machine.lock().unwrap().with_current_epoch(epoch, operation)
}

pub fn reset_recording_if_epoch(epoch: u64) -> bool {
    let machine = MACHINE.get_or_init(|| Mutex::new(HotkeyMachine::new()));
    machine.lock().unwrap().finish_recording_if_epoch(epoch)
}

pub fn reset_recording_and_discard_pending_if_epoch(
    epoch: u64,
    receiver: &mpsc::Receiver<HotkeyNotification>,
) -> bool {
    let machine = MACHINE.get_or_init(|| Mutex::new(HotkeyMachine::new()));
    // Enqueue and rejection share this mutex, so no event from the discarded
    // machine state can arrive after the queue has been drained.
    machine
        .lock()
        .unwrap()
        .finish_recording_and_discard_pending_if_epoch(epoch, receiver)
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = wparam.0 as u32;
        let info = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = info.vkCode;
        let is_down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;
        let is_up = event == WM_KEYUP || event == WM_SYSKEYUP;
        let now = Instant::now();

        reset_stale_state(now);

        update_key_state(vk, is_down, is_up);

        let (ctrl_down, win_down) = modifier_state();
        let machine = MACHINE.get_or_init(|| Mutex::new(HotkeyMachine::new()));
        let mut machine = machine.lock().unwrap();
        if let Some(event) = machine.update(ctrl_down, win_down, now) {
            // Retain the machine guard through enqueue; rejection resets and
            // discards queued gestures under the same mutex.
            send(&machine, event);
        }
    }

    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

fn update_key_state(vk: u32, is_down: bool, is_up: bool) {
    if !is_down && !is_up {
        return;
    }
    match vk {
        value if value == VK_LCONTROL.0 as u32 => CTRL_LEFT_DOWN.store(is_down, Ordering::SeqCst),
        value if value == VK_RCONTROL.0 as u32 => CTRL_RIGHT_DOWN.store(is_down, Ordering::SeqCst),
        value if value == VK_CONTROL.0 as u32 => {
            CTRL_LEFT_DOWN.store(is_down, Ordering::SeqCst);
            CTRL_RIGHT_DOWN.store(is_down, Ordering::SeqCst);
        }
        value if value == VK_LWIN.0 as u32 => WIN_LEFT_DOWN.store(is_down, Ordering::SeqCst),
        value if value == VK_RWIN.0 as u32 => WIN_RIGHT_DOWN.store(is_down, Ordering::SeqCst),
        _ => {}
    }
}

fn reset_stale_state(now: Instant) {
    let last_event = LAST_EVENT.get_or_init(|| Mutex::new(None));
    let mut last_event = last_event.lock().unwrap();
    if event_gap_is_stale(*last_event, now) {
        resync_modifier_state();
        if let Some(machine) = MACHINE.get() {
            let mut machine = machine.lock().unwrap();
            if let Some(event) = machine.reset_stale_input(modifier_chord_is_down()) {
                send(&machine, event);
            }
        }
    }
    *last_event = Some(now);
}

fn event_gap_is_stale(last_event: Option<Instant>, now: Instant) -> bool {
    last_event.is_some_and(|last| {
        now.saturating_duration_since(last) > Duration::from_millis(STALE_EVENT_RESET_MS)
    })
}

fn resync_modifier_state() {
    // The low-level hook runs before the current event updates async key state.
    // Refresh the preceding physical state, then hook_proc applies that event.
    for (key, state) in [
        (VK_LCONTROL, &CTRL_LEFT_DOWN),
        (VK_RCONTROL, &CTRL_RIGHT_DOWN),
        (VK_LWIN, &WIN_LEFT_DOWN),
        (VK_RWIN, &WIN_RIGHT_DOWN),
    ] {
        state.store(
            unsafe { GetAsyncKeyState(key.0 as i32) } < 0,
            Ordering::SeqCst,
        );
    }
}

fn modifier_chord_is_down() -> bool {
    let (ctrl_down, win_down) = modifier_state();
    ctrl_down && win_down
}

fn modifier_state() -> (bool, bool) {
    let ctrl_down = CTRL_LEFT_DOWN.load(Ordering::SeqCst) || CTRL_RIGHT_DOWN.load(Ordering::SeqCst);
    let win_down = WIN_LEFT_DOWN.load(Ordering::SeqCst) || WIN_RIGHT_DOWN.load(Ordering::SeqCst);
    (ctrl_down, win_down)
}

fn send(machine: &HotkeyMachine, event: HotkeyEvent) {
    if let Some(sender) = SENDER.get() {
        let _ = sender.send(machine.notification(event));
    }
}

#[derive(Debug)]
struct HotkeyMachine {
    reset_epoch: u64,
    both_down: bool,
    recording: bool,
    locked: bool,
    last_release: Option<Instant>,
    full_release_seen: bool,
}

impl HotkeyMachine {
    fn new() -> Self {
        Self {
            reset_epoch: 0,
            both_down: false,
            recording: false,
            locked: false,
            last_release: None,
            full_release_seen: false,
        }
    }

    fn finish_recording(&mut self) {
        self.reset_epoch = self.reset_epoch.wrapping_add(1);
        self.recording = false;
        self.locked = false;
        self.last_release = None;
        self.full_release_seen = false;
    }

    fn notification(&self, event: HotkeyEvent) -> HotkeyNotification {
        HotkeyNotification {
            event,
            reset_epoch: self.reset_epoch,
        }
    }

    fn with_current_epoch<T>(&self, epoch: u64, operation: impl FnOnce() -> T) -> Option<T> {
        if epoch != self.reset_epoch {
            return None;
        }
        Some(operation())
    }

    fn finish_recording_if_epoch(&mut self, epoch: u64) -> bool {
        if epoch != self.reset_epoch {
            return false;
        }
        self.finish_recording();
        true
    }

    fn finish_recording_and_discard_pending_if_epoch(
        &mut self,
        epoch: u64,
        receiver: &mpsc::Receiver<HotkeyNotification>,
    ) -> bool {
        if !self.finish_recording_if_epoch(epoch) {
            return false;
        }
        while receiver.try_recv().is_ok() {}
        true
    }

    fn reset_stale_input(&mut self, physical_chord_down: bool) -> Option<HotkeyEvent> {
        let release_hold = self.recording && self.both_down && !self.locked && !physical_chord_down;
        self.both_down = physical_chord_down;
        self.last_release = None;
        self.full_release_seen = false;
        // Locked recording and genuinely held chords survive keyboard inactivity.
        // Recover a missed release only when the modifiers are no longer held.
        if !self.locked && !physical_chord_down {
            self.recording = false;
        }
        release_hold.then_some(HotkeyEvent::HoldReleased)
    }

    fn update(&mut self, ctrl_down: bool, win_down: bool, now: Instant) -> Option<HotkeyEvent> {
        let both_down = ctrl_down && win_down;
        // The second modifier's release does not change both_down, but must
        // distinguish a genuine double tap from re-pressing only one modifier.
        if !ctrl_down && !win_down && self.recording && !self.locked {
            self.full_release_seen = true;
        }
        if both_down == self.both_down {
            return None;
        }
        self.both_down = both_down;

        if both_down {
            let full_release_seen = std::mem::take(&mut self.full_release_seen);
            if self.locked {
                self.locked = false;
                self.recording = false;
                return Some(HotkeyEvent::LockStopRequested);
            }

            if self.recording {
                if full_release_seen
                    && self
                        .last_release
                        .is_some_and(|last| tap_gap_is_lock(last, now))
                {
                    self.locked = true;
                    return Some(HotkeyEvent::LockStarted);
                }
                self.last_release = None;
                return Some(HotkeyEvent::HoldStarted);
            }

            self.recording = true;
            Some(HotkeyEvent::HoldStarted)
        } else if self.recording && !self.locked {
            self.full_release_seen = !ctrl_down && !win_down;
            self.last_release = Some(now);
            Some(HotkeyEvent::HoldReleased)
        } else {
            None
        }
    }
}

fn tap_gap_is_lock(last_release: Instant, now: Instant) -> bool {
    let gap = now.saturating_duration_since(last_release);
    gap >= Duration::from_millis(MIN_TAP_GAP_MS)
        && gap <= Duration::from_millis(DOUBLE_TAP_WINDOW_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejecting_busy_gesture_discards_newer_queued_lock_but_keeps_fresh_gesture() {
        let machine = std::sync::Arc::new(Mutex::new(HotkeyMachine::new()));
        let (sender, receiver) = mpsc::channel();
        let now = Instant::now();

        {
            let mut machine = machine.lock().unwrap();
            let event = machine.update(true, true, now).unwrap();
            sender.send(machine.notification(event)).unwrap();
        }
        // The worker has dequeued a start that it will reject, while the hook
        // has already queued a newer complete double-tap gesture.
        let rejected = receiver.recv().unwrap();
        assert_eq!(rejected.event, HotkeyEvent::HoldStarted);
        let mut reset_guard = machine.lock().unwrap();
        let release = reset_guard
            .update(false, false, now + Duration::from_millis(80))
            .unwrap();
        sender.send(reset_guard.notification(release)).unwrap();
        let lock = reset_guard
            .update(true, true, now + Duration::from_millis(180))
            .unwrap();
        sender.send(reset_guard.notification(lock)).unwrap();
        assert!(reset_guard.locked);

        let producer_machine = machine.clone();
        let (attempted_sender, attempted_receiver) = mpsc::channel();
        let producer = thread::spawn(move || {
            attempted_sender.send(()).unwrap();
            let mut machine = producer_machine.lock().unwrap();
            assert!(!machine.recording);
            assert!(!machine.locked);
            // A held repeat and release after rejection cannot resurrect the
            // old lock; a genuinely new chord must enqueue a new hold.
            assert!(machine
                .update(true, true, now + Duration::from_millis(200))
                .is_none());
            assert!(machine
                .update(false, false, now + Duration::from_millis(220))
                .is_none());
            let event = machine
                .update(true, true, now + Duration::from_millis(300))
                .unwrap();
            sender.send(machine.notification(event)).unwrap();
        });
        attempted_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();

        assert!(reset_guard
            .finish_recording_and_discard_pending_if_epoch(rejected.reset_epoch, &receiver));
        assert!(receiver.try_recv().is_err());
        drop(reset_guard);
        let fresh = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(fresh.event, HotkeyEvent::HoldStarted);
        assert_ne!(fresh.reset_epoch, rejected.reset_epoch);
        producer.join().unwrap();
        let machine = machine.lock().unwrap();
        assert!(machine.recording);
        assert!(!machine.locked);
        assert_eq!(
            machine.with_current_epoch(fresh.reset_epoch, || true),
            Some(true)
        );
    }

    #[test]
    fn automatic_stop_invalidates_queued_hold_or_lock_before_session_commit() {
        for queued_lock in [false, true] {
            let mut machine = HotkeyMachine::new();
            let (sender, receiver) = mpsc::channel();
            let now = Instant::now();
            let mut event = machine.update(true, true, now).unwrap();
            if queued_lock {
                machine.update(false, false, now + Duration::from_millis(80));
                event = machine
                    .update(true, true, now + Duration::from_millis(180))
                    .unwrap();
            }
            sender.send(machine.notification(event)).unwrap();

            // An automatic stop resets semantics while this start notification
            // remains queued. Dequeueing it must not resurrect a capture.
            machine.finish_recording();
            let stale = receiver.recv().unwrap();
            let mut committed = false;
            assert!(machine
                .with_current_epoch(stale.reset_epoch, || committed = true)
                .is_none());
            assert!(!committed);

            machine.update(false, false, now + Duration::from_millis(220));
            let event = machine
                .update(true, true, now + Duration::from_millis(300))
                .unwrap();
            let fresh = machine.notification(event);
            assert_eq!(fresh.event, HotkeyEvent::HoldStarted);
            assert!(machine
                .with_current_epoch(fresh.reset_epoch, || committed = true)
                .is_some());
            assert!(committed);
        }
    }

    #[test]
    fn stale_failed_start_cannot_reset_or_discard_a_newer_hold_or_lock() {
        for fresh_lock in [false, true] {
            let mut machine = HotkeyMachine::new();
            let (sender, receiver) = mpsc::channel();
            let now = Instant::now();
            let event = machine.update(true, true, now).unwrap();
            let failed_start = machine.notification(event);
            machine.finish_recording();

            machine.update(false, false, now + Duration::from_millis(20));
            let mut event = machine
                .update(true, true, now + Duration::from_millis(30))
                .unwrap();
            if fresh_lock {
                machine.update(false, false, now + Duration::from_millis(80));
                event = machine
                    .update(true, true, now + Duration::from_millis(180))
                    .unwrap();
            }
            let fresh = machine.notification(event);
            sender.send(fresh).unwrap();

            // Credential/audio failure from an older epoch arrives after a new
            // gesture. Neither reset variant may alter state or drain that event.
            assert!(!machine.finish_recording_if_epoch(failed_start.reset_epoch));
            assert!(!machine.finish_recording_and_discard_pending_if_epoch(
                failed_start.reset_epoch,
                &receiver
            ));
            assert_eq!(receiver.recv().unwrap(), fresh);
            assert!(machine.recording);
            assert_eq!(machine.locked, fresh_lock);
            assert_eq!(
                machine.with_current_epoch(fresh.reset_epoch, || true),
                Some(true)
            );

            assert!(machine.finish_recording_if_epoch(fresh.reset_epoch));
            assert!(machine
                .with_current_epoch(fresh.reset_epoch, || true)
                .is_none());
        }
    }

    #[test]
    fn partial_ctrl_release_and_repress_never_locks_recording() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        let sequence = [
            (true, false, 0, None),
            (true, true, 10, Some(HotkeyEvent::HoldStarted)),
            (false, true, 100, Some(HotkeyEvent::HoldReleased)),
            (true, true, 200, Some(HotkeyEvent::HoldStarted)),
            (false, true, 240, Some(HotkeyEvent::HoldReleased)),
            (false, false, 250, None),
        ];
        for (ctrl_down, win_down, milliseconds, expected) in sequence {
            assert_eq!(
                machine.update(
                    ctrl_down,
                    win_down,
                    now + Duration::from_millis(milliseconds)
                ),
                expected
            );
            assert!(!machine.locked);
        }
    }

    #[test]
    fn partial_win_release_and_repeated_represses_never_lock_recording() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        let sequence = [
            (false, true, 0, None),
            (true, true, 10, Some(HotkeyEvent::HoldStarted)),
            (true, false, 100, Some(HotkeyEvent::HoldReleased)),
            (true, true, 200, Some(HotkeyEvent::HoldStarted)),
            (true, false, 240, Some(HotkeyEvent::HoldReleased)),
            (true, true, 340, Some(HotkeyEvent::HoldStarted)),
            (true, false, 380, Some(HotkeyEvent::HoldReleased)),
            (false, false, 390, None),
        ];
        for (ctrl_down, win_down, milliseconds, expected) in sequence {
            assert_eq!(
                machine.update(
                    ctrl_down,
                    win_down,
                    now + Duration::from_millis(milliseconds)
                ),
                expected
            );
            assert!(!machine.locked);
        }
    }

    #[test]
    fn sequential_full_modifier_release_arms_intentional_double_tap() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        let sequence = [
            (true, false, 0, None),
            (true, true, 10, Some(HotkeyEvent::HoldStarted)),
            (false, true, 80, Some(HotkeyEvent::HoldReleased)),
            (false, false, 100, None),
            (false, true, 150, None),
            (true, true, 180, Some(HotkeyEvent::LockStarted)),
            (true, false, 200, None),
            (false, false, 220, None),
            (true, false, 850, None),
            (true, true, 900, Some(HotkeyEvent::LockStopRequested)),
        ];
        for (ctrl_down, win_down, milliseconds, expected) in sequence {
            assert_eq!(
                machine.update(
                    ctrl_down,
                    win_down,
                    now + Duration::from_millis(milliseconds)
                ),
                expected
            );
        }
        assert!(!machine.recording);
        assert!(!machine.locked);
    }

    #[test]
    fn slow_full_release_does_not_extend_double_tap_past_hold_stop_deadline() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        machine.update(false, true, now + Duration::from_millis(80));
        machine.update(false, false, now + Duration::from_millis(500));
        machine.update(true, false, now + Duration::from_millis(550));
        assert_eq!(
            machine.update(true, true, now + Duration::from_millis(600)),
            Some(HotkeyEvent::HoldStarted)
        );
        assert!(!machine.locked);
    }

    #[test]
    fn rejected_hold_or_lock_start_cannot_arm_a_phantom_locked_recording() {
        for reject_locked_start in [false, true] {
            let mut machine = HotkeyMachine::new();
            let now = Instant::now();
            machine.update(true, true, now);
            if reject_locked_start {
                machine.update(false, false, now + Duration::from_millis(80));
                assert_eq!(
                    machine.update(true, true, now + Duration::from_millis(180)),
                    Some(HotkeyEvent::LockStarted)
                );
            }

            // The worker rejects the start while processing another operation.
            machine.finish_recording();
            assert!(machine
                .update(true, true, now + Duration::from_millis(200))
                .is_none());
            assert!(machine
                .update(false, true, now + Duration::from_millis(220))
                .is_none());
            assert!(machine
                .update(false, false, now + Duration::from_millis(230))
                .is_none());
            assert!(!machine.full_release_seen);
            assert_eq!(
                machine.update(true, true, now + Duration::from_millis(300)),
                Some(HotkeyEvent::HoldStarted)
            );
            assert!(!machine.locked);
        }
    }

    #[test]
    fn hold_start_and_release() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        assert!(matches!(
            machine.update(true, true, now),
            Some(HotkeyEvent::HoldStarted)
        ));
        assert!(matches!(
            machine.update(false, false, now + Duration::from_millis(800)),
            Some(HotkeyEvent::HoldReleased)
        ));
    }

    #[test]
    fn double_tap_locks_then_next_press_stops() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        assert!(matches!(
            machine.update(true, true, now),
            Some(HotkeyEvent::HoldStarted)
        ));
        assert!(matches!(
            machine.update(false, false, now + Duration::from_millis(80)),
            Some(HotkeyEvent::HoldReleased)
        ));
        assert!(matches!(
            machine.update(true, true, now + Duration::from_millis(180)),
            Some(HotkeyEvent::LockStarted)
        ));
        assert!(machine
            .update(false, false, now + Duration::from_millis(220))
            .is_none());
        assert!(matches!(
            machine.update(true, true, now + Duration::from_millis(900)),
            Some(HotkeyEvent::LockStopRequested)
        ));
    }

    #[test]
    fn automatic_stop_clears_lock_without_retriggering_held_keys() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        machine.update(false, false, now + Duration::from_millis(80));
        machine.update(true, true, now + Duration::from_millis(180));
        machine.finish_recording();
        assert!(machine
            .update(true, true, now + Duration::from_millis(200))
            .is_none());
        assert!(machine
            .update(false, false, now + Duration::from_millis(220))
            .is_none());
        assert!(matches!(
            machine.update(true, true, now + Duration::from_secs(1)),
            Some(HotkeyEvent::HoldStarted)
        ));
    }

    #[test]
    fn repeated_down_does_not_retrigger() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        assert!(matches!(
            machine.update(true, true, now),
            Some(HotkeyEvent::HoldStarted)
        ));
        assert!(machine
            .update(true, true, now + Duration::from_millis(10))
            .is_none());
    }

    #[test]
    fn next_press_after_double_tap_window_starts_new_hold() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        assert!(matches!(
            machine.update(true, true, now),
            Some(HotkeyEvent::HoldStarted)
        ));
        assert!(matches!(
            machine.update(false, false, now + Duration::from_millis(80)),
            Some(HotkeyEvent::HoldReleased)
        ));
        assert!(matches!(
            machine.update(true, true, now + Duration::from_millis(700)),
            Some(HotkeyEvent::HoldStarted)
        ));
    }

    #[test]
    fn locked_recording_survives_31_seconds_without_keyboard_events() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        machine.update(false, false, now + Duration::from_millis(80));
        assert!(matches!(
            machine.update(true, true, now + Duration::from_millis(180)),
            Some(HotkeyEvent::LockStarted)
        ));
        let last_event = now + Duration::from_millis(220);
        assert!(machine.update(false, false, last_event).is_none());
        let next_press = last_event + Duration::from_secs(31);
        assert!(event_gap_is_stale(Some(last_event), next_press));

        assert!(machine.reset_stale_input(false).is_none());
        assert!(machine.recording);
        assert!(machine.locked);
        assert!(matches!(
            machine.update(true, true, next_press),
            Some(HotkeyEvent::LockStopRequested)
        ));
    }

    #[test]
    fn stale_locked_chord_can_stop_even_when_key_up_was_missed() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        machine.update(false, false, now + Duration::from_millis(80));
        machine.update(true, true, now + Duration::from_millis(180));
        assert!(machine.both_down);

        assert!(machine.reset_stale_input(false).is_none());
        assert!(matches!(
            machine.update(true, true, now + Duration::from_secs(31)),
            Some(HotkeyEvent::LockStopRequested)
        ));
    }

    #[test]
    fn stale_held_chord_releases_capture_once_and_clears_tap_window() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        let release = now + Duration::from_secs(31);
        assert!(event_gap_is_stale(Some(now), release));

        assert!(matches!(
            machine.reset_stale_input(false),
            Some(HotkeyEvent::HoldReleased)
        ));
        // Processing the delayed key-up must not emit another release or make
        // the next chord look like the second tap of a locked recording.
        assert!(machine.update(false, false, release).is_none());
        assert!(machine.reset_stale_input(false).is_none());
        assert!(machine.last_release.is_none());
        assert!(matches!(
            machine.update(true, true, release + Duration::from_millis(100)),
            Some(HotkeyEvent::HoldStarted)
        ));
    }

    #[test]
    fn stale_released_chord_discards_pending_double_tap() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        let release = now + Duration::from_millis(80);
        machine.update(false, false, release);
        assert!(machine.last_release.is_some());
        let next_press = release + Duration::from_secs(31);
        assert!(event_gap_is_stale(Some(release), next_press));

        assert!(machine.reset_stale_input(false).is_none());
        assert!(machine.last_release.is_none());
        assert!(!machine.recording);
        assert!(matches!(
            machine.update(true, true, next_press),
            Some(HotkeyEvent::HoldStarted)
        ));
    }

    #[test]
    fn stale_recovery_after_automatic_stop_does_not_release_inactive_capture() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        machine.finish_recording();
        assert!(machine.both_down);

        assert!(machine.reset_stale_input(true).is_none());
        assert!(machine
            .update(true, true, now + Duration::from_secs(31))
            .is_none());
        assert!(machine
            .update(false, false, now + Duration::from_secs(31))
            .is_none());
        assert!(!machine.recording);
        assert!(!machine.locked);
    }

    #[test]
    fn held_chord_continues_after_31_seconds_and_unrelated_key_event() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        let unrelated_event = now + Duration::from_secs(31);
        assert!(event_gap_is_stale(Some(now), unrelated_event));

        // Resync sees both modifiers still physically held. Applying an
        // unrelated key event leaves that chord unchanged and must not stop it.
        assert!(machine.reset_stale_input(true).is_none());
        assert!(machine.update(true, true, unrelated_event).is_none());
        assert!(machine.recording);
        assert!(!machine.locked);
        assert!(matches!(
            machine.update(false, false, unrelated_event + Duration::from_secs(1)),
            Some(HotkeyEvent::HoldReleased)
        ));
    }

    #[test]
    fn delayed_actual_key_up_after_31_seconds_still_releases_held_capture() {
        let mut machine = HotkeyMachine::new();
        let now = Instant::now();
        machine.update(true, true, now);
        let release = now + Duration::from_secs(31);
        assert!(event_gap_is_stale(Some(now), release));

        // Async key state still reports the held chord before the hook's key-up
        // event is applied, so the ordinary update must deliver its release.
        assert!(machine.reset_stale_input(true).is_none());
        assert!(matches!(
            machine.update(false, false, release),
            Some(HotkeyEvent::HoldReleased)
        ));
        assert!(machine.update(false, false, release).is_none());
    }

    #[test]
    fn fresh_or_missing_event_gap_does_not_require_stale_recovery() {
        let now = Instant::now();
        assert!(!event_gap_is_stale(None, now));
        assert!(!event_gap_is_stale(
            Some(now),
            now + Duration::from_millis(STALE_EVENT_RESET_MS)
        ));
        assert!(event_gap_is_stale(
            Some(now),
            now + Duration::from_millis(STALE_EVENT_RESET_MS + 1)
        ));
    }
}
