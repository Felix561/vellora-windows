//! Text Assist job orchestrator (spec section 3.4).
//!
//! `Idle -> Capturing -> Thinking/Streaming -> Checking -> Replacing -> Done`, ending early as
//! `Failed` (text untouched), `Kept` (result saved, not pasted), `NoChange` or `Cancelled`.
//!
//! The orchestrator only makes decisions. Windows work (snapshot, capture, verify + paste) is
//! behind [`JobPlatform`], presentation and history behind [`JobObserver`], and generation behind
//! [`TextEngine`], so every outcome is unit-tested with fakes.

use std::{
    sync::{
        atomic::{AtomicU8, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use chrono::Utc;

use super::{guard, prompts};
use crate::{
    history::{
        TextAssistRun, KIND_POLISH, OUTCOME_APPLIED, OUTCOME_CANCELLED, OUTCOME_FAILED,
        OUTCOME_KEPT, OUTCOME_NO_CHANGE,
    },
    text_providers::{
        catalog, CancelToken, FinishReason, ModelRef, TextEngine, TextError, TextRequest,
        TokenUsage,
    },
};

/// Quick Polish request budget (spec 3.4).
pub const QUICK_POLISH_TIMEOUT: Duration = Duration::from_secs(20);
/// Progress updates while streaming are throttled to this interval.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(60);

/// Job phases. Stored in an [`AtomicU8`] so the chord worker can read them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Phase {
    Capturing = 1,
    Thinking = 2,
    Streaming = 3,
    Checking = 4,
    Replacing = 5,
    Done = 6,
    NoChange = 7,
    Kept = 8,
    Failed = 9,
    Cancelled = 10,
}

impl Phase {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Phase::Capturing,
            2 => Phase::Thinking,
            3 => Phase::Streaming,
            4 => Phase::Checking,
            5 => Phase::Replacing,
            6 => Phase::Done,
            7 => Phase::NoChange,
            8 => Phase::Kept,
            9 => Phase::Failed,
            10 => Phase::Cancelled,
            _ => return None,
        })
    }

    pub fn is_final(self) -> bool {
        matches!(
            self,
            Phase::Done | Phase::NoChange | Phase::Kept | Phase::Failed | Phase::Cancelled
        )
    }
}

/// Current phase of a running job, shared with the chord worker.
#[derive(Debug, Default)]
pub struct PhaseCell(AtomicU8);

impl PhaseCell {
    pub fn get(&self) -> Option<Phase> {
        Phase::from_u8(self.0.load(Ordering::SeqCst))
    }

    fn set(&self, phase: Phase) {
        self.0.store(phase as u8, Ordering::SeqCst);
    }

    /// A chord tap cancels the job unless it is already pasting or finished (spec 3.4).
    pub fn cancellable(&self) -> bool {
        self.get()
            .is_none_or(|phase| !phase.is_final() && phase != Phase::Replacing)
    }
}

/// A user-facing failure from the platform. `code` is a sanitized history reason code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
}

impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Windows-facing operations. All methods block and are called on blocking threads.
pub trait JobPlatform: Send + Sync + 'static {
    type Target: Clone + Send + Sync + 'static;

    /// Snapshot the foreground target (window, focus, activity counter, geometry).
    fn snapshot(&self) -> Result<Self::Target, Failure>;
    /// Copy the selection through the clipboard round trip and restore the clipboard.
    fn capture(&self, target: &Self::Target, max_chars: usize) -> Result<String, Failure>;
    /// Verify the target is unchanged, then paste `text` over the selection. An error means
    /// nothing was pasted; the job ends as `Kept`.
    fn replace(
        &self,
        target: &Self::Target,
        text: &str,
        restore_delay: Duration,
    ) -> Result<(), Failure>;
    /// Executable file name for history.
    fn target_app(&self, target: &Self::Target) -> Option<String>;
}

