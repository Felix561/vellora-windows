#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod brand;
mod clipboard;
mod credentials;
mod history;
mod hotkey;
mod migration;
mod openai;
mod pricing;
mod settings;
mod support;
mod types;
mod windows_integration;

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use chrono::Utc;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};
use types::{
    AppHealthSnapshot, AppSettings, AppSnapshot, AudioLevelEvent, CleanupMode, DashboardStats,
    DiagnosticCode, DiagnosticEvent, DictationState, OverlaySnapshot, RecordingMode, StartupPhase,
    TranscriptHistoryItem,
};
use uuid::Uuid;

struct RuntimeState {
    snapshot: Mutex<AppSnapshot>,
    health: Mutex<AppHealthSnapshot>,
    settings: Mutex<AppSettings>,
    startup_warning: Option<String>,
    state_generation: AtomicU64,
    recorder: audio::AudioRecorder,
    busy: Mutex<bool>,
    recording_session: Mutex<Option<RecordingSession>>,
}

static RUNTIME_STATE: OnceLock<Arc<RuntimeState>> = OnceLock::new();
static STARTED_AT: OnceLock<String> = OnceLock::new();
const MIN_TRANSCRIPTION_RECORDING_MS: u64 = 1_200;

#[derive(Debug, Clone)]
struct RecordingSession {
    id: String,
    mode: RecordingMode,
    started_at_utc: chrono::DateTime<Utc>,
    started_at_instant: Instant,
    paste_target: isize,
    hotkey_epoch: u64,
    release_epoch: u64,
    pending_hold_release: Option<u64>,
}

impl RecordingSession {
    fn resume_hold(&mut self) -> bool {
        if self.mode != RecordingMode::Hold {
            return false;
        }
        self.pending_hold_release = None;
        true
    }

    fn defer_hold_release(&mut self) -> Option<u64> {
        if self.mode != RecordingMode::Hold {
            return None;
        }
        self.release_epoch = self.release_epoch.wrapping_add(1);
        self.pending_hold_release = Some(self.release_epoch);
        self.pending_hold_release
    }

    fn change_mode(&mut self, mode: RecordingMode) {
        self.mode = mode;
        self.pending_hold_release = None;
    }
}

fn claim_recording_session(
    current: &mut Option<RecordingSession>,
    expected_id: Option<&str>,
    expected_hold_release: Option<u64>,
) -> Option<RecordingSession> {
    if !current.as_ref().is_some_and(|session| {
        Some(session.id.as_str()) == expected_id
            && expected_hold_release.is_none_or(|release| {
                session.mode == RecordingMode::Hold && session.pending_hold_release == Some(release)
            })
    }) {
        return None;
    }
    current.take()
}

trait RecordingEpochGate {
    fn with_current_epoch<T>(&self, epoch: u64, operation: impl FnOnce() -> T) -> Option<T>;
}

struct HotkeyEpochGate;

impl RecordingEpochGate for HotkeyEpochGate {
    fn with_current_epoch<T>(&self, epoch: u64, operation: impl FnOnce() -> T) -> Option<T> {
        hotkey::with_current_epoch(epoch, operation)
    }
}

fn reserve_recording_session(
    gate: &impl RecordingEpochGate,
    current: &mut Option<RecordingSession>,
    session: RecordingSession,
) -> bool {
    gate.with_current_epoch(session.hotkey_epoch, || {
        if current.is_some() {
            return false;
        }
        *current = Some(session);
        true
    }) == Some(true)
}

impl RuntimeState {
    fn new() -> anyhow::Result<Self> {
        let (settings, startup_warning) = settings::load_settings_with_warning()?;
        Ok(Self {
            snapshot: Mutex::new(AppSnapshot {
                state: DictationState::Idle,
                message: None,
                last_transcript: None,
                recording_mode: None,
            }),
            health: Mutex::new(default_health(StartupPhase::InitializingRuntime, false)),
            settings: Mutex::new(settings),
            startup_warning,
            state_generation: AtomicU64::new(0),
            recorder: audio::AudioRecorder::new(),
            busy: Mutex::new(false),
            recording_session: Mutex::new(None),
        })
    }
}

#[tauri::command]
fn get_settings() -> Result<AppSettings, String> {
    let state = runtime_state()?;
    let settings = state.settings.lock().unwrap().clone();
    Ok(settings)
}

fn runtime_state() -> Result<Arc<RuntimeState>, String> {
    RUNTIME_STATE
        .get()
        .cloned()
        .ok_or_else(|| "Vellora is still starting. Please try again in a moment.".to_string())
}

#[tauri::command]
fn save_settings(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let state = runtime_state()?;
    require_main_window(&window)?;
    let mut current = state.settings.lock().unwrap();
    settings::save_settings(&settings).map_err(to_string)?;
    *current = settings.clone();
    drop(current);
    let _ = app.emit("settings-changed", &settings);
    Ok(settings)
}

#[tauri::command]
fn has_openai_api_key(window: tauri::WebviewWindow) -> Result<bool, String> {
    require_main_window(&window)?;
    credentials::get_openai_key()
        .map(|key| key.is_some())
        .map_err(to_string)
}

#[tauri::command]
fn save_openai_api_key(window: tauri::WebviewWindow, key: String) -> Result<(), String> {
    require_main_window(&window)?;
    credentials::save_openai_key(key.trim()).map_err(to_string)
}

#[tauri::command]
fn delete_openai_api_key(window: tauri::WebviewWindow) -> Result<(), String> {
    require_main_window(&window)?;
    credentials::delete_openai_key().map_err(to_string)
}

#[tauri::command]
fn get_dictation_snapshot(window: tauri::WebviewWindow) -> Result<AppSnapshot, String> {
    let state = runtime_state()?;
    let mut snapshot = state.snapshot.lock().unwrap().clone();
    if window.label() != "main" {
        snapshot.last_transcript = None;
    }
    Ok(snapshot)
}

#[tauri::command]
fn list_transcripts(
    window: tauri::WebviewWindow,
    limit: usize,
) -> Result<Vec<TranscriptHistoryItem>, String> {
    require_main_window(&window)?;
    history::list(limit.min(500)).map_err(to_string)
}

