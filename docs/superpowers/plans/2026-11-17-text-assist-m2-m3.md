# Text Assist M2 + M3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Select text in any Windows editor, tap Shift+Win, and get it proofread in place by a fast OpenAI model, with a colored halo showing progress (spec: [TEXT_ASSIST_PLAN.md](../../TEXT_ASSIST_PLAN.md), milestones M2 and M3).

**Architecture:** An independent `AssistChordMachine` runs in the existing low-level keyboard hook and emits tap/hold events to a worker. A job orchestrator captures the selection through a clipboard round trip, streams a provider request through the `TextEngine` trait, checks the output with a guard, verifies the target is unchanged, and pastes. A transparent, click-through `assist-halo` window draws the box/ring/label.

**Tech Stack:** Rust (Tauri 2, `windows` 0.58, reqwest 0.12, tokio, rusqlite), React 18 + TypeScript, Vitest.

**Branch:** `feature/text-assist`. Nothing is pushed. Paid calls only in the explicitly consented benchmark (owner approved on November 17, 2026).

---

## Ground rules for every task

- Run Rust checks from the repo root: `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml --locked`, `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`.
- Agents in separate worktrees share one build cache: set `CARGO_TARGET_DIR=C:\dev\projects\vellora-windows\src-tauri\target` (builds wait for each other's lock; that is expected).
- No new crate dependencies unless the task says so.
- Tests never inject real keyboard/mouse input, never touch the real clipboard, and never make network requests outside `127.0.0.1` fixtures.
- New modules start with `#![allow(dead_code)]` only while not yet wired; remove it when wiring.
- Error messages never contain selected text, prompts, response bodies or keys.
- Commit at the end of each task with a descriptive message ending in `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## File map

| File | Status | Owner task |
| --- | --- | --- |
| `src-tauri/src/text_providers/mod.rs` | done (foundation) | `Effort`, `ProviderKind`, `Billing`, `ModelRef`, `TextRequest`, `TextOutcome`, `TokenUsage`, `FinishReason`, `TextError`, `CancelToken`, `TextEngine` |
| `src-tauri/src/text_providers/offline.rs` | done | `UppercaseEngine` (dev transform) |
| `src-tauri/src/text_assist/prompts.rs` | done | Polish prompt, presets, `instructions_for`, `max_output_tokens` |
| `src-tauri/src/assist_chord.rs` | new | Task A |
| `src-tauri/src/input_activity.rs` | new | Task A |
| `src-tauri/src/hotkey.rs` | modify | Task A |
| `src-tauri/src/text_assist/guard.rs` | new | Task B |
| `src-tauri/src/text_providers/http.rs` | new (moved from `openai.rs`) | Task C |
| `src-tauri/src/text_providers/openai.rs` | new | Task C |
| `src-tauri/src/text_providers/catalog.rs` | new | Task C |
| `src-tauri/src/openai.rs` | modify (use `http.rs`) | Task C |
| `src-tauri/src/types.rs`, `settings.rs`, `history.rs` | modify | Task D |
| `src-tauri/src/clipboard.rs`, `src-tauri/src/input_lock.rs` | modify/new | Task E |
| `src-tauri/src/text_assist/{capture,geometry,replace,job}.rs` | new | Tasks F, G |
| `src-tauri/src/main.rs`, `tauri.conf.json`, `capabilities/*` | modify (wiring only) | Task G |
| `src/views/HaloView.tsx`, `src/lib/*`, settings/history/dashboard views | new/modify | Tasks G, H |

Tasks A-D are independent and run in parallel worktrees. E-H run after merging A-D and the M1 findings.

---

## Task A: Assist chord machine and input activity

**Files:** create `src-tauri/src/assist_chord.rs`, `src-tauri/src/input_activity.rs`; modify `src-tauri/src/hotkey.rs` (hook integration only), `src-tauri/src/main.rs` (`mod` lines only).

### A1. `input_activity.rs`

```rust
/// Marks keyboard/mouse input injected by Vellora (`dwExtraInfo`). "VELL".
pub const VELLORA_INPUT_SIGNATURE: usize = 0x5645_4C4C;

pub fn record_key_down();          // real key-down (not Vellora-injected)
pub fn record_pointer();           // real mouse button-down or wheel
pub fn current() -> u64;           // monotonically increasing activity counter
pub fn mouse_tracking_active() -> bool;
/// True for WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN, WM_XBUTTONDOWN, WM_MOUSEWHEEL,
/// WM_MOUSEHWHEEL. Mouse moves and button-ups are not activity.
pub fn is_counted_mouse_message(message: u32) -> bool;
/// Builds a signed keyboard INPUT for SendInput (virtual key, key-up flag).
pub fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT;
/// Installs WH_MOUSE_LL on the calling thread, which must run a message loop. Sets
/// mouse_tracking_active on success. The callback only increments the counter.
pub(crate) fn install_mouse_hook() -> windows::core::Result<HHOOK>;
```

- The mouse callback ignores events whose `MSLLHOOKSTRUCT.dwExtraInfo == VELLORA_INPUT_SIGNATURE`.
- Needs `windows` features already present (`Win32_UI_WindowsAndMessaging`, `Win32_UI_Input_KeyboardAndMouse`).
- Tests: counter increments, `is_counted_mouse_message` table, `key_input` sets `dwExtraInfo` and `KEYEVENTF_KEYUP`.

### A2. `assist_chord.rs` (pure state machine + hook glue)

```rust
pub const DEFAULT_HOLD_THRESHOLD_MS: u64 = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistChordEvent {
    Armed { press_id: u64 },      // Shift+Win both down, no Ctrl/Alt: worker starts hold-cue timer
    Cancelled { press_id: u64 },  // abandoned: hide any cue
    Tap { press_id: u64 },        // emitted on FULL release, first release < threshold
    Hold { press_id: u64 },       // emitted on FULL release, first release >= threshold
}

pub struct AssistChordMachine { /* key states, phase, press counter, threshold, enabled */ }
impl AssistChordMachine {
    pub fn new(hold_threshold: Duration) -> Self;
    pub fn set_enabled(&mut self, enabled: bool);           // disabling resets to idle silently
    pub fn set_hold_threshold(&mut self, threshold: Duration);
    /// One key event that Vellora did not inject. `down` false means key-up.
    pub fn update(&mut self, vk: u32, down: bool, now: Instant) -> Option<AssistChordEvent>;
    /// After a long input gap: physical modifier state from GetAsyncKeyState.
    pub fn resync(&mut self, shift: bool, win: bool, ctrl: bool, alt: bool) -> Option<AssistChordEvent>;
    /// For the worker's hold-cue timer: still armed, no key released yet, threshold reached.
    pub fn hold_reached(&self, press_id: u64, now: Instant) -> bool;
}