/// One presentation update.
#[derive(Debug, Clone, PartialEq)]
pub struct JobUpdate {
    pub job_id: String,
    pub phase: Phase,
    /// 0..1 while streaming.
    pub progress: Option<f32>,
    pub label: String,
}

/// Receives presentation updates and finished history rows.
pub trait JobObserver<T>: Send + Sync {
    fn update(&self, target: Option<&T>, update: &JobUpdate);
    fn record(&self, run: TextAssistRun);
}

/// Immutable options for one Quick Polish job, snapshotted when the chord fired.
#[derive(Debug, Clone)]
pub struct PolishJob {
    pub id: String,
    pub model: ModelRef,
    /// Display name, e.g. "GPT-6 Luna".
    pub model_label: String,
    pub max_chars: usize,
    pub restore_delay: Duration,
    pub timeout: Duration,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq)]
pub struct JobResult {
    pub phase: Phase,
    pub reason_code: Option<&'static str>,
    pub label: String,
    /// The restored result text, when one exists (applied, kept, no-change).
    pub result_text: Option<String>,
}

struct Run<'a, T> {
    job: &'a PolishJob,
    observer: &'a dyn JobObserver<T>,
    phase: &'a PhaseCell,
    started: Instant,
}

impl<T> Run<'_, T> {
    fn emit(&self, target: Option<&T>, phase: Phase, progress: Option<f32>, label: String) {
        self.phase.set(phase);
        self.observer.update(
            target,
            &JobUpdate {
                job_id: self.job.id.clone(),
                phase,
                progress,
                label,
            },
        );
    }
}

/// Everything known when a job finishes, turned into one history row.
struct Finish<'a> {
    phase: Phase,
    reason: Option<&'static str>,
    label: String,
    original: Option<&'a str>,
    result: Option<String>,
    target_app: Option<String>,
    usage: Option<TokenUsage>,
    first_token_ms: Option<u64>,
}

fn finish<T>(run: &Run<'_, T>, target: Option<&T>, done: Finish<'_>) -> JobResult {
    run.emit(target, done.phase, None, done.label.clone());
    if let Some(original) = done.original {
        let outcome = match done.phase {
            Phase::Done => OUTCOME_APPLIED,
            Phase::NoChange => OUTCOME_NO_CHANGE,
            Phase::Kept => OUTCOME_KEPT,
            Phase::Cancelled => OUTCOME_CANCELLED,
            _ => OUTCOME_FAILED,
        };
        let model = &run.job.model;
        run.observer.record(TextAssistRun {
            id: run.job.id.clone(),
            created_at: Utc::now().to_rfc3339(),
            kind: KIND_POLISH.to_string(),
            preset_id: Some(prompts::PRESET_POLISH.to_string()),
            ensemble_run_id: None,
            provider: model.provider.as_str().to_string(),
            model: model.model.clone(),
            effort: model.effort.map(|effort| effort.as_str().to_string()),
            billing: model.provider.billing().as_str().to_string(),
            original_text: original.to_string(),
            result_text: done.result.clone(),
            outcome: outcome.to_string(),
            reason_code: done.reason.map(str::to_string),
            target_app: done.target_app,
            duration_ms: Some(run.started.elapsed().as_millis() as u64),
            first_token_ms: done.first_token_ms,
            input_tokens: done.usage.map(|usage| usage.input_tokens),
            output_tokens: done.usage.map(|usage| usage.output_tokens),
            estimated_cost_usd: done
                .usage
                .and_then(|usage| catalog::estimate_cost_usd(model, &usage)),
        });
    }
    JobResult {
        phase: done.phase,
        reason_code: done.reason,
        label: done.label,
        result_text: done.result,
    }
}