#[tauri::command]
fn copy_transcript(window: tauri::WebviewWindow, id: String) -> Result<(), String> {
    require_main_window(&window)?;
    let item = find_transcript(&id)?;
    clipboard::copy_text(&item.final_text).map_err(to_string)
}

#[tauri::command]
fn paste_transcript(window: tauri::WebviewWindow, id: String) -> Result<(), String> {
    require_main_window(&window)?;
    let item = find_transcript(&id)?;
    clipboard::copy_text(&item.final_text).map_err(to_string)?;
    clipboard::paste_clipboard()
        .map_err(|err| format!("Copied to clipboard, but paste failed. {err:#}"))
}

#[tauri::command]
fn get_dashboard_stats(window: tauri::WebviewWindow) -> Result<DashboardStats, String> {
    require_main_window(&window)?;
    history::dashboard_stats().map_err(to_string)
}

#[tauri::command]
fn get_app_health(window: tauri::WebviewWindow) -> Result<AppHealthSnapshot, String> {
    require_main_window(&window)?;
    let state = runtime_state()?;
    let health = state.health.lock().unwrap().clone();
    Ok(health)
}

#[tauri::command]
async fn get_support_info(window: tauri::WebviewWindow) -> Result<support::SupportInfo, String> {
    require_main_window(&window)?;
    let state = runtime_state()?;
    let health = state.health.lock().unwrap().clone();
    let dictation_state = state.snapshot.lock().unwrap().state.clone();
    let key_configured = credentials::get_openai_key()
        .map_err(|_| "Could not check whether an API key is saved.".to_string())?
        .is_some();
    let microphone_available = support::check_microphone()
        .await
        .map(|status| status.available)
        .unwrap_or(false);
    Ok(support::SupportInfo::new(
        health,
        dictation_state,
        key_configured,
        microphone_available,
    ))
}

#[tauri::command]
async fn check_microphone(
    window: tauri::WebviewWindow,
) -> Result<support::MicrophoneStatus, String> {
    require_main_window(&window)?;
    support::check_microphone().await
}

#[tauri::command]
fn open_help_link(window: tauri::WebviewWindow, id: String) -> Result<(), String> {
    require_main_window(&window)?;
    support::open_help_link(&id)
}

#[tauri::command]
fn show_main_window_command(app: tauri::AppHandle) {
    show_main_window(&app);
}

#[tauri::command]
fn clear_error(window: tauri::WebviewWindow) -> Result<(), String> {
    require_main_window(&window)?;
    let state = runtime_state()?;
    if *state.busy.lock().unwrap() {
        return Err("Wait for the current dictation to finish.".to_string());
    }
    set_state(window.app_handle(), &state, DictationState::Idle, None);
    emit_overlay_state(
        window.app_handle(),
        &state,
        DictationState::Idle,
        None,
        None,
    );
    Ok(())
}

fn require_main_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("This action is only available in the main Vellora window.".to_string())
    }
}

fn find_transcript(id: &str) -> Result<TranscriptHistoryItem, String> {
    if let Some(item) = runtime_state()?
        .snapshot
        .lock()
        .unwrap()
        .last_transcript
        .as_ref()
        .filter(|item| item.id == id)
        .cloned()
    {
        return Ok(item);
    }
    history::get(id)
        .map_err(to_string)?
        .ok_or_else(|| "Transcript not found".to_string())
}

#[tauri::command]
fn delete_transcript(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    id: String,
) -> Result<bool, String> {
    require_main_window(&window)?;
    let removed = history::delete(&id).map_err(to_string)?;
    let state = runtime_state()?;
    let mut snapshot = state.snapshot.lock().unwrap();
    if snapshot
        .last_transcript
        .as_ref()
        .is_some_and(|item| item.id == id)
    {
        snapshot.last_transcript = None;
    }
    drop(snapshot);
    let _ = app.emit_to("main", "history-changed", ());
    Ok(removed)
}