/// Global glue used by hotkey.rs. Events go to the sender registered with set_sender.
pub fn set_sender(sender: std::sync::mpsc::Sender<AssistChordEvent>) -> anyhow::Result<()>;
pub fn on_hook_key(vk: u32, down: bool, up: bool, now: Instant);
pub fn on_stale_resync(shift: bool, win: bool, ctrl: bool, alt: bool);
pub fn hold_reached(press_id: u64, now: Instant) -> bool;
pub fn configure(enabled: bool, hold_threshold: Duration);
```

Semantics (each bullet needs a test):

- Track Left/Right Shift (`0xA0`/`0xA1`, generic `0x10` sets both), Left/Right Win (`0x5B`/`0x5C`), Ctrl (`0xA2`/`0xA3`/`0x11`), Alt (`0xA4`/`0xA5`/`0x12`) separately.
- **Arm:** the key-down that makes "any Shift down AND any Win down" true, with no Ctrl and no Alt down, moves Idle -> Armed with a new `press_id` and emits `Armed`. Shift-then-Win and Win-then-Shift both arm.
- If Shift+Win become down while Ctrl or Alt is down, go to Blocked (no event) until Shift and Win are fully released; releasing Ctrl/Alt first must not arm.
- **Repeats:** a key-down for a key already down is ignored in every phase.
- **Cancel while Armed:** any key-down of Ctrl, Alt or any other non-Shift/non-Win key emits `Cancelled` and moves to Blocked until full release (Win+Shift+S, Win+Shift+arrows keep working because the hook never swallows keys). Pressing the other-side Shift/Win while armed is not a cancel.
- **Release:** the first Shift/Win key-up records `first_release`. Once no Shift and no Win is down, emit `Tap` if `first_release - armed_at < threshold`, else `Hold`, and return to Idle. Other keys pressed after the first release but before full release still cancel.
- Key-ups of unrelated keys are ignored. Blocked -> Idle silently on full Shift/Win release.
- `resync`: set key states from the physical snapshot; if Armed and Shift or Win is no longer down, emit `Cancelled` and go Idle (or Blocked if one is still down).
- Disabled: no events; `set_enabled(false)` while armed resets silently.
- Threshold edge: exactly 350 ms counts as `Hold`.

### A3. `hotkey.rs` integration (recording machine unchanged)

- In `hook_proc`, read `info.dwExtraInfo` and `info.flags`. If `dwExtraInfo != VELLORA_INPUT_SIGNATURE`: on key-down call `input_activity::record_key_down()`, and call `assist_chord::on_hook_key(vk, is_down, is_up, now)`. Vellora-signed events skip both. The existing recording-machine code path is untouched (it still sees every event as before).
- In `reset_stale_state`, after `resync_modifier_state()`, call `assist_chord::on_stale_resync(...)` with `GetAsyncKeyState` for Shift (`VK_LSHIFT`/`VK_RSHIFT`), Win, Ctrl, Alt (`VK_LMENU`/`VK_RMENU`).
- In `start_listener`'s hook thread, after installing the keyboard hook, call `input_activity::install_mouse_hook()`; on failure continue (keyboard-only activity) and unhook it on exit.
- All existing `hotkey.rs` tests still pass unchanged.

---

## Task B: Output guard, whitespace and word diff

**Files:** create `src-tauri/src/text_assist/guard.rs`; add `pub mod guard;` to `src-tauri/src/text_assist/mod.rs`. No new dependencies (hand-written scanners, no regex crate).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding { Lf, CrLf }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSelection { pub core: String, pub leading: String, pub trailing: String, pub line_ending: LineEnding }

pub fn prepare(selection: &str) -> PreparedSelection;
/// Trims the model output's own outer whitespace, normalizes its line endings to the
/// original's dominant style, and re-attaches the original leading/trailing whitespace.
pub fn restore(prepared: &PreparedSelection, model_output: &str) -> String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardIssue { Incomplete, Empty, Preamble, Wrapper, ProtectedTokens, LineCount, TooManyChanges, LengthChange }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardReport { pub issues: Vec<GuardIssue>, pub changed_words: usize, pub original_words: usize, pub fixes: usize }
impl GuardReport {
    pub fn passed(&self) -> bool;
    /// First issue as a history reason code: "guard_incomplete", "guard_empty", "guard_preamble",
    /// "guard_wrapper", "guard_markup", "guard_line_count", "guard_too_many_changes", "guard_length".
    pub fn reason_code(&self) -> Option<&'static str>;
}

/// `original_core`/`result_core` are whitespace-trimmed (from prepare / model output).
/// `completed` is false when the provider reported truncation, refusal or a filter stop.
pub fn check(original_core: &str, result_core: &str, completed: bool) -> GuardReport;

pub fn protected_tokens(text: &str) -> Vec<String>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "op", content = "text", rename_all = "snake_case")]
pub enum DiffOp { Equal(String), Insert(String), Delete(String) }
/// Token-level diff (words, single punctuation chars, whitespace runs), Myers O(ND),
/// adjacent ops of the same kind merged. Falls back to one Delete + one Insert when the
/// edit distance exceeds 4,000 tokens.
pub fn word_diff(original: &str, result: &str) -> Vec<DiffOp>;
```

