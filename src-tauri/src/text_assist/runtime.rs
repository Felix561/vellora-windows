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
    platform::{TargetSnapshot, WindowsPlatform},
};
use crate::{
    assist_chord::{self, AssistChordEvent},
    credentials,
    history::{self, TextAssistRun, OUTCOME_KEPT},
    text_providers::{
        catalog, offline::UppercaseEngine, openai::OpenAiEngine, CancelToken, ModelRef,
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

type SettingsSource = Arc<dyn Fn() -> AppSettings + Send + Sync>;

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
struct Halo {
    app: tauri::AppHandle,
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

    fn hide_now(&self) {
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

fn model_color(settings: &AppSettings, model: &ModelRef) -> String {
    settings
        .text_assist
        .model_colors
        .get(&model.color_key())
        .cloned()
        .or_else(|| catalog::find(model.provider, &model.model).map(|m| m.color.to_string()))
        .unwrap_or_else(|| catalog::default_color(model.provider).to_string())
}

fn model_label(model: &ModelRef) -> String {
    catalog::find(model.provider, &model.model)
        .map(|entry| entry.label.to_string())
        .unwrap_or_else(|| model.model.clone())
}

fn choose_engine(
    settings: &AppSettings,
    openai: &Mutex<Option<Arc<OpenAiEngine>>>,
) -> Result<EngineChoice, TextError> {
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
    catalog::resolve_effort(&model)?;
    let engine: Arc<dyn TextEngine> = match model.provider {
        ProviderKind::Openai => openai_engine(openai)?,
        _ => return Err(TextError::Unsupported),
    };
    Ok(EngineChoice {
        engine,
        label: model_label(&model),
        color: model_color(settings, &model),
        model,
    })
}

struct Runtime {
    settings: SettingsSource,
    halo: Halo,
    active: Mutex<Option<ActiveJob>>,
    last_kept: Arc<Mutex<Option<TextAssistRun>>>,
    /// One OpenAI engine (and so one pooled HTTPS connection) reused across jobs.
    openai: Arc<Mutex<Option<Arc<OpenAiEngine>>>>,
    last_warm_up: Mutex<Option<Instant>>,
}

/// Minimum spacing between connection warm-ups.
const WARM_UP_INTERVAL: Duration = Duration::from_secs(20);

fn openai_engine(cache: &Mutex<Option<Arc<OpenAiEngine>>>) -> Result<Arc<OpenAiEngine>, TextError> {
    let key = credentials::get_openai_key()
        .map_err(|_| TextError::MissingKey)?
        .ok_or(TextError::MissingKey)?;
    let mut cache = cache.lock().unwrap();
    if let Some(engine) = cache.as_ref().filter(|engine| engine.uses_key(&key)) {
        return Ok(engine.clone());
    }
    let engine = Arc::new(OpenAiEngine::new(key).map_err(|_| TextError::Network)?);
    *cache = Some(engine.clone());
    Ok(engine)
}

impl Runtime {
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
                        if assist_chord::hold_reached(press_id, Instant::now()) {
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
                        self.halo.hide_now();
                    }
                }
                Some(AssistChordEvent::Tap { .. }) => {
                    pending_hold = None;
                    cue = None;
                    self.on_tap();
                }
                Some(AssistChordEvent::Hold { .. }) => {
                    pending_hold = None;
                    cue = None;
                    self.on_hold();
                }
            }
        }
    }

    /// Pre-connects to the provider while Shift+Win is still down, so the request that follows
    /// on release does not pay for DNS and the TLS handshake (about 8 s on a cold start here).
    fn warm_up(&self) {
        let settings = (self.settings)();
        if !settings.text_assist.enabled
            || settings.text_assist.dev_offline_transform
            || settings.text_assist.quick_model.provider != ProviderKind::Openai
        {
            return;
        }
        {
            let mut last = self.last_warm_up.lock().unwrap();
            if last.is_some_and(|at| at.elapsed() < WARM_UP_INTERVAL) {
                return;
            }
            *last = Some(Instant::now());
        }
        let cache = self.openai.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(engine) = openai_engine(&cache) {
                engine.warm_up().await;
            }
        });
    }

    fn show_info(&self, phase: Phase, label: &str, color: &str) {
        let anchor = pointer_anchor();
        let layout = layout_for(&anchor);
        let generation = self.halo.show(
            &layout,
            HaloPayload {
                job_id: String::new(),
                phase: phase_name(phase),
                kind: "polish",
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

    fn show_hold_cue(&self) {
        let halo = self.halo.clone();
        let settings = (self.settings)();
        let color = model_color(&settings, &settings.text_assist.quick_model);
        thread::spawn(move || {
            let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
            let (anchor, _) = geometry::inspect_target(hwnd.0 as isize);
            let layout = layout_for(&anchor);
            halo.show(
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
        });
    }

    fn on_hold(&self) {
        // The Rewrite panel arrives in M4; until then explain instead of doing nothing.
        self.halo.hide_now();
        self.show_info(Phase::Kept, "Panel coming soon", "#8a8f98");
    }

    fn on_tap(&self) {
        let mut active = self.active.lock().unwrap();
        if let Some(job) = active.as_ref() {
            if job.phase.get().is_none_or(|phase| !phase.is_final()) {
                // A tap during a job cancels it, except while pasting (spec 3.4).
                if job.phase.cancellable() {
                    job.cancel.cancel();
                }
                return;
            }
        }
        let settings = (self.settings)();
        if !settings.text_assist.enabled {
            return;
        }
        let choice = match choose_engine(&settings, &self.openai) {
            Ok(choice) => choice,
            Err(error) => {
                let label = match error {
                    TextError::MissingKey => "Add API key".to_string(),
                    TextError::Unsupported => "Model not available".to_string(),
                    other => job::engine_error_label(&other),
                };
                self.show_info(Phase::Failed, &label, "#8a8f98");
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
        let polish = PolishJob {
            id: uuid::Uuid::new_v4().to_string(),
            model: choice.model,
            model_label: choice.label,
            max_chars: limits.quick_max_chars,
            restore_delay: Duration::from_millis(limits.paste_restore_delay_ms),
            timeout: job::QUICK_POLISH_TIMEOUT,
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

static LAST_KEPT: std::sync::OnceLock<Arc<Mutex<Option<TextAssistRun>>>> =
    std::sync::OnceLock::new();

/// The latest result that was kept instead of pasted (also when history is off).
pub fn last_kept_result() -> Option<TextAssistRun> {
    LAST_KEPT.get()?.lock().unwrap().clone()
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
    let runtime = Arc::new(Runtime {
        settings,
        halo: Halo {
            app,
            generation: Arc::new(AtomicU64::new(0)),
            last_window: Arc::new(Mutex::new(None)),
        },
        active: Mutex::new(None),
        last_kept,
        openai: Arc::new(Mutex::new(None)),
        last_warm_up: Mutex::new(None),
    });
    // Open the provider connection once at startup as well.
    runtime.warm_up();
    thread::Builder::new()
        .name("text-assist".to_string())
        .spawn(move || runtime.run(receiver))?;
    Ok(())
}