/// User-facing text for provider errors. Never includes provider responses.
pub fn engine_error_label(error: &TextError) -> String {
    match error {
        TextError::MissingKey => "Add an API key in Vellora first".to_string(),
        TextError::InvalidKey => "The provider rejected the API key".to_string(),
        TextError::RateLimited => "Rate limit or quota reached – try later".to_string(),
        TextError::Network => "No connection to the provider".to_string(),
        TextError::Timeout => "The model took too long – text untouched".to_string(),
        TextError::Server(_) => "Provider error – text untouched".to_string(),
        TextError::Rejected(_) => "The provider rejected the request".to_string(),
        TextError::BadResponse => "Unexpected provider response".to_string(),
        TextError::ProviderRefused => "The model refused – text untouched".to_string(),
        TextError::Unsupported => "This model or effort is not supported".to_string(),
        TextError::CliNotInstalled => "The CLI is not installed".to_string(),
        TextError::CliNotSignedIn => "The CLI is not signed in".to_string(),
        TextError::Cancelled => "Cancelled".to_string(),
    }
}

fn fixes_label(fixes: usize) -> String {
    match fixes {
        0 | 1 => "✓ 1 fix · Ctrl+Z undoes".to_string(),
        n => format!("✓ {n} fixes · Ctrl+Z undoes"),
    }
}

async fn blocking<R: Send + 'static>(
    work: impl FnOnce() -> R + Send + 'static,
) -> Result<R, Failure> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| Failure::new("internal_error", "Text Assist stopped unexpectedly"))
}