Rules (each needs positive and negative tests):

1. **Whitespace:** `prepare` splits leading/trailing `char::is_whitespace` runs; dominant line ending is CRLF when `\r\n` occurrences outnumber lone `\n`. `restore` never changes interior whitespace except line-ending normalization; Unicode is kept exactly (no normalization). Test CRLF, LF, mixed, umlauts, ß, emoji, combining marks, whitespace-only.
2. **Complete:** `completed == false` -> `Incomplete`; empty `result_core` -> `Empty`.
3. **Preamble:** result's first line (lowercased, trimmed) starts with one of: `here is`, `here's`, `corrected text`, `corrected version`, `the corrected`, `sure,`, `sure!`, `certainly`, `of course`, `hier ist`, `hier sind`, `hier die`, `korrigierte`, `korrigierter`, `gerne`, `natürlich,`, `selbstverständlich` - and the original's first line does not start with the same phrase.
4. **Wrapper:** result is wrapped in matching quotes (`"…"`, `'…'`, `“…”`, `„…“`, `«…»`, `‚…‘`) or a code fence (starts with three backticks) while the original is not.
5. **Protected tokens identical and in order.** Scan left to right, producing one ordered list:
   - LaTeX mode is on when the text contains a LaTeX command (`\` followed by ASCII letters) or a `$`-delimited pair. Outside LaTeX mode only backtick code, URLs and e-mail addresses are protected (so "50 % der" in normal prose is not a comment).
   - Math segments `$$…$$`, `$…$`, `\(…\)`, `\[…\]` are one token each (their content is not scanned again). Unmatched `$` is plain text. `\$` is not a delimiter.
   - LaTeX commands `\[A-Za-z@]+\*?`; for `\ref`, `\eqref`, `\cite…` (any command starting with `cite`), `\label`, `\input`, `\include`, `\begin`, `\end`, also the following brace argument (skip one optional `[…]` first; brace matching handles nesting).
   - `%` to end of line, excluding `\%` (LaTeX mode only).
   - Inline code in single or triple backticks.
   - URLs starting with `http://`, `https://` or `www.` up to whitespace (strip trailing `.,;:!?)`); e-mail addresses (`local@domain.tld`).
   - Mismatch in count, content or order -> `ProtectedTokens` (reason `guard_markup`).