#[tauri::command]
fn clear_transcripts(window: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<usize, String> {
    require_main_window(&window)?;
    let removed = history::clear().map_err(to_string)?;
    runtime_state()?.snapshot.lock().unwrap().last_transcript = None;
    let _ = app.emit_to("main", "history-changed", ());
    Ok(removed)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name(brand::APP_NAME)
                .arg("--from-autostart")
                .build(),
        )
        .setup(|app| {
            let _ = STARTED_AT.set(Utc::now().to_rfc3339());
            let early_health = Arc::new(Mutex::new(default_health(StartupPhase::Booting, false)));
            set_early_health(&early_health, StartupPhase::Migrating, None);
            let mut migration_warnings = migration::run_startup_migrations();
            if let Err(err) = audio::cleanup_stale_audio_files() {
                migration_warnings.push(format!("Temporary audio cleanup warning: {err:#}"));
            }
            for warning in &migration_warnings {
                eprintln!("{warning}");
            }
            set_early_health(&early_health, StartupPhase::InitializingRuntime, None);
            let state = Arc::new(RuntimeState::new().map_err(|err| err.to_string())?);
            *state.health.lock().unwrap() = early_health.lock().unwrap().clone();
            let _ = RUNTIME_STATE.set(state.clone());
            windows_integration::ensure_release_desktop_shortcut();
            if let Some(warning) = &state.startup_warning {
                record_diagnostic(app.handle(), &state, diagnostic(DiagnosticCode::StartupNotReady, "Settings recovered", warning.clone(), None, true));
            }
            if !migration_warnings.is_empty() {
                let diagnostic = diagnostic(
                    DiagnosticCode::StartupNotReady,
                    "Legacy migration warning",
                    format!(
                        "Some legacy {} data could not be migrated. Dictation can still run.",
                        brand::OLD_APP_NAME
                    ),
                    Some(migration_warnings.join("\n")),
                    true,
                );
                record_diagnostic(app.handle(), &state, diagnostic);
                state.snapshot.lock().unwrap().message = Some(format!(
                    "Some legacy {} data could not be migrated. Dictation can still run.",
                    brand::OLD_APP_NAME
                ));
            }
            if let Some(main_window) = app.get_webview_window("main") {
                let win = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win.hide();
                    }
                });
            }
            setup_audio_level_events(app.handle().clone(), state.clone());
            setup_recording_events(app.handle().clone(), state.clone());
            app.manage(state.clone());
            set_health(app.handle(), &state, StartupPhase::InitializingTray, None);
            setup_tray(app.handle())?;
            set_health(
                app.handle(),
                &state,
                StartupPhase::InitializingOverlay,
                None,
            );
            if let Some(window) = app.get_webview_window("overlay") {
                // Native hit testing must pass through the whole window, including
                // transparent margins and the larger recording/error states.
                window.set_ignore_cursor_events(true)?;
                window.set_focusable(false)?;
            }
            position_overlay(app.handle(), 64.0, 24.0);
            show_overlay(app.handle());
            emit_overlay_state(
                app.handle(),
                &state,
                DictationState::Idle,
                Some("Ready".to_string()),
                None,
            );
            set_health(app.handle(), &state, StartupPhase::InitializingHotkey, None);
            match start_hotkey_worker(app.handle().clone(), state.clone()) {
                Ok(()) => set_health(app.handle(), &state, StartupPhase::Ready, None),
                Err(err) => {
                    set_health(app.handle(), &state, StartupPhase::Degraded, None);
                    record_diagnostic(app.handle(), &state, diagnostic_from_error(DiagnosticCode::HotkeyStateMismatch, "Hotkey unavailable", "Vellora could not register the keyboard shortcut. Restart the app to retry.", &anyhow::anyhow!(err)));
                    show_main_window(app.handle());
                }
            }
            if should_show_main_on_launch() {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            windows_integration::get_start_at_login,
            windows_integration::set_start_at_login,
            get_settings,
            save_settings,
            has_openai_api_key,
            save_openai_api_key,
            delete_openai_api_key,
            get_dictation_snapshot,
            list_transcripts,
            delete_transcript,
            clear_transcripts,
            copy_transcript,
            paste_transcript,
            get_dashboard_stats,
            get_app_health,
            get_support_info,
            check_microphone,
            open_help_link,
            show_main_window_command,
            clear_error,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vellora");
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(
        app,
        "show",
        format!("Open {}", brand::APP_NAME),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let icon = app.default_window_icon().cloned().unwrap_or_else(|| {
        tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png")).unwrap()
    });

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app.emit_to("main", "open-dashboard-requested", ());
    }
}

fn should_show_main_on_launch() -> bool {
    !std::env::args().any(|arg| arg == "--from-autostart")
}

fn show_overlay(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.show();
        let _ = window.set_always_on_top(true);
    }
}

fn position_overlay(app: &tauri::AppHandle, width: f64, height: f64) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };
    let position = monitor.position();
    let size = monitor.size();
    let x = position.x + ((size.width as f64 - width) / 2.0).round() as i32;
    let y = position.y;
    let _ = window.set_size(PhysicalSize::new(
        width.round() as u32,
        height.round() as u32,
    ));
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

fn setup_audio_level_events(app: tauri::AppHandle, state: Arc<RuntimeState>) {
    let callback_state = state.clone();
    let callback = Arc::new(move |level: f32| {
        if !matches!(
            callback_state.snapshot.lock().unwrap().state,
            DictationState::Recording
        ) {
            return;
        }
        let _ = app.emit("audio-level", AudioLevelEvent { level });
        emit_overlay_state(
            &app,
            &callback_state,
            DictationState::Recording,
            None,
            Some(level),
        );
    });
    state.recorder.set_audio_level_callback(Some(callback));
}

fn setup_recording_events(app: tauri::AppHandle, state: Arc<RuntimeState>) {
    let callback_state = state.clone();
    state
        .recorder
        .set_recording_event_callback(Some(Arc::new(move |event| {
            let app = app.clone();
            let state = callback_state.clone();
            tauri::async_runtime::spawn(async move {
                if !event.belongs_to(current_session_id(&state).as_deref()) {
                    return;
                }
                let session_id = Some(event.session_id);
                match event.kind {
                    audio::RecordingEventKind::LimitReached => {
                        let _ =
                            finish_recording(app.clone(), state.clone(), session_id, None).await;
                    }
                    audio::RecordingEventKind::Error(message) => {
                        let owns_session = {
                            let mut current = state.recording_session.lock().unwrap();
                            if current.as_ref().map(|session| &session.id) != session_id.as_ref() {
                                false
                            } else {
                                let session = current.take().unwrap();
                                hotkey::reset_recording_if_epoch(session.hotkey_epoch);
                                true
                            }
                        };
                        if !owns_session {
                            return;
                        }
                        state.recorder.cancel();
                        record_diagnostic(
                            &app,
                            &state,
                            diagnostic(
                                DiagnosticCode::MicrophoneUnavailable,
                                "Microphone stopped",
                                message.clone(),
                                None,
                                true,
                            ),
                        );
                        fail(&app, &state, message);
                        release_busy(&state);
                    }
                }
            });
        })));
}