/// Runs one Quick Polish job to completion and returns how it ended.
pub async fn run_polish<P: JobPlatform>(
    platform: Arc<P>,
    engine: Arc<dyn TextEngine>,
    observer: Arc<dyn JobObserver<P::Target>>,
    job: PolishJob,
    cancel: CancelToken,
    phase: Arc<PhaseCell>,
) -> JobResult {
    let run = Run {
        job: &job,
        observer: observer.as_ref(),
        phase: phase.as_ref(),
        started: Instant::now(),
    };
    let polish_label = format!("{} · polish", job.model_label);
    run.phase.set(Phase::Capturing);

    // 1. Target snapshot.
    let snapshot = {
        let platform = platform.clone();
        blocking(move || platform.snapshot()).await.and_then(|r| r)
    };
    let target = match snapshot {
        Ok(target) => target,
        Err(failure) => {
            return finish(
                &run,
                None,
                Finish {
                    phase: Phase::Failed,
                    reason: Some(failure.code),
                    label: failure.message,
                    original: None,
                    result: None,
                    target_app: None,
                    usage: None,
                    first_token_ms: None,
                },
            )
        }
    };
    let target_app = platform.target_app(&target);
    run.emit(Some(&target), Phase::Capturing, None, polish_label.clone());

    // 2. Selection capture.
    let captured = {
        let platform = platform.clone();
        let target = target.clone();
        let max_chars = job.max_chars;
        blocking(move || platform.capture(&target, max_chars))
            .await
            .and_then(|r| r)
    };
    let selection = match captured {
        Ok(text) => text,
        Err(failure) => {
            return finish(
                &run,
                Some(&target),
                Finish {
                    phase: Phase::Failed,
                    reason: Some(failure.code),
                    label: failure.message,
                    original: None,
                    result: None,
                    target_app,
                    usage: None,
                    first_token_ms: None,
                },
            )
        }
    };
    let prepared = guard::prepare(&selection);
    let cancelled = |run: &Run<'_, P::Target>, usage, first_token_ms| {
        finish(
            run,
            Some(&target),
            Finish {
                phase: Phase::Cancelled,
                reason: Some("cancelled"),
                label: "Cancelled – text untouched".to_string(),
                original: Some(&selection),
                result: None,
                target_app: target_app.clone(),
                usage,
                first_token_ms,
            },
        )
    };
    if prepared.core.is_empty() {
        return finish(
            &run,
            Some(&target),
            Finish {
                phase: Phase::Failed,
                reason: Some("empty_selection"),
                label: "Select some text first".to_string(),
                original: None,
                result: None,
                target_app,
                usage: None,
                first_token_ms: None,
            },
        );
    }
    if cancel.is_cancelled() {
        return cancelled(&run, None, None);
    }

    // 3. Request with streaming progress.
    run.emit(Some(&target), Phase::Thinking, None, polish_label.clone());
    let total_chars = prepared.core.chars().count().max(1);
    let received = AtomicUsize::new(0);
    let last_progress = Mutex::new(None::<Instant>);
    let on_delta = |piece: &str| {
        let count =
            received.fetch_add(piece.chars().count(), Ordering::SeqCst) + piece.chars().count();
        let now = Instant::now();
        let mut last = last_progress.lock().unwrap();
        if last.is_some_and(|at| now.duration_since(at) < PROGRESS_INTERVAL) {
            return;
        }
        *last = Some(now);
        drop(last);
        let progress = (count as f32 / total_chars as f32).min(0.99);
        run.emit(
            Some(&target),
            Phase::Streaming,
            Some(progress),
            polish_label.clone(),
        );
    };
    let request = TextRequest {
        model: job.model.clone(),
        instructions: prompts::POLISH_INSTRUCTIONS.to_string(),
        input: prepared.core.clone(),
        max_output_tokens: prompts::max_output_tokens(&prepared.core),
        timeout: job.timeout,
    };
    let outcome = engine.stream(request, cancel.clone(), &on_delta).await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(TextError::Cancelled) => return cancelled(&run, None, None),
        Err(error) => {
            return finish(
                &run,
                Some(&target),
                Finish {
                    phase: Phase::Failed,
                    reason: Some(error.reason_code()),
                    label: engine_error_label(&error),
                    original: Some(&selection),
                    result: None,
                    target_app,
                    usage: None,
                    first_token_ms: None,
                },
            )
        }
    };
    let usage = outcome.usage;
    let first_token_ms = outcome.first_token_ms;

    // 4. Output checks.
    run.emit(Some(&target), Phase::Checking, Some(1.0), polish_label);
    let completed = outcome.finish == FinishReason::Completed;
    let restored = guard::restore(&prepared, &outcome.text);
    if completed && restored == selection {
        return finish(
            &run,
            Some(&target),
            Finish {
                phase: Phase::NoChange,
                reason: None,
                label: "No changes needed".to_string(),
                original: Some(&selection),
                result: Some(restored),
                target_app,
                usage,
                first_token_ms,
            },
        );
    }
    let report = guard::check(&prepared.core, outcome.text.trim(), completed);
    if !report.passed() {
        return finish(
            &run,
            Some(&target),
            Finish {
                phase: Phase::Kept,
                reason: report.reason_code(),
                label: "Polish looked wrong – review in Vellora".to_string(),
                original: Some(&selection),
                result: (!restored.trim().is_empty()).then_some(restored),
                target_app,
                usage,
                first_token_ms,
            },
        );
    }
    if cancel.is_cancelled() {
        return cancelled(&run, usage, first_token_ms);
    }

    // 5. Verify and replace. From here a chord no longer cancels.
    run.emit(
        Some(&target),
        Phase::Replacing,
        Some(1.0),
        format!("{} · applying", job.model_label),
    );
    let replaced = {
        let platform = platform.clone();
        let target = target.clone();
        let text = restored.clone();
        let delay = job.restore_delay;
        blocking(move || platform.replace(&target, &text, delay))
            .await
            .and_then(|r| r)
    };
    match replaced {
        Ok(()) => finish(
            &run,
            Some(&target),
            Finish {
                phase: Phase::Done,
                reason: None,
                label: fixes_label(report.fixes),
                original: Some(&selection),
                result: Some(restored),
                target_app,
                usage,
                first_token_ms,
            },
        ),
        Err(failure) => finish(
            &run,
            Some(&target),
            Finish {
                phase: Phase::Kept,
                reason: Some(failure.code),
                label: "Result ready – open Vellora".to_string(),
                original: Some(&selection),
                result: Some(restored),
                target_app,
                usage,
                first_token_ms,
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::text_providers::{
        offline::UppercaseEngine, DeltaSink, Effort, ProviderKind, TextFuture, TextOutcome,
    };

    #[derive(Debug, Clone, PartialEq)]
    struct FakeTarget(u32);

    struct FakePlatform {
        snapshot_error: Option<Failure>,
        selection: Result<String, Failure>,
        replace_error: Option<Failure>,
        replaced: Mutex<Vec<String>>,
    }

    impl Default for FakePlatform {
        fn default() -> Self {
            Self {
                snapshot_error: None,
                selection: Err(Failure::new("no_selection", "Select some text first")),
                replace_error: None,
                replaced: Mutex::new(Vec::new()),
            }
        }
    }

    impl FakePlatform {
        fn with_selection(text: &str) -> Self {
            Self {
                selection: Ok(text.to_string()),
                ..Self::default()
            }
        }
    }

    impl JobPlatform for FakePlatform {
        type Target = FakeTarget;
        fn snapshot(&self) -> Result<FakeTarget, Failure> {
            match &self.snapshot_error {
                Some(failure) => Err(failure.clone()),
                None => Ok(FakeTarget(7)),
            }
        }
        fn capture(&self, _: &FakeTarget, max_chars: usize) -> Result<String, Failure> {
            let text = self.selection.clone()?;
            if text.chars().count() > max_chars {
                return Err(Failure::new("too_long", "Too long"));
            }
            Ok(text)
        }
        fn replace(&self, _: &FakeTarget, text: &str, _: Duration) -> Result<(), Failure> {
            if let Some(failure) = &self.replace_error {
                return Err(failure.clone());
            }
            self.replaced.lock().unwrap().push(text.to_string());
            Ok(())
        }
        fn target_app(&self, _: &FakeTarget) -> Option<String> {
            Some("notepad.exe".to_string())
        }
    }

    #[derive(Default)]
    struct Recorder {
        updates: Mutex<Vec<JobUpdate>>,
        runs: Mutex<Vec<TextAssistRun>>,
    }

    impl JobObserver<FakeTarget> for Recorder {
        fn update(&self, _: Option<&FakeTarget>, update: &JobUpdate) {
            self.updates.lock().unwrap().push(update.clone());
        }
        fn record(&self, run: TextAssistRun) {
            self.runs.lock().unwrap().push(run);
        }
    }

    /// Returns a fixed output (streamed in two pieces) or an error.
    struct FixedEngine {
        output: Result<(String, FinishReason), TextError>,
        cancel_first: bool,
    }

    impl FixedEngine {
        fn text(output: &str) -> Arc<dyn TextEngine> {
            Arc::new(Self {
                output: Ok((output.to_string(), FinishReason::Completed)),
                cancel_first: false,
            })
        }
    }

    impl TextEngine for FixedEngine {
        fn stream<'a>(
            &'a self,
            _request: TextRequest,
            cancel: CancelToken,
            on_delta: DeltaSink<'a>,
        ) -> TextFuture<'a> {
            Box::pin(async move {
                if self.cancel_first {
                    cancel.cancel();
                    return Err(TextError::Cancelled);
                }
                let (text, finish) = self.output.clone()?;
                let middle = text.char_indices().nth(text.chars().count() / 2);
                let split = middle.map_or(text.len(), |(index, _)| index);
                on_delta(&text[..split]);
                on_delta(&text[split..]);
                Ok(TextOutcome {
                    text,
                    finish,
                    usage: Some(TokenUsage {
                        input_tokens: 200,
                        output_tokens: 40,
                        reasoning_tokens: 0,
                    }),
                    first_token_ms: Some(120),
                })
            })
        }
    }

    fn job() -> PolishJob {
        PolishJob {
            id: "job-1".to_string(),
            model: ModelRef::new(ProviderKind::Openai, "gpt-6-luna", Some(Effort::None)),
            model_label: "GPT-6 Luna".to_string(),
            max_chars: 8_000,
            restore_delay: Duration::from_millis(400),
            timeout: QUICK_POLISH_TIMEOUT,
        }
    }

    async fn run(
        platform: FakePlatform,
        engine: Arc<dyn TextEngine>,
        cancel: CancelToken,
    ) -> (JobResult, Arc<FakePlatform>, Arc<Recorder>) {
        let platform = Arc::new(platform);
        let recorder = Arc::new(Recorder::default());
        let result = run_polish(
            platform.clone(),
            engine,
            recorder.clone(),
            job(),
            cancel,
            Arc::new(PhaseCell::default()),
        )
        .await;
        (result, platform, recorder)
    }

    #[tokio::test]
    async fn applies_a_correction_and_records_history() {
        let (result, platform, recorder) = run(
            FakePlatform::with_selection("  i think its fine\r\n"),
            FixedEngine::text("I think it's fine."),
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Done);
        // Outer whitespace and CRLF style come back from the original selection.
        assert_eq!(
            platform.replaced.lock().unwrap().as_slice(),
            ["  I think it's fine.\r\n"]
        );
        assert!(result.label.starts_with('✓'));
        let runs = recorder.runs.lock().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].outcome, OUTCOME_APPLIED);
        assert_eq!(runs[0].original_text, "  i think its fine\r\n");
        assert_eq!(runs[0].target_app.as_deref(), Some("notepad.exe"));
        assert_eq!(runs[0].effort.as_deref(), Some("none"));
        assert_eq!(runs[0].billing, "api");
        assert!(runs[0].estimated_cost_usd.unwrap() > 0.0);
        let phases: Vec<Phase> = recorder
            .updates
            .lock()
            .unwrap()
            .iter()
            .map(|update| update.phase)
            .collect();
        assert_eq!(phases.first(), Some(&Phase::Capturing));
        assert!(phases.contains(&Phase::Thinking));
        assert!(phases.contains(&Phase::Streaming));
        assert_eq!(phases.last(), Some(&Phase::Done));
    }

    #[tokio::test]
    async fn identical_output_is_no_change_and_never_pastes() {
        let (result, platform, recorder) = run(
            FakePlatform::with_selection("All good here.\n"),
            FixedEngine::text("All good here."),
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::NoChange);
        assert!(platform.replaced.lock().unwrap().is_empty());
        assert_eq!(recorder.runs.lock().unwrap()[0].outcome, OUTCOME_NO_CHANGE);
    }

    #[tokio::test]
    async fn guard_failure_keeps_the_result_without_pasting() {
        let (result, platform, recorder) = run(
            FakePlatform::with_selection("Wir sehen $x^2$ in Tabelle \\ref{tab:a}."),
            FixedEngine::text("Wir sehen $x^3$ in Tabelle \\ref{tab:b}."),
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Kept);
        assert_eq!(result.reason_code, Some("guard_markup"));
        assert!(platform.replaced.lock().unwrap().is_empty());
        let runs = recorder.runs.lock().unwrap();
        assert_eq!(runs[0].outcome, OUTCOME_KEPT);
        assert!(runs[0].result_text.is_some());
    }

    #[tokio::test]
    async fn truncated_output_is_kept() {
        let engine: Arc<dyn TextEngine> = Arc::new(FixedEngine {
            output: Ok(("I think".to_string(), FinishReason::MaxTokens)),
            cancel_first: false,
        });
        let (result, platform, _) = run(
            FakePlatform::with_selection("i think its fine"),
            engine,
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Kept);
        assert_eq!(result.reason_code, Some("guard_incomplete"));
        assert!(platform.replaced.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn target_change_at_replace_time_keeps_the_result() {
        let platform = FakePlatform {
            replace_error: Some(Failure::new("user_activity", "typed")),
            ..FakePlatform::with_selection("i think its fine")
        };
        let (result, _, recorder) = run(
            platform,
            FixedEngine::text("I think it's fine."),
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Kept);
        assert_eq!(result.reason_code, Some("user_activity"));
        assert_eq!(result.label, "Result ready – open Vellora");
        assert_eq!(
            recorder.runs.lock().unwrap()[0].result_text.as_deref(),
            Some("I think it's fine.")
        );
    }

    #[tokio::test]
    async fn capture_failure_fails_without_history_text() {
        let (result, _, recorder) = run(
            FakePlatform::default(),
            FixedEngine::text("unused"),
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Failed);
        assert_eq!(result.reason_code, Some("no_selection"));
        assert_eq!(result.label, "Select some text first");
        // Nothing was captured, so there is no history row to write.
        assert!(recorder.runs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn snapshot_refusal_is_reported() {
        let platform = FakePlatform {
            snapshot_error: Some(Failure::new(
                "password_field",
                "Password fields are not supported",
            )),
            ..FakePlatform::with_selection("x")
        };
        let (result, _, _) = run(platform, FixedEngine::text("X"), CancelToken::new()).await;
        assert_eq!(result.phase, Phase::Failed);
        assert_eq!(result.reason_code, Some("password_field"));
    }

    #[tokio::test]
    async fn provider_error_fails_and_records_original() {
        let engine: Arc<dyn TextEngine> = Arc::new(FixedEngine {
            output: Err(TextError::RateLimited),
            cancel_first: false,
        });
        let (result, platform, recorder) = run(
            FakePlatform::with_selection("i think its fine"),
            engine,
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Failed);
        assert_eq!(result.reason_code, Some("rate_limited"));
        assert!(platform.replaced.lock().unwrap().is_empty());
        let runs = recorder.runs.lock().unwrap();
        assert_eq!(runs[0].outcome, OUTCOME_FAILED);
        assert_eq!(runs[0].reason_code.as_deref(), Some("rate_limited"));
    }

    #[tokio::test]
    async fn cancellation_during_the_request_leaves_text_untouched() {
        let engine: Arc<dyn TextEngine> = Arc::new(FixedEngine {
            output: Ok(("unused".to_string(), FinishReason::Completed)),
            cancel_first: true,
        });
        let (result, platform, recorder) = run(
            FakePlatform::with_selection("i think its fine"),
            engine,
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Cancelled);
        assert!(platform.replaced.lock().unwrap().is_empty());
        assert_eq!(recorder.runs.lock().unwrap()[0].outcome, OUTCOME_CANCELLED);
    }

    #[tokio::test]
    async fn cancelled_before_start_never_requests() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let (result, platform, _) = run(
            FakePlatform::with_selection("i think its fine"),
            FixedEngine::text("I think it's fine."),
            cancel,
        )
        .await;
        assert_eq!(result.phase, Phase::Cancelled);
        assert!(platform.replaced.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn offline_uppercase_engine_flows_through_the_guard() {
        // The developer transform changes every word, so the size guard keeps it unless the
        // selection is a single short word; this documents that behavior.
        let engine: Arc<dyn TextEngine> = Arc::new(UppercaseEngine {
            chunk_chars: 4,
            delay: Duration::from_millis(1),
        });
        let (result, platform, _) = run(
            FakePlatform::with_selection("hello"),
            engine,
            CancelToken::new(),
        )
        .await;
        assert_eq!(result.phase, Phase::Done);
        assert_eq!(platform.replaced.lock().unwrap().as_slice(), ["HELLO"]);
    }

    #[test]
    fn phase_cell_cancellable_rules() {
        let cell = PhaseCell::default();
        assert!(cell.cancellable());
        for (phase, cancellable) in [
            (Phase::Capturing, true),
            (Phase::Thinking, true),
            (Phase::Streaming, true),
            (Phase::Checking, true),
            (Phase::Replacing, false),
            (Phase::Done, false),
            (Phase::Kept, false),
        ] {
            cell.set(phase);
            assert_eq!(cell.cancellable(), cancellable, "{phase:?}");
        }
    }
}
