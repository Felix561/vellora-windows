//! Text Assist runtime: the chord worker thread, engine selection, the halo presenter and
//! history recording. `main.rs` only calls [`start`] and [`apply_settings`].

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};
use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

use super::{
    geometry::{self, Anchor, AnchorKind, HaloLayout, PointPx, RectPx},
    job::{self, JobObserver, JobUpdate, Phase, PhaseCell, PolishJob},
    panel,
    platform::{TargetSnapshot, WindowsPlatform},
};
use crate::{
    assist_chord::{self, AssistChordEvent},
    history::{self, TextAssistRun, OUTCOME_KEPT},
    text_providers::{
        catalog, factory::EngineFactory, offline::UppercaseEngine, CancelToken, ModelRef,
        ProviderKind, TextEngine, TextError,
    },
    types::AppSettings,
};

pub const HALO_WINDOW: &str = "assist-halo";
pub const HALO_EVENT: &str = "text-assist-halo";
/// Emitted to the main window after a run was saved, so History can refresh.
pub const RUN_RECORDED_EVENT: &str = "text-assist-run-recorded";
const PARK: PhysicalPosition<i32> = PhysicalPosition {
    x: -32000,
    y: -32000,
};

pub(crate) type SettingsSource = Arc<dyn Fn() -> AppSettings + Send + Sync>;
/// Applies a change to the saved settings (persisted and announced to the main window).
pub(crate) type SettingsUpdate = Arc<dyn Fn(&dyn Fn(&mut AppSettings)) + Send + Sync>;

/// Halo payload; mirrors `HaloPayload` in `src/lib/halo.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HaloPayload {
    job_id: String,
    phase: &'static str,
    kind: &'static str,
    mode: geometry::HaloMode,
    color: String,
    label: String,
    progress: Option<f32>,
    #[serde(rename = "box")]
    box_rect: Option<RectPx>,
    point: PointPx,
    badge_side: geometry::BadgeSide,
    scale: f64,
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Capturing => "capturing",
        Phase::Thinking => "thinking",
        Phase::Streaming => "streaming",
        Phase::Checking => "checking",
        Phase::Replacing => "replacing",
        Phase::Done => "done",
        Phase::NoChange => "no_change",
        Phase::Kept => "kept",
        Phase::Failed => "failed",
        Phase::Cancelled => "cancelled",
    }
}

/// How long a final state stays visible before the halo hides.
fn linger(phase: Phase) -> Duration {
    match phase {
        Phase::Done | Phase::NoChange => Duration::from_millis(2_200),
        Phase::Cancelled => Duration::from_millis(1_600),
        _ => Duration::from_millis(4_500),
    }
}

fn pointer_anchor() -> Anchor {
    let mut point = POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    Anchor {
        kind: AnchorKind::Pointer,
        rect: RectPx {
            x: point.x,
            y: point.y,
            width: 1,
            height: 1,
        },
    }
}

fn layout_for(anchor: &Anchor) -> HaloLayout {
    let center = PointPx {
        x: anchor.rect.x + anchor.rect.width / 2,
        y: anchor.rect.y + anchor.rect.height / 2,
    };
    let (work_area, scale) = geometry::monitor_for(center);
    geometry::layout(anchor, work_area, scale)
}

/// Shared halo window control. A generation counter keeps stale hide timers from hiding a
/// newer job's halo.
#[derive(Clone)]
pub(crate) struct Halo {
    pub(crate) app: tauri::AppHandle,
    generation: Arc<AtomicU64>,
    last_window: Arc<Mutex<Option<RectPx>>>,
}

impl Halo {
    fn show(&self, layout: &HaloLayout, payload: HaloPayload) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(window) = self.app.get_webview_window(HALO_WINDOW) {
            let mut last = self.last_window.lock().unwrap();
            if last.is_none() {
                // First update of a job: lift the halo above every window, including other
                // always-on-top windows that appeared since (without activating it).
                let _ = window.set_always_on_top(true);
            }
            if *last != Some(layout.window) {
                // Moving/resizing never activates the window; it stays shown and click-through.
                let _ =
                    window.set_position(PhysicalPosition::new(layout.window.x, layout.window.y));
                let _ = window.set_size(PhysicalSize::new(
                    layout.window.width.max(1) as u32,
                    layout.window.height.max(1) as u32,
                ));
                *last = Some(layout.window);
            }
        }
        let _ = self.app.emit_to(HALO_WINDOW, HALO_EVENT, payload);
        generation
    }

    pub(crate) fn hide_now(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.park();
    }

    fn park(&self) {
        let _ = self.app.emit_to(
            HALO_WINDOW,
            HALO_EVENT,
            serde_json::json!({ "jobId": "", "phase": "hidden" }),
        );
        if let Some(window) = self.app.get_webview_window(HALO_WINDOW) {
            let _ = window.set_position(PARK);
            *self.last_window.lock().unwrap() = None;
        }
    }

    /// Hides after `delay` unless another update arrived meanwhile.
    fn hide_later(&self, generation: u64, delay: Duration) {
        let halo = self.clone();
        thread::spawn(move || {
            thread::sleep(delay);
            if halo.generation.load(Ordering::SeqCst) == generation {
                halo.park();
            }
        });
    }
}