fn start_hotkey_worker(app: tauri::AppHandle, state: Arc<RuntimeState>) -> tauri::Result<()> {
    let (sender, receiver) = mpsc::channel();
    hotkey::start_listener(sender).map_err(tauri::Error::Anyhow)?;

    thread::spawn(move || {
        while let Ok(notification) = receiver.recv() {
            let epoch = notification.reset_epoch;
            if hotkey::with_current_epoch(epoch, || ()).is_none() {
                continue;
            }
            match notification.event {
                hotkey::HotkeyEvent::HoldStarted => {
                    let resumed = {
                        let mut current = state.recording_session.lock().unwrap();
                        hotkey::with_current_epoch(epoch, || {
                            current
                                .as_mut()
                                .filter(|session| session.hotkey_epoch == epoch)
                                .is_some_and(RecordingSession::resume_hold)
                        })
                        .unwrap_or(false)
                    };
                    if resumed {
                        continue;
                    }
                    if try_mark_busy(&state) {
                        match begin_recording(&app, &state, RecordingMode::Hold, epoch) {
                            Ok(true) => {}
                            Ok(false) => release_busy(&state),
                            Err(err) => {
                                fail(&app, &state, err.to_string());
                                release_busy(&state);
                            }
                        }
                    } else {
                        reset_rejected_start_if_no_session(&state, epoch, &receiver);
                    }
                }
                hotkey::HotkeyEvent::HoldReleased => {
                    let pending_release = {
                        let mut current = state.recording_session.lock().unwrap();
                        hotkey::with_current_epoch(epoch, || {
                            current
                                .as_mut()
                                .filter(|session| session.hotkey_epoch == epoch)
                                .and_then(|session| {
                                    session
                                        .defer_hold_release()
                                        .map(|release| (session.id.clone(), release))
                                })
                        })
                        .flatten()
                    };
                    let Some((session_id, release)) = pending_release else {
                        continue;
                    };
                    let app = app.clone();
                    let state = state.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(450)).await;
                        let _ = finish_recording(app, state, Some(session_id), Some(release)).await;
                    });
                }
                hotkey::HotkeyEvent::LockStarted => {
                    if !set_recording_mode(&app, &state, RecordingMode::Locked, epoch) {
                        if try_mark_busy(&state) {
                            match begin_recording(&app, &state, RecordingMode::Locked, epoch) {
                                Ok(true) => {}
                                Ok(false) => release_busy(&state),
                                Err(err) => {
                                    fail(&app, &state, err.to_string());
                                    release_busy(&state);
                                }
                            }
                        } else {
                            reset_rejected_start_if_no_session(&state, epoch, &receiver);
                        }
                    }
                }
                hotkey::HotkeyEvent::LockStopRequested => {
                    let app = app.clone();
                    let state = state.clone();
                    let session_id = {
                        let current = state.recording_session.lock().unwrap();
                        hotkey::with_current_epoch(epoch, || {
                            current
                                .as_ref()
                                .filter(|session| session.hotkey_epoch == epoch)
                                .map(|session| session.id.clone())
                        })
                        .flatten()
                    };
                    if session_id.is_none() {
                        continue;
                    }
                    tauri::async_runtime::spawn(async move {
                        let _ = finish_recording(app, state, session_id, None).await;
                    });
                }
            }
        }
    });
    Ok(())
}

fn try_mark_busy(state: &Arc<RuntimeState>) -> bool {
    let mut busy = state.busy.lock().unwrap();
    if *busy {
        return false;
    }
    *busy = true;
    true
}

fn release_busy(state: &Arc<RuntimeState>) {
    *state.busy.lock().unwrap() = false;
}

fn reset_rejected_start_if_no_session(
    state: &Arc<RuntimeState>,
    epoch: u64,
    receiver: &mpsc::Receiver<hotkey::HotkeyNotification>,
) {
    // Busy processing cannot accept a new recording. Clear its logical hotkey
    // gesture without disturbing a recording that still owns the session.
    let current = state.recording_session.lock().unwrap();
    if current.is_none() {
        hotkey::reset_recording_and_discard_pending_if_epoch(epoch, receiver);
    }
}

fn begin_recording(
    app: &tauri::AppHandle,
    state: &Arc<RuntimeState>,
    mode: RecordingMode,
    epoch: u64,
) -> anyhow::Result<bool> {
    // False means this queued gesture lost its epoch before starting. The
    // caller releases its busy reservation without publishing a failure.
    let has_key = match credentials::get_openai_key() {
        Ok(key) => key.is_some(),
        Err(err) => {
            return if hotkey::reset_recording_if_epoch(epoch) {
                Err(err)
            } else {
                Ok(false)
            };
        }
    };
    if !has_key {
        if !hotkey::reset_recording_if_epoch(epoch) {
            return Ok(false);
        }
        let event = diagnostic(
            DiagnosticCode::MissingApiKey,
            "Missing OpenAI API key",
            "Add your OpenAI API key in Settings before dictating.",
            None,
            true,
        );
        record_diagnostic(app, state, event);
        anyhow::bail!("Add your OpenAI API key in Settings before dictating.");
    }
    let session = RecordingSession {
        id: Uuid::new_v4().to_string(),
        mode: mode.clone(),
        started_at_utc: Utc::now(),
        started_at_instant: Instant::now(),
        paste_target: clipboard::foreground_target(),
        hotkey_epoch: epoch,
        release_epoch: 0,
        pending_hold_release: None,
    };
    // Do not let a device-error callback publish failure before the initial
    // Recording state is finished publishing.
    let mut current_session = state.recording_session.lock().unwrap();
    let session_id = session.id.clone();
    // A queued gesture may have become stale during the credential query.
    // Validate and reserve together, then release the hook mutex before opening
    // the microphone (which can block while a device initializes).
    if !reserve_recording_session(&HotkeyEpochGate, &mut current_session, session) {
        return Ok(false);
    }
    if let Err(err) = state.recorder.start(session_id) {
        *current_session = None;
        let owns_epoch = hotkey::reset_recording_if_epoch(epoch);
        drop(current_session);
        if !owns_epoch {
            return Ok(false);
        }
        record_diagnostic(
            app,
            state,
            diagnostic_from_error(
                DiagnosticCode::MicrophoneUnavailable,
                "Microphone unavailable",
                "Vellora could not start the microphone.",
                &err,
            ),
        );
        return Err(err);
    }
    state.snapshot.lock().unwrap().recording_mode = Some(mode.clone());
    let message = if mode == RecordingMode::Locked {
        "Locked recording"
    } else {
        "Recording"
    };
    set_state(
        app,
        state,
        DictationState::Recording,
        Some(format!("{message}...")),
    );
    emit_overlay_state(
        app,
        state,
        DictationState::Recording,
        Some(message.to_string()),
        Some(0.0),
    );
    Ok(true)
}

// The first stop/limit event claims the session. Duplicate events cannot release
// another operation's busy flag or overwrite its state.
struct BusyRelease(Arc<RuntimeState>);
impl Drop for BusyRelease {
    fn drop(&mut self) {
        release_busy(&self.0);
    }
}