6. **Line count:** number of lines (after CRLF->LF) must be equal -> else `LineCount`.
7. **Size:** `changed_words = max(deleted word tokens, inserted word tokens)` from `word_diff` (punctuation and whitespace tokens do not count). Fail `TooManyChanges` when `changed_words > max(2, ceil(0.4 * original_words))`. Fail `LengthChange` when `|len(result) - len(original)| > max(10, 0.3 * len(original))` in chars.
8. `fixes` = number of change hunks in `word_diff` (consecutive non-Equal ops form one hunk), ignoring hunks that only change whitespace.

Fixtures to include: German comma fix (passes), English typo fix (passes), single-word typo `teh` -> `the` (passes), LaTeX paragraph with `$x^2$`, `\cite[p.~5]{knuth}`, `\%`, `% comment`, nested braces (passes when untouched; fails when the model "fixes" inside math or a cite key), Markdown with inline code and URL, preamble variants in both languages, quoted output, code-fenced output, added line, rewritten sentence (fails size).

---

## Task C: Shared HTTP, OpenAI streaming engine and catalog

**Files:** create `src-tauri/src/text_providers/{http.rs,openai.rs,catalog.rs}`; add the `pub mod` lines to `text_providers/mod.rs`; modify `src-tauri/src/openai.rs` to use `http.rs`. No new dependencies (use `reqwest::Response::chunk()` for streaming).

### C1. `http.rs`

- Move `client`, `send_with_retry`, `retry_delay`, `retryable_status`, `retry_jitter`, `http_error` and `diagnostic_code` out of `src-tauri/src/openai.rs` into `text_providers/http.rs`; generalize messages with a provider label (`"OpenAI cleanup failed [401] …"` must stay byte-identical for dictation). Dictation code and all existing `openai.rs` tests keep passing (move tests with the code they test).
- Add `pub fn status_error(status: StatusCode) -> TextError` (401/403 -> `InvalidKey`, 429 -> `RateLimited`, 5xx -> `Server(code)`, other 4xx -> `Rejected(code)`) and `pub fn transport_error(err: &reqwest::Error) -> TextError` (timeout -> `Timeout`, connect -> `Network`, else `Network`).

### C2. `catalog.rs`