/// Presents one job's updates on the halo and records its history row.
struct Presenter {
    halo: Halo,
    color: String,
    save_history: bool,
    last_kept: Arc<Mutex<Option<TextAssistRun>>>,
}

impl JobObserver<TargetSnapshot> for Presenter {
    fn update(&self, target: Option<&TargetSnapshot>, update: &JobUpdate) {
        let anchor = target.map_or_else(pointer_anchor, |target| target.anchor);
        let layout = layout_for(&anchor);
        #[cfg(debug_assertions)]
        if update.phase == Phase::Capturing {
            // Geometry diagnostics for development builds only; contains no text.
            eprintln!("text-assist anchor={anchor:?} layout={layout:?}");
        }
        let payload = HaloPayload {
            job_id: update.job_id.clone(),
            phase: phase_name(update.phase),
            kind: "polish",
            mode: layout.mode,
            color: self.color.clone(),
            label: update.label.clone(),
            progress: update.progress,
            box_rect: layout.box_rect,
            point: layout.point,
            badge_side: layout.badge_side,
            scale: layout.scale,
        };
        let generation = self.halo.show(&layout, payload);
        if update.phase.is_final() {
            self.halo.hide_later(generation, linger(update.phase));
        }
    }

    fn record(&self, run: TextAssistRun) {
        if run.outcome == OUTCOME_KEPT {
            *self.last_kept.lock().unwrap() = Some(run.clone());
        }
        if self.save_history {
            if let Err(err) = history::insert_text_assist_run(&run) {
                eprintln!("Could not save Text Assist history: {err:#}");
            }
            let _ = self.halo.app.emit_to("main", RUN_RECORDED_EVENT, &run.id);
        }
    }
}

struct ActiveJob {
    cancel: CancelToken,
    phase: Arc<PhaseCell>,
}

/// The engine, model and presentation for one job.
struct EngineChoice {
    engine: Arc<dyn TextEngine>,
    model: ModelRef,
    label: String,
    color: String,
}

pub(crate) fn model_color(settings: &AppSettings, model: &ModelRef) -> String {
    settings
        .text_assist
        .model_colors
        .get(&model.color_key())
        .cloned()
        .or_else(|| catalog::find(model.provider, &model.model).map(|m| m.color.to_string()))
        .unwrap_or_else(|| catalog::default_color(model.provider).to_string())
}

pub(crate) fn model_label(model: &ModelRef) -> String {
    catalog::find(model.provider, &model.model)
        .map(|entry| entry.label.to_string())
        .unwrap_or_else(|| model.model.clone())
}

fn choose_engine(runtime: &Runtime, settings: &AppSettings) -> Result<EngineChoice, TextError> {
    if settings.text_assist.dev_offline_transform {
        let model = ModelRef::new(
            ProviderKind::Offline,
            crate::text_providers::offline::OFFLINE_MODEL_ID,
            None,
        );
        return Ok(EngineChoice {
            engine: Arc::new(UppercaseEngine::default()),
            label: "Offline test".to_string(),
            color: catalog::default_color(ProviderKind::Offline).to_string(),
            model,
        });
    }
    let model = settings.text_assist.quick_model.clone();
    let engine = runtime.engine_for(&model)?;
    Ok(EngineChoice {
        engine,
        label: model_label(&model),
        color: model_color(settings, &model),
        model,
    })
}

pub(crate) struct Runtime {
    pub(crate) settings: SettingsSource,
    pub(crate) update_settings: SettingsUpdate,
    pub(crate) halo: Halo,
    active: Mutex<Option<ActiveJob>>,
    last_kept: Arc<Mutex<Option<TextAssistRun>>>,
    /// One engine per provider (and so one pooled HTTPS connection) reused across jobs.
    factory: Arc<EngineFactory>,
    last_warm_up: Mutex<Option<Instant>>,
}