async fn finish_recording(
    app: tauri::AppHandle,
    state: Arc<RuntimeState>,
    expected_id: Option<String>,
    expected_hold_release: Option<u64>,
) -> anyhow::Result<()> {
    let session = {
        let mut current = state.recording_session.lock().unwrap();
        // Validate release identity and claim in one critical section. A
        // resumed hold or a newer release makes earlier timer tasks harmless.
        let session =
            claim_recording_session(&mut current, expected_id.as_deref(), expected_hold_release);
        if let Some(session) = &session {
            hotkey::reset_recording_if_epoch(session.hotkey_epoch);
        }
        session
    };
    let Some(session) = session else {
        return Ok(());
    };
    let _busy_release = BusyRelease(state.clone());
    let result = finish_recording_session(app.clone(), state.clone(), session).await;
    if let Err(err) = &result {
        state.recorder.cancel();
        fail(&app, &state, err.to_string());
    }
    result
}

async fn finish_recording_session(
    app: tauri::AppHandle,
    state: Arc<RuntimeState>,
    session: RecordingSession,
) -> anyhow::Result<()> {
    let settings = state.settings.lock().unwrap().clone();
    let pipeline_started = session.started_at_utc;
    let total_started = Instant::now();
    let recording_duration_ms = session.started_at_instant.elapsed().as_millis() as u64;
    let audio_path = match state.recorder.stop_to_wav()? {
        Some(path) => path,
        None => {
            record_diagnostic(
                &app,
                &state,
                diagnostic(
                    DiagnosticCode::RecordingTooShort,
                    "Recording was too short",
                    "Recording was too short.",
                    None,
                    true,
                ),
            );
            set_state(
                &app,
                &state,
                DictationState::Idle,
                Some("Recording was too short.".to_string()),
            );
            emit_overlay_state(
                &app,
                &state,
                DictationState::Idle,
                Some("Too short".to_string()),
                None,
            );
            return Ok(());
        }
    };
    if recording_duration_ms < MIN_TRANSCRIPTION_RECORDING_MS {
        drop(audio_path);
        record_diagnostic(
            &app,
            &state,
            diagnostic(
                DiagnosticCode::RecordingTooShort,
                "Recording was too short",
                "Recording was too short.",
                Some(format!(
                    "{recording_duration_ms}ms recording was ignored before transcription."
                )),
                true,
            ),
        );
        set_state(
            &app,
            &state,
            DictationState::Idle,
            Some("Recording was too short.".to_string()),
        );
        emit_overlay_state(
            &app,
            &state,
            DictationState::Idle,
            Some("Too short".to_string()),
            None,
        );
        return Ok(());
    }

    let api_key = credentials::get_openai_key()?
        .ok_or_else(|| anyhow::anyhow!("Add your OpenAI API key in Settings before dictating."))?;

    let transcription_started = Instant::now();
    set_state(
        &app,
        &state,
        DictationState::Transcribing,
        Some("Transcribing...".to_string()),
    );
    emit_overlay_state(
        &app,
        &state,
        DictationState::Transcribing,
        Some("Transcribing".to_string()),
        None,
    );
    let raw_text = match openai::transcribe(
        &api_key,
        &audio_path,
        &settings.stt_model,
        &settings.language,
    )
    .await
    {
        Ok(text) => text,
        Err(err) => {
            drop(audio_path);
            record_diagnostic(
                &app,
                &state,
                diagnostic_from_error(
                    openai::diagnostic_code(&err),
                    "Transcription failed",
                    "Vellora could not reach the transcription service.",
                    &err,
                ),
            );
            return Err(err);
        }
    };
    let transcription_duration_ms = transcription_started.elapsed().as_millis() as u64;
    drop(audio_path);

    if raw_text.trim().is_empty() {
        record_diagnostic(
            &app,
            &state,
            diagnostic(
                DiagnosticCode::OpenAiBadResponse,
                "Empty transcript",
                "OpenAI returned an empty transcript.",
                None,
                true,
            ),
        );
        anyhow::bail!("OpenAI returned an empty transcript.");
    }

    let cleanup_mode = if settings.cleanup_enabled {
        CleanupMode::Light
    } else {
        CleanupMode::Off
    };

    let cleanup_started = Instant::now();
    let (final_text, cleanup_error) = if settings.cleanup_enabled {
        set_state(
            &app,
            &state,
            DictationState::Cleaning,
            Some("Cleaning transcript...".to_string()),
        );
        emit_overlay_state(
            &app,
            &state,
            DictationState::Cleaning,
            Some("Cleaning".to_string()),
            None,
        );
        match openai::cleanup(&api_key, &settings.cleanup_model, &raw_text).await {
            Ok(text) if !text.trim().is_empty() => (text, None),
            Ok(_) => (
                raw_text.clone(),
                Some("Cleanup returned empty text; using raw transcript.".to_string()),
            ),
            Err(err) => {
                record_diagnostic(
                    &app,
                    &state,
                    diagnostic_from_error(
                        openai::diagnostic_code(&err),
                        "Cleanup failed",
                        "Cleanup failed, so Vellora kept the raw transcript.",
                        &err,
                    ),
                );
                (
                    raw_text.clone(),
                    Some(format!("Cleanup failed; using raw transcript. {err:#}")),
                )
            }
        }
    } else {
        (raw_text.clone(), None)
    };
    let cleanup_duration_ms = if settings.cleanup_enabled {
        Some(cleanup_started.elapsed().as_millis() as u64)
    } else {
        None
    };

    let paste_started = Instant::now();
    let (pasted, paste_error) =
        deliver_transcript(&app, &state, &settings, &final_text, session.paste_target).await;
    let paste_duration_ms = paste_started.elapsed().as_millis() as u64;

    let (word_count, char_count) = history::counts(&final_text);
    let item = TranscriptHistoryItem {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        provider: settings.provider,
        stt_model: settings.stt_model,
        cleanup_model: if settings.cleanup_enabled {
            Some(settings.cleanup_model)
        } else {
            None
        },
        cleanup_mode,
        raw_text,
        final_text,
        duration_ms: Some(transcription_duration_ms),
        word_count,
        char_count,
        pasted,
        error: cleanup_error.or(paste_error),
        pipeline_started_at: Some(pipeline_started.to_rfc3339()),
        recording_duration_ms: Some(recording_duration_ms),
        transcription_duration_ms: Some(transcription_duration_ms),
        cleanup_duration_ms,
        paste_duration_ms: Some(paste_duration_ms),
        total_pipeline_duration_ms: Some(total_started.elapsed().as_millis() as u64),
    };

    if settings.save_history {
        if let Err(err) = history::insert(&item) {
            record_diagnostic(
                &app,
                &state,
                diagnostic(
                    DiagnosticCode::HistoryWriteFailed,
                    "History write failed",
                    "Transcript is ready, but Vellora could not save it to history.",
                    Some(format!("{err:#}")),
                    true,
                ),
            );
        }
    }

    {
        let mut snapshot = state.snapshot.lock().unwrap();
        snapshot.state = DictationState::Done;
        snapshot.message = item.error.clone().or_else(|| {
            Some(
                if pasted {
                    "Transcript pasted."
                } else {
                    "Transcript ready."
                }
                .to_string(),
            )
        });
        snapshot.last_transcript = Some(item.clone());
        snapshot.recording_mode = None;
    }
    set_state(
        &app,
        &state,
        DictationState::Done,
        item.error.clone().or_else(|| {
            Some(
                if pasted {
                    "Transcript pasted."
                } else {
                    "Transcript ready."
                }
                .to_string(),
            )
        }),
    );
    let generation = state.state_generation.load(Ordering::SeqCst);
    let _ = app.emit_to("main", "transcript-created", item);
    emit_overlay_state(
        &app,
        &state,
        DictationState::Done,
        Some(if pasted { "Pasted" } else { "Ready to copy" }.to_string()),
        None,
    );

    let app_for_idle = app.clone();
    let state_for_idle = state.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1200)).await;
        reset_terminal_to_idle(&app_for_idle, &state_for_idle, generation);
    });
    Ok(())
}