```rust
pub struct CatalogModel {
    pub provider: ProviderKind, pub id: &'static str, pub label: &'static str,
    pub efforts: &'static [Effort], pub default_effort: Option<Effort>,
    pub input_usd_per_million: f64, pub output_usd_per_million: f64,
    pub color: &'static str, pub shutdown: Option<&'static str>, pub quick_candidate: bool,
}
pub fn models() -> &'static [CatalogModel];
pub fn find(provider: ProviderKind, id: &str) -> Option<&'static CatalogModel>;
/// Effort to send: the ModelRef's effort if allowed, else the default. Catalog models with
/// a disallowed effort -> Err(TextError::Unsupported). Unknown model IDs: Ok(None) when no
/// effort was requested, Err(Unsupported) when one was.
pub fn resolve_effort(model: &ModelRef) -> Result<Option<Effort>, TextError>;
pub fn estimate_cost_usd(model: &ModelRef, usage: &TokenUsage) -> Option<f64>;
pub fn default_color(provider: ProviderKind) -> &'static str; // openai #10a37f, anthropic/claude_cli #d97757, gemini #4285f4, others #8a8f98
```

OpenAI entries (recheck each model page on developers.openai.com before writing; record the check date in a comment): `gpt-6-luna` (efforts none, low, medium, high, xhigh, max; $0.10 / $0.50; quick candidate), `gpt-5.6-luna` (none..max; $0.20 / $1.20; quick candidate), `gpt-6-astra` and `gpt-5.6-terra` (rewrite models; efforts/prices from their pages). Plus `offline-uppercase` (provider `Offline`, no efforts, $0). Anthropic/Gemini entries are added in M5.

### C3. `text_providers/openai.rs`

```rust
pub struct OpenAiEngine { /* key, base_url, client */ }
impl OpenAiEngine {
    pub fn new(api_key: String) -> anyhow::Result<Self>;                 // https://api.openai.com/v1, http::client
    pub fn with_client(api_key: String, base_url: String, client: reqwest::Client) -> Self; // tests
}
impl TextEngine for OpenAiEngine { /* POST {base_url}/responses */ }
pub(crate) fn request_body(request: &TextRequest, effort: Option<Effort>) -> serde_json::Value;
```

- Body: `model`, `instructions`, `input` (the selected text as a string), `store: false`, `stream: true`, `max_output_tokens`, and `reasoning: {effort}` when `resolve_effort` returns one. Never `tools`.
- SSE parsing (own small parser; events may be split across chunks at any byte, including inside UTF-8 sequences): `response.output_text.delta` -> append + `on_delta`; `response.refusal.delta`/`response.refusal.done` -> mark refused; `response.completed` -> `FinishReason::Completed`, usage from `response.usage` (`input_tokens`, `output_tokens`, `output_tokens_details.reasoning_tokens`); `response.incomplete` -> `MaxTokens` when `incomplete_details.reason == "max_output_tokens"`, `ContentFilter` for `content_filter`, else `Other`; `response.failed` / `error` -> `TextError::Server(500)` or `BadResponse` (rate-limit error codes -> `RateLimited`). Stream ending without a terminal event -> `BadResponse`. Refused -> `Err(ProviderRefused)`.
- Retry: only the connection/initial status, through `send_with_retry`, within `request.timeout`. Never retry after a delta was received.
- The whole call (connect + stream) obeys `request.timeout` -> `Timeout`; `cancel` aborts promptly -> `Cancelled` (use `tokio::select!`).
- Loopback fixture tests (pattern: `fixture_client()` + `TcpListener` in existing `openai.rs` tests): normal stream with usage, split chunks/UTF-8 boundary, incomplete max tokens, refusal, 401, 429, 500 twice (retried once then `Server(500)`), malformed JSON, no terminal event, mid-stream cancel returns within 500 ms, timeout. Check the request body (store false, stream true, reasoning effort, input separate from instructions) by reading it in the fixture server.

---

## Task D: Settings block and history table

**Files:** modify `src-tauri/src/types.rs`, `src-tauri/src/settings.rs`, `src-tauri/src/history.rs`.