/// Minimum spacing between connection warm-ups.
const WARM_UP_INTERVAL: Duration = Duration::from_secs(20);

/// Subscription CLIs start a whole agent process, so they get the CLI budget (spec 3.4: 120 s)
/// instead of the API budget of the calling mode.
pub(crate) fn request_timeout(model: &ModelRef, api_timeout: Duration) -> Duration {
    match model.provider {
        ProviderKind::CodexCli | ProviderKind::ClaudeCli => {
            crate::text_providers::cli_common::CLI_TIMEOUT
        }
        _ => api_timeout,
    }
}

static RUNTIME: std::sync::OnceLock<Arc<Runtime>> = std::sync::OnceLock::new();

/// The running Text Assist runtime, once started.
pub(crate) fn current() -> Option<Arc<Runtime>> {
    RUNTIME.get().cloned()
}

impl Runtime {
    /// The engine for a model (OpenAI, Anthropic, Gemini, OpenAI-compatible or offline). The
    /// factory keeps one engine per provider/endpoint so connections are reused.
    pub(crate) fn engine_for(&self, model: &ModelRef) -> Result<Arc<dyn TextEngine>, TextError> {
        catalog::resolve_effort(model)?;
        let settings = (self.settings)();
        self.factory.engine_with_cli(
            model,
            &settings.text_assist.openai_compatible,
            &settings.text_assist.cli,
        )
    }

    /// Shows a final status (ring + short label) at `anchor`, or at the pointer.
    pub(crate) fn show_status(
        &self,
        anchor: Option<&Anchor>,
        phase: Phase,
        kind: &'static str,
        color: &str,
        label: &str,
    ) {
        let anchor = anchor.copied().unwrap_or_else(pointer_anchor);
        let layout = layout_for(&anchor);
        let generation = self.halo.show(
            &layout,
            HaloPayload {
                job_id: String::new(),
                phase: phase_name(phase),
                kind,
                mode: layout.mode,
                color: color.to_string(),
                label: label.to_string(),
                progress: None,
                box_rect: layout.box_rect,
                point: layout.point,
                badge_side: layout.badge_side,
                scale: layout.scale,
            },
        );
        self.halo.hide_later(generation, linger(phase));
    }