async fn deliver_transcript(
    app: &tauri::AppHandle,
    state: &Arc<RuntimeState>,
    settings: &AppSettings,
    text: &str,
    target: isize,
) -> (bool, Option<String>) {
    if !settings.auto_paste && !settings.copy_to_clipboard {
        return (false, None);
    }
    let previous = if settings.auto_paste && !settings.copy_to_clipboard {
        match clipboard::capture_text() {
            Ok(previous) => previous,
            Err(err) => return (false, Some(err.to_string())),
        }
    } else {
        None
    };
    let clipboard_sequence = match clipboard::copy_text_with_sequence(text) {
        Ok(sequence) => sequence,
        Err(err) => {
            record_diagnostic(
                app,
                state,
                diagnostic_from_error(
                    DiagnosticCode::ClipboardFailed,
                    "Clipboard failed",
                    "Transcript is ready, but could not be copied. Use Copy to try again.",
                    &err,
                ),
            );
            return (false, Some(format!("Could not copy transcript. {err:#}")));
        }
    };
    if !settings.auto_paste {
        return (false, None);
    }
    set_state(
        app,
        state,
        DictationState::Pasting,
        Some("Pasting...".to_string()),
    );
    emit_overlay_state(
        app,
        state,
        DictationState::Pasting,
        Some("Pasting".to_string()),
        None,
    );
    let result = clipboard::paste_into(Some(target));
    let mut warning = result.as_ref().err().map(|err| err.to_string());
    if !settings.copy_to_clipboard {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Err(err) = clipboard::restore_text_if_unchanged(previous, clipboard_sequence) {
            record_diagnostic(
                app,
                state,
                diagnostic_from_error(
                    DiagnosticCode::ClipboardFailed,
                    "Clipboard restore failed",
                    "Vellora could not restore the previous text clipboard.",
                    &err,
                ),
            );
            warning = Some(format!("Could not restore the clipboard. {err:#}"));
        }
    }
    if let Err(err) = &result {
        record_diagnostic(
            app,
            state,
            diagnostic_from_error(
                DiagnosticCode::PasteFailed,
                "Paste skipped or failed",
                "The transcript is ready. Copy it manually to your destination.",
                err,
            ),
        );
    }
    (result.is_ok(), warning)
}

fn set_state(
    app: &tauri::AppHandle,
    state: &Arc<RuntimeState>,
    next: DictationState,
    message: Option<String>,
) {
    state.state_generation.fetch_add(1, Ordering::SeqCst);
    {
        let mut snapshot = state.snapshot.lock().unwrap();
        snapshot.state = next.clone();
        snapshot.message = message;
        if !matches!(next, DictationState::Recording) {
            snapshot.recording_mode = None;
        }
    }
    let health = {
        let mut health = state.health.lock().unwrap();
        health.active_operation = match next {
            DictationState::Idle | DictationState::Done | DictationState::Error => None,
            DictationState::Recording => Some("Recording".to_string()),
            DictationState::Transcribing => Some("Transcribing".to_string()),
            DictationState::Cleaning => Some("Cleaning".to_string()),
            DictationState::Pasting => Some("Pasting".to_string()),
        };
        health.clone()
    };
    let _ = app.emit("dictation-state-changed", next);
    let _ = app.emit_to("main", "app-health-changed", health);
}

fn current_session_id(state: &Arc<RuntimeState>) -> Option<String> {
    state
        .recording_session
        .lock()
        .unwrap()
        .as_ref()
        .map(|session| session.id.clone())
}

fn set_recording_mode(
    app: &tauri::AppHandle,
    state: &Arc<RuntimeState>,
    mode: RecordingMode,
    epoch: u64,
) -> bool {
    let mut current = state.recording_session.lock().unwrap();
    let changed = hotkey::with_current_epoch(epoch, || {
        let Some(session) = current
            .as_mut()
            .filter(|session| session.hotkey_epoch == epoch)
        else {
            return false;
        };
        session.change_mode(mode.clone());
        true
    })
    .unwrap_or(false);
    if !changed {
        return false;
    }
    // Keep the session reserved through publication, so an automatic stop
    // cannot publish its terminal state before this recording-mode update.
    {
        let mut snapshot = state.snapshot.lock().unwrap();
        snapshot.recording_mode = Some(mode.clone());
        snapshot.message = Some("Locked recording...".to_string());
    }
    emit_overlay_state(
        app,
        state,
        DictationState::Recording,
        Some("Locked recording".to_string()),
        Some(0.0),
    );
    true
}