### D1. Types (`types.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextAssistSettings {
    pub enabled: bool,                       // true
    pub chord: String,                       // "Shift+Win" (only supported value)
    pub hold_threshold_ms: u64,              // 350
    pub quick_model: ModelRef,               // openai / gpt-6-luna / effort none
    pub presets: Vec<RewritePreset>,         // from prompts::BUILTIN_PRESETS, builtin = true
    pub last_preset_id: String,              // "polish"
    pub favorite_models: Vec<ModelRef>,      // [quick model, openai gpt-5.6-luna low]
    pub ensemble_profiles: Vec<EnsembleProfile>, // []
    pub model_colors: BTreeMap<String, String>,  // {}
    pub save_history: bool,                  // true
    pub limits: TextAssistLimits,
    pub cli: CliBridges,
    pub openai_compatible: Vec<CompatEndpoint>,  // []
    pub custom_instructions: Vec<String>,    // last five, newest first
    pub dev_offline_transform: bool,         // false; developer-only uppercase engine
}
pub struct RewritePreset { pub id: String, pub name: String, pub instruction: String,
    #[serde(default)] pub model: Option<ModelRef>, #[serde(default)] pub color: Option<String>, #[serde(default)] pub builtin: bool }
pub struct EnsembleProfile { pub id: String, pub name: String, pub members: Vec<ModelRef> }
pub struct TextAssistLimits { pub quick_max_chars: usize /*8000*/, pub panel_max_chars: usize /*30000*/,
    pub ensemble_max_usd_per_run: f64 /*0.05*/, pub paste_restore_delay_ms: u64 /*400*/ }
pub struct CliBridges { pub codex: CliBridge, pub claude: CliBridge }
pub struct CliBridge { pub enabled: bool, pub path: Option<String> }
pub struct CompatEndpoint { pub id: String, pub name: String, pub base_url: String, pub models: Vec<String> }
```

All structs: `Debug, Clone, PartialEq, Serialize, Deserialize`, `#[serde(rename_all = "camelCase")]`, `#[serde(default)]` with a `Default` impl giving the values above. `AppSettings` gains `#[serde(default)] pub text_assist: TextAssistSettings`. Existing settings files (no `textAssist` key) must load unchanged; a partial `textAssist` object fills missing fields from defaults.

### D2. Validation (`settings.rs`)

Add to `validate_settings` (used on load and save; a failure on load preserves the file as `settings-invalid-*.json`, so keep rules about values only):
- `chord == "Shift+Win"`; `hold_threshold_ms` in 150..=1500.
- Every `ModelRef.model` (quick, presets, favorites, ensemble members): 1..=128 chars of `[A-Za-z0-9._:/-]` (`/` for OpenRouter-style IDs); `endpoint` ids `[a-z0-9_-]{1,40}`.
- Colors (`model_colors` values, preset colors): `#rrggbb`. `model_colors` keys <= 200 chars, at most 64 entries.
- Limits: quick 100..=50_000, panel 100..=200_000, ensemble usd 0.0..=5.0 (finite), restore delay 50..=5_000.
- Presets: 1..=20, ids unique `[a-z0-9_-]{1,40}`, names 1..=40 chars, instructions <= 2_000 chars; `last_preset_id` 1..=40 chars (an unknown id falls back to polish at use time, not a validation error).
- Ensemble profiles <= 20, members 1..=9; favorites <= 12; custom instructions <= 5, each <= 500 chars.
- `openai_compatible`: <= 10, ids unique, `base_url` parses as `https://…`, or `http://` only for `127.0.0.1`, `localhost` or `[::1]`; models <= 50.
- CLI `path`: if set, absolute and <= 260 chars. Existence is checked when the user sets it (later task), never on load.
- Raise `MAX_SETTINGS_BYTES` from 64 KB to 256 KB (presets make files larger).
- Tests: defaults validate; old file without `textAssist` loads with defaults; partial block fills defaults; each rule rejects a bad value; round trip.

### D3. History (`history.rs`)

- Create `text_assist_runs` and its index (spec 7.3, exact columns) in the same schema setup as transcripts (idempotent `CREATE … IF NOT EXISTS`).
- `TextAssistRun` struct (camelCase serde) with all columns; `outcome` and `kind` as `String`s with constants (`KIND_POLISH`, `KIND_REWRITE`, `KIND_ENSEMBLE`; `OUTCOME_APPLIED`, `OUTCOME_NO_CHANGE`, `OUTCOME_KEPT`, `OUTCOME_CANCELLED`, `OUTCOME_FAILED`).
- Functions following the existing transcript patterns (same connection handling): `insert_text_assist_run`, `update_text_assist_run_outcome(id, outcome, reason_code)`, `list_text_assist_runs(limit) -> newest first`, `delete_text_assist_run(id)`, `clear_text_assist_runs()`, `text_assist_cost_totals(today) -> TextAssistCostTotals { runs, applied_runs, estimated_cost_usd, estimated_cost_today_usd, plan_runs }`.
- Tests: migration on an existing transcripts-only database, insert/list order, update, delete, clear, totals.

---

## Task E: Clipboard snapshot, exclusion formats, VS Code markers, input lock (after A-D merge)

- `input_lock.rs`: `pub fn lock() -> MutexGuard<'static, ()>`; dictation `paste_into` and Text Assist capture/replace hold it around clipboard + SendInput.
- `clipboard.rs`: `ClipboardSnapshot::capture() -> Result<(Self, Restored)>` enumerating formats per spec 4.4 (pure format filter `fn keep_format(format: u32) -> bool` and size cap tested), `restore()`, `write_text_excluded(text)` adding `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`=0, `CanUploadToCloudClipboard`=0; `vscode_marker(&snapshot) -> VsCodeCopy { Unknown, Selection, EmptySelection, MultiCursor }` from the M1-confirmed location; `send_copy()`/`send_paste()` with signed input from `input_activity::key_input`. Dictation `paste_into` switches to signed input and the input lock.

## Task F: Capture, geometry, replace (after M1 findings)

- `capture.rs`: `TargetSnapshot { hwnd, pid, exe_name, focus_hwnd, uia_runtime_id, is_password, activity, geometry }`, `capture_selection(limit) -> Result<Captured, CaptureError>` per spec 4.2.
- `geometry.rs`: UIA selection rectangles -> caret -> mouse -> pill, per spec 2.5 (M1 decides which apps get which).
- `replace.rs`: `verify_quick(&TargetSnapshot)`, `replace_selection(text, restore_delay)` per spec 4.3.
- Win32 parts are covered by the manual matrix; pure decision functions get unit tests.

## Task G: Job orchestrator, halo window, wiring

- `job.rs`: one active job, states per spec 3.4, events `text-assist://job` to the halo (`{ jobId, phase, color, label, progress, anchor }`), history row per job, `Kept` handling, cancel on second tap. Orchestrator logic is generic over traits `Capturer`, `Replacer`, `TextEngine`, `HistorySink` so it is tested with fakes (target changed, activity during request, cancel, duplicate chord, guard failure -> Kept, history rows per outcome).
- Worker thread consuming `AssistChordEvent`s; hold-cue timer using `assist_chord::hold_reached`.
- `assist-halo` window: transparent, undecorated, always on top, skip taskbar, click-through (`set_ignore_cursor_events(true)`), shown without activation (`SW_SHOWNOACTIVATE` + `WS_EX_NOACTIVATE`), positioned over the anchor in physical pixels.
- `HaloView.tsx`: box + ring + label; ring states filling/spinning/check/error; colors per model; reduced motion; both appearances.

## Task H: M3 completion

- Quick Polish uses `OpenAiEngine` with `settings.text_assist.quick_model`; dictation cleanup moves onto `text_providers::http`.
- Benchmark: `src-tauri/tests/fixtures/polish_benchmark.json` (~40 synthetic samples) and an ignored test `live_polish_benchmark` gated by `VELLORA_LIVE_PAID_CALLS=1`, reporting exact match, false-change rate, guard pass rate, p50/p95 latency, first token, cost per sample. Results recorded in the spec.
- Settings "Text Assist" section, History "Text Assist" tab (list, diff, copy, delete, clear), Dashboard Text Assist cost; Tauri commands in `text_assist/mod.rs`; TS types/api.
- Docs: PRIVACY, README, TROUBLESHOOTING, CHANGELOG; manual matrix rows recorded.