    fn run(self: Arc<Self>, events: mpsc::Receiver<AssistChordEvent>) {
        let mut pending_hold: Option<(u64, Instant)> = None;
        let mut cue: Option<u64> = None;
        loop {
            let event = match pending_hold {
                Some((_, deadline)) => {
                    match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(event) => Some(event),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                None => match events.recv() {
                    Ok(event) => Some(event),
                    Err(_) => return,
                },
            };
            match event {
                None => {
                    if let Some((press_id, _)) = pending_hold.take() {
                        let reached = assist_chord::hold_reached(press_id, Instant::now());
                        if show_cue(reached, panel::is_open(&self)) {
                            self.show_hold_cue();
                            cue = Some(press_id);
                        }
                    }
                }
                Some(AssistChordEvent::Armed { press_id }) => {
                    self.warm_up();
                    let threshold = (self.settings)().text_assist.hold_threshold_ms;
                    pending_hold =
                        Some((press_id, Instant::now() + Duration::from_millis(threshold)));
                }
                Some(AssistChordEvent::Cancelled { press_id }) => {
                    pending_hold = None;
                    if cue.take() == Some(press_id) {
                        end_hold_cue(&self.halo, true);
                    }
                }
                // While the Rewrite panel is open, Shift+Win starts its current choice instead
                // of Quick Polish or a new panel (the foreground is the panel itself).
                Some(AssistChordEvent::Tap { .. }) => {
                    pending_hold = None;
                    if cue.take().is_some() {
                        end_hold_cue(&self.halo, true);
                    }
                    if !panel::on_chord(&self, false) {
                        self.on_tap();
                    }
                }
                Some(AssistChordEvent::Hold { .. }) => {
                    pending_hold = None;
                    if cue.take().is_some() {
                        end_hold_cue(&self.halo, true);
                    }
                    if !panel::on_chord(&self, true) && (self.settings)().text_assist.enabled {
                        panel::open(&self);
                    }
                }
            }
        }
    }

    /// Pre-connects to the provider while Shift+Win is still down, so the request that follows
    /// on release does not pay for DNS and the TLS handshake (about 8 s on a cold start here).
    fn warm_up(&self) {
        let settings = (self.settings)();
        if !settings.text_assist.enabled || settings.text_assist.dev_offline_transform {
            return;
        }
        {
            let mut last = self.last_warm_up.lock().unwrap();
            if last.is_some_and(|at| at.elapsed() < WARM_UP_INTERVAL) {
                return;
            }
            *last = Some(Instant::now());
        }
        // Spawns its own task; no key or text is sent, and CLI/offline models are skipped.
        self.factory.warm_up(
            &settings.text_assist.quick_model,
            &settings.text_assist.openai_compatible,
        );
    }

    fn show_hold_cue(&self) {
        let halo = self.halo.clone();
        let settings = (self.settings)();
        let color = model_color(&settings, &settings.text_assist.quick_model);
        let ticket = {
            let mut current = CUE_TICKET.lock().unwrap();
            *current += 1;
            *current
        };
        thread::spawn(move || {
            let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
            // The geometry lookup can take up to about a second; the chord may end meanwhile.
            let (anchor, _) = geometry::inspect_target(hwnd.0 as isize);
            let layout = layout_for(&anchor);
            let current = CUE_TICKET.lock().unwrap();
            if *current != ticket {
                return;
            }
            let generation = halo.show(
                &layout,
                HaloPayload {
                    job_id: String::new(),
                    phase: "hold_cue",
                    kind: "rewrite",
                    mode: layout.mode,
                    color,
                    label: "Rewrite".to_string(),
                    progress: None,
                    box_rect: layout.box_rect,
                    point: layout.point,
                    badge_side: layout.badge_side,
                    scale: layout.scale,
                },
            );
            // Safety net: a cue never outlives a very long hold.
            halo.hide_later(generation, HOLD_CUE_TIMEOUT);
            drop(current);
        });
    }

    fn on_tap(&self) {
        let mut active = self.active.lock().unwrap();
        if let Some(job) = active.as_ref() {
            if job.phase.get().is_none_or(|phase| !phase.is_final()) {
                // A tap during a job cancels it, except while pasting (spec 3.4). The phase
                // cell decides this atomically with the job's switch to pasting.
                job.phase.try_cancel(&job.cancel);
                return;
            }
        }
        let settings = (self.settings)();
        if !settings.text_assist.enabled {
            return;
        }
        let choice = match choose_engine(self, &settings) {
            Ok(choice) => choice,
            Err(error) => {
                let label = match error {
                    TextError::MissingKey => "Add API key".to_string(),
                    TextError::Unsupported => "Model not available".to_string(),
                    other => job::engine_error_label(&other),
                };
                self.show_status(None, Phase::Failed, "polish", "#8a8f98", &label);
                return;
            }
        };
        let cancel = CancelToken::new();
        let phase = Arc::new(PhaseCell::default());
        *active = Some(ActiveJob {
            cancel: cancel.clone(),
            phase: phase.clone(),
        });
        drop(active);
        let limits = &settings.text_assist.limits;
        let polish_model = choice.model.clone();
        let polish = PolishJob {
            id: uuid::Uuid::new_v4().to_string(),
            model: choice.model,
            model_label: choice.label,
            max_chars: limits.quick_max_chars,
            restore_delay: Duration::from_millis(limits.paste_restore_delay_ms),
            timeout: request_timeout(&polish_model, job::QUICK_POLISH_TIMEOUT),
        };
        let observer = Arc::new(Presenter {
            halo: self.halo.clone(),
            color: choice.color,
            save_history: settings.text_assist.save_history,
            last_kept: self.last_kept.clone(),
        });
        tauri::async_runtime::spawn(job::run_polish(
            Arc::new(WindowsPlatform),
            choice.engine,
            observer,
            polish,
            cancel,
            phase,
        ));
    }
}

/// The hold cue ring shows once the hold threshold is reached, but never while the panel is
/// open (a chord there starts the panel's run instead of opening a new panel).
fn show_cue(hold_reached: bool, panel_open: bool) -> bool {
    hold_reached && !panel_open
}

/// Hold-cue tickets. A cue is shown only while its ticket is current (checked under this lock),
/// and every chord outcome advances the ticket, so a slow geometry lookup can never show the
/// cue after the chord ended.
static CUE_TICKET: Mutex<u64> = Mutex::new(0);
/// The longest a hold cue stays visible.
const HOLD_CUE_TIMEOUT: Duration = Duration::from_secs(10);

/// Ends the current hold cue: a cue still being prepared is never shown, and `hide` hides one
/// already visible.
fn end_hold_cue(halo: &Halo, hide: bool) {
    let mut current = CUE_TICKET.lock().unwrap();
    *current += 1;
    if hide {
        halo.hide_now();
    }
}

static LAST_KEPT: std::sync::OnceLock<Arc<Mutex<Option<TextAssistRun>>>> =
    std::sync::OnceLock::new();

/// The latest result that was kept instead of pasted (also when history is off).
pub fn last_kept_result() -> Option<TextAssistRun> {
    LAST_KEPT.get()?.lock().unwrap().clone()
}

/// Forgets the kept result when its run is deleted (`Some(id)`) or all runs are cleared
/// (`None`), so deleted text does not come back through the "last kept result" banner.
pub fn forget_kept(run_id: Option<&str>) {
    let Some(last_kept) = LAST_KEPT.get() else {
        return;
    };
    let mut last_kept = last_kept.lock().unwrap();
    if run_id.is_none_or(|id| last_kept.as_ref().is_some_and(|run| run.id == id)) {
        *last_kept = None;
    }
}

/// Applies chord-related settings (enabled, hold threshold).
pub fn apply_settings(settings: &AppSettings) {
    assist_chord::configure(
        settings.text_assist.enabled,
        Duration::from_millis(settings.text_assist.hold_threshold_ms),
    );
}

/// Prepares the halo window and starts the worker. Call once after the keyboard hook started.
pub fn start(
    app: tauri::AppHandle,
    settings: impl Fn() -> AppSettings + Send + Sync + 'static,
    update_settings: impl Fn(&dyn Fn(&mut AppSettings)) + Send + Sync + 'static,
) -> anyhow::Result<()> {
    if let Some(window) = app.get_webview_window(HALO_WINDOW) {
        // Click-through and never focusable, like the status pill; parked off-screen while
        // idle so it never needs a show/activate call during a job.
        window.set_ignore_cursor_events(true)?;
        window.set_focusable(false)?;
        window.set_position(PARK)?;
        window.set_size(PhysicalSize::new(1, 1))?;
        window.show()?;
        window.set_always_on_top(true)?;
    }
    let settings: SettingsSource = Arc::new(settings);
    apply_settings(&settings());
    let (sender, receiver) = mpsc::channel();
    assist_chord::set_sender(sender)?;
    let last_kept = LAST_KEPT.get_or_init(|| Arc::new(Mutex::new(None))).clone();
    if let Some(window) = app.get_webview_window(panel::PANEL_WINDOW) {
        window.set_always_on_top(true)?;
    }
    let runtime = Arc::new(Runtime {
        settings,
        update_settings: Arc::new(update_settings),
        halo: Halo {
            app,
            generation: Arc::new(AtomicU64::new(0)),
            last_window: Arc::new(Mutex::new(None)),
        },
        active: Mutex::new(None),
        last_kept,
        factory: Arc::new(EngineFactory::new()),
        last_warm_up: Mutex::new(None),
    });
    let _ = RUNTIME.set(runtime.clone());
    // Open the provider connection once at startup as well.
    runtime.warm_up();
    thread::Builder::new()
        .name("text-assist".to_string())
        .spawn(move || runtime.run(receiver))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_a_kept_run_forgets_it() {
        let last_kept = LAST_KEPT.get_or_init(|| Arc::new(Mutex::new(None)));
        let kept = |id: &str| TextAssistRun {
            id: id.to_string(),
            created_at: String::new(),
            kind: "polish".to_string(),
            preset_id: None,
            ensemble_run_id: None,
            provider: "openai".to_string(),
            model: "m".to_string(),
            effort: None,
            billing: "api".to_string(),
            original_text: "synthetic".to_string(),
            result_text: Some("Synthetic".to_string()),
            outcome: "kept".to_string(),
            reason_code: None,
            target_app: None,
            duration_ms: None,
            first_token_ms: None,
            input_tokens: None,
            output_tokens: None,
            estimated_cost_usd: None,
        };
        *last_kept.lock().unwrap() = Some(kept("a"));
        forget_kept(Some("b"));
        assert!(last_kept_result().is_some_and(|run| run.id == "a"));
        forget_kept(Some("a"));
        assert!(last_kept_result().is_none());
        *last_kept.lock().unwrap() = Some(kept("c"));
        forget_kept(None);
        assert!(last_kept_result().is_none());
    }

    #[test]
    fn hold_cue_waits_for_the_threshold_and_never_shows_over_the_panel() {
        assert!(show_cue(true, false));
        assert!(!show_cue(false, false));
        assert!(!show_cue(true, true));
        assert!(!show_cue(false, true));
    }
}