fn fail(app: &tauri::AppHandle, state: &Arc<RuntimeState>, message: String) {
    *state.recording_session.lock().unwrap() = None;
    set_state(app, state, DictationState::Error, Some(message.clone()));
    let generation = state.state_generation.load(Ordering::SeqCst);
    let _ = app.emit_to("main", "dictation-error", message.clone());
    emit_overlay_state(app, state, DictationState::Error, Some(message), None);

    let app_for_idle = app.clone();
    let state_for_idle = state.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(4000)).await;
        reset_terminal_to_idle(&app_for_idle, &state_for_idle, generation);
    });
}

fn reset_terminal_to_idle(app: &tauri::AppHandle, state: &Arc<RuntimeState>, generation: u64) {
    let busy = state.busy.lock().unwrap();
    if *busy
        || state.state_generation.load(Ordering::SeqCst) != generation
        || !matches!(
            state.snapshot.lock().unwrap().state,
            DictationState::Done | DictationState::Error
        )
    {
        return;
    }
    set_state(app, state, DictationState::Idle, Some("Ready".to_string()));
    emit_overlay_state(
        app,
        state,
        DictationState::Idle,
        Some("Ready".to_string()),
        None,
    );
}

fn emit_overlay_state(
    app: &tauri::AppHandle,
    state: &Arc<RuntimeState>,
    next: DictationState,
    message: Option<String>,
    level: Option<f32>,
) {
    let settings = state.settings.lock().unwrap().clone();
    if level.is_none() || message.is_some() {
        let (width, height) = overlay_dimensions(&next);
        position_overlay(app, width, height);
    }
    let payload = OverlaySnapshot {
        state: next,
        message,
        level,
        model: Some(settings.stt_model.as_str().to_string()),
        cleanup_enabled: Some(settings.cleanup_enabled),
        last_transcript_preview: None,
        recording_mode: state.snapshot.lock().unwrap().recording_mode.clone(),
    };
    let _ = app.emit("overlay-state-changed", payload);
}

fn overlay_dimensions(state: &DictationState) -> (f64, f64) {
    match state {
        DictationState::Idle => (64.0, 24.0),
        _ => (340.0, 64.0),
    }
}

fn default_health(phase: StartupPhase, ready: bool) -> AppHealthSnapshot {
    AppHealthSnapshot {
        ready,
        startup_phase: phase,
        last_error: None,
        active_operation: None,
        backend_started_at: STARTED_AT
            .get()
            .cloned()
            .unwrap_or_else(|| Utc::now().to_rfc3339()),
        single_instance: true,
    }
}

fn set_early_health(
    health: &Arc<Mutex<AppHealthSnapshot>>,
    phase: StartupPhase,
    active_operation: Option<String>,
) {
    let mut health = health.lock().unwrap();
    health.ready = matches!(phase, StartupPhase::Ready);
    health.startup_phase = phase;
    health.active_operation = active_operation;
}

fn set_health(
    app: &tauri::AppHandle,
    state: &Arc<RuntimeState>,
    phase: StartupPhase,
    active_operation: Option<String>,
) {
    let snapshot = {
        let mut health = state.health.lock().unwrap();
        health.ready = matches!(phase, StartupPhase::Ready);
        health.startup_phase = phase;
        health.active_operation = active_operation;
        health.clone()
    };
    let _ = app.emit_to("main", "app-health-changed", snapshot);
}

fn diagnostic(
    code: DiagnosticCode,
    title: impl Into<String>,
    message: impl Into<String>,
    detail: Option<String>,
    recoverable: bool,
) -> DiagnosticEvent {
    DiagnosticEvent {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        code,
        title: title.into(),
        message: message.into(),
        detail,
        recoverable,
    }
}

fn diagnostic_from_error(
    code: DiagnosticCode,
    title: impl Into<String>,
    message: impl Into<String>,
    err: &anyhow::Error,
) -> DiagnosticEvent {
    diagnostic(code, title, message, Some(format!("{err:#}")), true)
}

fn record_diagnostic(app: &tauri::AppHandle, state: &Arc<RuntimeState>, event: DiagnosticEvent) {
    let health = {
        let mut health = state.health.lock().unwrap();
        health.last_error = Some(event.clone());
        if !health.ready {
            health.startup_phase = StartupPhase::Degraded;
        }
        health.clone()
    };
    let _ = app.emit_to("main", "diagnostic-event", event);
    let _ = app.emit_to("main", "app-health-changed", health);
}

fn to_string(err: anyhow::Error) -> String {
    format!("{err:#}")
}

#[cfg(test)]
mod recording_session_tests {
    use super::*;

    fn session(id: &str) -> RecordingSession {
        RecordingSession {
            id: id.to_string(),
            mode: RecordingMode::Hold,
            started_at_utc: chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            started_at_instant: Instant::now(),
            paste_target: 0,
            hotkey_epoch: 0,
            release_epoch: 0,
            pending_hold_release: None,
        }
    }

    // The hotkey module separately verifies its real mutex/queue epoch gate.
    // Keep these worker interleavings local and free of microphone, keyring,
    // app-data, or the process-global keyboard listener.
    impl RecordingEpochGate for Mutex<u64> {
        fn with_current_epoch<T>(&self, epoch: u64, operation: impl FnOnce() -> T) -> Option<T> {
            let current = self.lock().unwrap();
            (*current == epoch).then(operation)
        }
    }

    #[test]
    fn failed_blocked_start_cannot_replay_a_queued_lock_as_a_new_recording() {
        let epoch = Mutex::new(0);
        let mut current = None;
        assert!(reserve_recording_session(
            &epoch,
            &mut current,
            session("starting")
        ));

        // The hook queues a lock gesture while device startup is blocked.
        let queued_epoch = current.as_ref().unwrap().hotkey_epoch;
        current.take(); // Device initialization failed.
        *epoch.lock().unwrap() += 1;

        let mut queued_lock = session("queued-lock");
        queued_lock.hotkey_epoch = queued_epoch;
        queued_lock.change_mode(RecordingMode::Locked);
        assert!(!reserve_recording_session(
            &epoch,
            &mut current,
            queued_lock
        ));
        assert!(current.is_none());

        let mut fresh = session("fresh");
        fresh.hotkey_epoch = *epoch.lock().unwrap();
        assert!(reserve_recording_session(&epoch, &mut current, fresh));
        assert_eq!(current.unwrap().mode, RecordingMode::Hold);
    }

    #[test]
    fn short_hold_finish_cannot_replay_a_queued_resume_after_busy_clears() {
        let epoch = Mutex::new(0);
        let mut current = Some(session("short"));
        let release = current.as_mut().unwrap().defer_hold_release().unwrap();
        let queued_resume_epoch = current.as_ref().unwrap().hotkey_epoch;

        // The timer wins before the worker reaches a queued partial re-press.
        assert!(claim_recording_session(&mut current, Some("short"), Some(release)).is_some());
        *epoch.lock().unwrap() += 1;
        // The too-short path ends without an API call and clears busy. A stale
        // resume still must not reserve a session, even with no active owner.
        let mut queued_resume = session("queued-resume");
        queued_resume.hotkey_epoch = queued_resume_epoch;
        assert!(!reserve_recording_session(
            &epoch,
            &mut current,
            queued_resume
        ));
        assert!(current.is_none());
    }

    #[test]
    fn start_commit_rechecks_epoch_after_a_precheck_and_preserves_current_owner() {
        let epoch = Mutex::new(0);
        let mut current = None;
        let candidate = session("old-precheck");
        assert_eq!(candidate.hotkey_epoch, *epoch.lock().unwrap());

        // The prior operation resets while the credential read is in flight.
        *epoch.lock().unwrap() += 1;
        assert!(!reserve_recording_session(&epoch, &mut current, candidate));
        assert!(current.is_none());

        let mut fresh = session("fresh-after-reset");
        fresh.hotkey_epoch = *epoch.lock().unwrap();
        assert!(reserve_recording_session(&epoch, &mut current, fresh));
        let mut duplicate = session("duplicate");
        duplicate.hotkey_epoch = *epoch.lock().unwrap();
        assert!(!reserve_recording_session(&epoch, &mut current, duplicate));
        assert_eq!(current.unwrap().id, "fresh-after-reset");
    }

    #[test]
    fn resumed_hold_survives_its_original_release_timer() {
        let mut current = Some(session("held"));
        let release = current.as_mut().unwrap().defer_hold_release().unwrap();
        assert!(current.as_mut().unwrap().resume_hold());

        assert!(claim_recording_session(&mut current, Some("held"), Some(release)).is_none());
        let retained = current.unwrap();
        assert_eq!(retained.mode, RecordingMode::Hold);
        assert!(retained.pending_hold_release.is_none());
    }

    #[test]
    fn old_timer_cannot_shorten_a_new_release_grace_period() {
        let mut current = Some(session("held"));
        let first = current.as_mut().unwrap().defer_hold_release().unwrap();
        current.as_mut().unwrap().resume_hold();
        let second = current.as_mut().unwrap().defer_hold_release().unwrap();
        assert_ne!(first, second);

        assert!(claim_recording_session(&mut current, Some("held"), Some(first)).is_none());
        assert_eq!(current.as_ref().unwrap().pending_hold_release, Some(second));
        assert_eq!(
            claim_recording_session(&mut current, Some("held"), Some(second))
                .unwrap()
                .id,
            "held"
        );
        assert!(current.is_none());
    }

    #[test]
    fn locked_upgrade_cancels_hold_expiry_but_can_still_stop_immediately() {
        let mut current = Some(session("locked"));
        let release = current.as_mut().unwrap().defer_hold_release().unwrap();
        current.as_mut().unwrap().change_mode(RecordingMode::Locked);
        assert!(!current.as_mut().unwrap().resume_hold());
        assert!(current.as_mut().unwrap().defer_hold_release().is_none());

        assert!(claim_recording_session(&mut current, Some("locked"), Some(release)).is_none());
        assert_eq!(
            claim_recording_session(&mut current, Some("locked"), None)
                .unwrap()
                .mode,
            RecordingMode::Locked
        );
    }

    #[test]
    fn automatic_stop_claims_once_and_old_timer_cannot_stop_a_replacement() {
        let mut current = Some(session("previous"));
        let old_release = current.as_mut().unwrap().defer_hold_release().unwrap();
        assert!(claim_recording_session(&mut current, Some("previous"), None).is_some());
        assert!(
            claim_recording_session(&mut current, Some("previous"), Some(old_release)).is_none()
        );

        current = Some(session("replacement"));
        let new_release = current.as_mut().unwrap().defer_hold_release().unwrap();
        // Release epochs intentionally begin alike: the session ID also has to
        // match, so a previous session's timeout cannot claim a newer one.
        assert_eq!(old_release, new_release);
        assert!(
            claim_recording_session(&mut current, Some("previous"), Some(old_release)).is_none()
        );
        assert_eq!(current.as_ref().unwrap().id, "replacement");
        assert!(
            claim_recording_session(&mut current, Some("replacement"), Some(new_release)).is_some()
        );
    }

    #[test]
    fn timer_and_immediate_stop_race_can_claim_only_once() {
        for timer_first in [true, false] {
            let mut current = Some(session("recording"));
            let release = current.as_mut().unwrap().defer_hold_release().unwrap();
            let first = if timer_first { Some(release) } else { None };
            let second = if timer_first { None } else { Some(release) };
            assert!(claim_recording_session(&mut current, Some("recording"), first).is_some());
            assert!(claim_recording_session(&mut current, Some("recording"), second).is_none());
        }
    }

    #[test]
    fn missing_identity_or_release_token_never_claims_a_live_hold() {
        let mut current = Some(session("recording"));
        assert!(claim_recording_session(&mut current, None, None).is_none());
        assert!(claim_recording_session(&mut current, Some("foreign"), None).is_none());
        assert!(claim_recording_session(&mut current, Some("recording"), Some(1)).is_none());
        assert_eq!(current.unwrap().id, "recording");
    }
}
