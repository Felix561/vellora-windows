//! Claude plan bridge: the user's own signed-in Claude Code CLI (M6, personal use).
//!
//! Runs `claude -p` in a fresh empty directory with the selected text on stdin and the
//! instructions in a file outside the cwd. Flags verified with `claude --help` (2.1.325) and
//! the CLI reference / headless docs (<https://code.claude.com/docs/en/cli-reference>,
//! <https://code.claude.com/docs/en/headless>) on 2026-11-17:
//!
//! - `-p --output-format stream-json --verbose --include-partial-messages`: streamed
//!   `stream_event` lines (`content_block_delta` / `text_delta`) and a final `result` line.
//! - `--model <alias>` (`haiku`, `sonnet`, `opus` or a full name); `--effort low..max`.
//! - `--system-prompt-file <file>` replaces Claude Code's default agent prompt with the
//!   instructions, so no environment/tool sections are added.
//! - `--tools ""` disables every built-in tool; `--strict-mcp-config` (without `--mcp-config`)
//!   loads no MCP server; `--safe-mode` skips CLAUDE.md, skills, plugins, hooks, MCP servers,
//!   custom commands/agents, output styles and auto memory while keeping the subscription
//!   login; `--disable-slash-commands` keeps a selection starting with `/` from running a
//!   command; `--no-session-persistence` saves no transcript.
//! - `--bare` is NOT used: it never reads the OAuth login, so it cannot use the plan.
//! - A failure inside the run (such as missing authentication) is printed as the `result` on
//!   stdout with `is_error: true`; `system/api_retry` events carry an `error` category such as
//!   `authentication_failed`.
//!
//! Max output tokens cannot be set through a flag; `TextRequest::max_output_tokens` is not
//! applied.

use std::{
    ffi::OsString,
    path::Path,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::{
    catalog,
    cli_common::{self, CliKind, RunDir, RunSpec},
    CancelToken, DeltaSink, Effort, FinishReason, ProviderKind, TextEngine, TextError, TextFuture,
    TextOutcome, TextRequest, TokenUsage,
};

/// Engine for the Claude Code CLI. Cheap to build; the CLI is located on every run.
#[derive(Debug, Clone, Default)]
pub struct ClaudeCliEngine {
    /// Settings path (`textAssist.cli.claude.path`); `None` searches `PATH`.
    path: Option<String>,
}

impl ClaudeCliEngine {
    pub fn new(path: Option<String>) -> Self {
        Self { path }
    }

    async fn run(
        &self,
        request: TextRequest,
        cancel: CancelToken,
        on_delta: DeltaSink<'_>,
    ) -> Result<TextOutcome, TextError> {
        if request.model.provider != ProviderKind::ClaudeCli
            || !cli_common::valid_model_arg(&request.model.model)
        {
            return Err(TextError::Unsupported);
        }
        let effort = catalog::resolve_effort(&request.model)?;
        let program = cli_common::locate(CliKind::Claude, self.path.as_deref())
            .ok_or(TextError::CliNotInstalled)?;
        let timeout = effective_timeout(&request);
        let dir = RunDir::new()?;
        let instructions = dir.write_file("instructions.md", &request.instructions)?;
        let spec = RunSpec {
            program,
            args: claude_args(&request.model.model, effort, &instructions)?,
            cwd: dir.work().to_path_buf(),
            stdin: Some(request.input.into_bytes()),
            env_remove: CliKind::Claude.env_remove(),
            timeout,
            pid_probe: None,
        };
        let started = Instant::now();
        let mut state = StreamState::new(started);
        let exit = cli_common::run(spec, &cancel, &mut |line| state.apply(line, on_delta)).await?;
        drop(dir);
        state.finish(exit.code, &exit.stderr, on_delta)
    }
}

impl TextEngine for ClaudeCliEngine {
    fn stream<'a>(
        &'a self,
        request: TextRequest,
        cancel: CancelToken,
        on_delta: DeltaSink<'a>,
    ) -> TextFuture<'a> {
        Box::pin(self.run(request, cancel, on_delta))
    }
}

/// Command-line arguments. The selected text is never among them.
pub(crate) fn claude_args(
    model: &str,
    effort: Option<Effort>,
    instructions_file: &Path,
) -> Result<Vec<OsString>, TextError> {
    let mut args: Vec<OsString> = [
        "-p",
        // Variadic list; the empty value disables all built-in tools. The next token starts
        // with `--`, so nothing else is taken as a tool name.
        "--tools",
        "",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--strict-mcp-config",
        "--safe-mode",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--model",
        model,
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.push("--system-prompt-file".into());
    args.push(instructions_file.as_os_str().to_owned());
    match effort {
        None => {}
        Some(
            effort @ (Effort::Low | Effort::Medium | Effort::High | Effort::Xhigh | Effort::Max),
        ) => {
            args.push("--effort".into());
            args.push(effort.as_str().into());
        }
        Some(Effort::None | Effort::Minimal) => return Err(TextError::Unsupported),
    }
    Ok(args)
}

/// Accumulates the `stream-json` output.
struct StreamState {
    started: Instant,
    /// Text from `text_delta` events of the main conversation.
    streamed: String,
    /// Text of complete `assistant` messages, used when no partial events arrived.
    assistant_text: String,
    first_token_ms: Option<u64>,
    stop_reason: Option<String>,
    /// Error category from an `assistant` message (`error` field).
    message_error: Option<String>,
    result: Option<ResultLine>,
}

struct ResultLine {
    is_error: bool,
    subtype: String,
    text: Option<String>,
    stop_reason: Option<String>,
    usage: Option<TokenUsage>,
}

impl StreamState {
    fn new(started: Instant) -> Self {
        Self {
            started,
            streamed: String::new(),
            assistant_text: String::new(),
            first_token_ms: None,
            stop_reason: None,
            message_error: None,
            result: None,
        }
    }

    /// Applies one stdout line. Lines that are not JSON are ignored (the stream must still end
    /// with a valid `result`). Errors stop the run.
    fn apply(&mut self, line: &str, on_delta: DeltaSink<'_>) -> Result<(), TextError> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Ok(());
        };
        let str_at = |pointer: &str| value.pointer(pointer).and_then(Value::as_str);
        // Subagent output is not part of the answer (no tools are enabled, so none expected).
        if value
            .get("parent_tool_use_id")
            .is_some_and(|id| !id.is_null())
        {
            return Ok(());
        }
        match str_at("/type").unwrap_or_default() {
            "stream_event" => match str_at("/event/type").unwrap_or_default() {
                "content_block_delta" if str_at("/event/delta/type") == Some("text_delta") => {
                    let text = str_at("/event/delta/text").ok_or(TextError::BadResponse)?;
                    if !text.is_empty() {
                        self.first_token_ms
                            .get_or_insert(self.started.elapsed().as_millis() as u64);
                        self.streamed.push_str(text);
                        on_delta(text);
                    }
                }
                "message_delta" => {
                    if let Some(reason) = str_at("/event/delta/stop_reason") {
                        self.stop_reason = Some(reason.to_string());
                    }
                }
                _ => {}
            },
            "assistant" => {
                if let Some(error) = str_at("/error") {
                    self.message_error = Some(error.to_string());
                }
                if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
                    for block in blocks {
                        if block.get("type").and_then(Value::as_str) == Some("text") {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                self.assistant_text.push_str(text);
                            }
                        }
                    }
                }
            }
            "system" if str_at("/subtype") == Some("api_retry") => {
                // Retrying cannot fix a missing or rejected login; stop early.
                if matches!(
                    str_at("/error"),
                    Some("authentication_failed" | "oauth_org_not_allowed")
                ) {
                    return Err(TextError::CliNotSignedIn);
                }
            }
            "result" => {
                self.result = Some(ResultLine {
                    is_error: value
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    subtype: str_at("/subtype").unwrap_or_default().to_string(),
                    text: str_at("/result").map(str::to_string),
                    stop_reason: str_at("/stop_reason").map(str::to_string),
                    usage: value.get("usage").and_then(read_usage),
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(
        self,
        exit_code: Option<i32>,
        stderr: &str,
        on_delta: DeltaSink<'_>,
    ) -> Result<TextOutcome, TextError> {
        let message_error = self.message_error.as_deref().map(error_from_category);
        let Some(result) = self.result else {
            // No result line: a startup failure (bad flag, crash) or truncated output.
            return Err(match message_error {
                Some(err) => err,
                None if exit_code != Some(0) => cli_common::classify_failure(stderr),
                None => TextError::BadResponse,
            });
        };
        if result.is_error || result.subtype != "success" {
            return Err(message_error.unwrap_or_else(|| {
                cli_common::classify_failure(result.text.as_deref().unwrap_or_default())
            }));
        }
        let finish = match self
            .stop_reason
            .as_deref()
            .or(result.stop_reason.as_deref())
        {
            Some("end_turn" | "stop_sequence") | None => FinishReason::Completed,
            Some("max_tokens") => FinishReason::MaxTokens,
            Some("refusal") => return Err(TextError::ProviderRefused),
            Some(_) => FinishReason::Other,
        };
        let mut first_token_ms = self.first_token_ms;
        let text = if !self.streamed.is_empty() {
            self.streamed
        } else {
            // No partial events (older CLI): deliver the whole answer as one piece.
            let text = result
                .text
                .filter(|text| !text.is_empty())
                .unwrap_or(self.assistant_text);
            if !text.is_empty() {
                first_token_ms = Some(self.started.elapsed().as_millis() as u64);
                on_delta(&text);
            }
            text
        };
        Ok(TextOutcome {
            text,
            finish,
            usage: result.usage,
            first_token_ms,
        })
    }
}

/// `usage` of the `result` line. Cache tokens count as input so estimates stay an upper bound.
fn read_usage(usage: &Value) -> Option<TokenUsage> {
    let field = |name: &str| usage.get(name).and_then(Value::as_u64);
    Some(TokenUsage {
        input_tokens: field("input_tokens")?
            + field("cache_creation_input_tokens").unwrap_or(0)
            + field("cache_read_input_tokens").unwrap_or(0),
        output_tokens: field("output_tokens")?,
        reasoning_tokens: 0,
    })
}

/// Error categories as used by `system/api_retry` and assistant `error` fields.
fn error_from_category(category: &str) -> TextError {
    match category {
        "authentication_failed" | "oauth_org_not_allowed" => TextError::CliNotSignedIn,
        "rate_limit" | "billing_error" | "account_on_hold" => TextError::RateLimited,
        "overloaded" | "server_error" => TextError::Server(529),
        "model_not_found" | "invalid_request" => TextError::Unsupported,
        _ => TextError::BadResponse,
    }
}

/// Total budget actually applied for `request` (the CLI ceiling caps it).
pub fn effective_timeout(request: &TextRequest) -> Duration {
    request.timeout.min(cli_common::CLI_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use super::*;
    use crate::text_providers::{cli_common::fake::FakeCli, ModelRef};

    fn request(model: &str, effort: Option<Effort>, timeout: Duration) -> TextRequest {
        TextRequest {
            model: ModelRef::new(ProviderKind::ClaudeCli, model, effort),
            instructions: "Fix grammar. Reply with the text only.".into(),
            input: "SELECTED-TEXT-7f3a teh text".into(),
            max_output_tokens: 1024,
            timeout,
        }
    }

    async fn run_fake(
        fake: &FakeCli,
        request: TextRequest,
    ) -> (Result<TextOutcome, TextError>, Vec<String>) {
        let engine = ClaudeCliEngine::new(Some(fake.path_string()));
        let pieces: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = {
            let pieces = pieces.clone();
            move |piece: &str| pieces.lock().unwrap().push(piece.to_string())
        };
        let result = engine.stream(request, CancelToken::new(), &sink).await;
        let pieces = pieces.lock().unwrap().clone();
        (result, pieces)
    }

    fn delta(text: &str) -> String {
        serde_json::json!({
            "type": "stream_event",
            "parent_tool_use_id": null,
            "event": {"type": "content_block_delta", "index": 0,
                      "delta": {"type": "text_delta", "text": text}}
        })
        .to_string()
    }

    const INIT: &str =
        r#"{"type":"system","subtype":"init","model":"claude-haiku-5-5","tools":[]}"#;
    const STOP: &str = r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}}"#;

    fn result_line(text: &str) -> String {
        serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": text,
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 5}
        })
        .to_string()
    }

    fn lines(lines: &[&str]) -> String {
        lines.join("\n") + "\n"
    }

    #[tokio::test]
    async fn streams_deltas_and_reads_the_result() {
        let output = lines(&[
            INIT,
            &delta("The ü "),
            &delta("text 👋"),
            STOP,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"The ü text 👋"}]}}"#,
            &result_line("The ü text 👋"),
        ]);
        let fake = FakeCli::printing(&output, 0);
        let (result, pieces) =
            run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        let outcome = result.unwrap();
        assert_eq!(outcome.text, "The ü text 👋");
        assert_eq!(pieces, ["The ü ", "text 👋"]);
        assert_eq!(outcome.finish, FinishReason::Completed);
        assert_eq!(
            outcome.usage,
            Some(TokenUsage {
                input_tokens: 100,
                output_tokens: 5,
                reasoning_tokens: 0
            })
        );
        assert!(outcome.first_token_ms.is_some());

        // The selected text went to stdin only; the instructions went to a file.
        assert!(fake.recorded("stdin.txt").contains("SELECTED-TEXT-7f3a"));
        let args = fake.recorded("args.txt");
        assert!(!args.contains("SELECTED-TEXT"), "{args}");
        assert!(!args.contains("Fix grammar"), "{args}");
        for flag in [
            "-p",
            "--output-format stream-json",
            "--include-partial-messages",
            "--model haiku",
            "--tools \"\"",
            "--safe-mode",
            "--strict-mcp-config",
            "--system-prompt-file",
            "--no-session-persistence",
        ] {
            assert!(args.contains(flag), "{flag} missing in {args}");
        }
        assert!(!args.contains("--bare"));
        assert!(!args.contains("--effort"));
        assert!(cli_common::fake::listing_is_empty(
            &fake.recorded("listing.txt")
        ));
    }

    #[tokio::test]
    async fn without_partial_events_the_result_is_delivered_once() {
        let fake = FakeCli::printing(&lines(&[INIT, &result_line("Whole answer.")]), 0);
        let (result, pieces) = run_fake(
            &fake,
            request("sonnet", Some(Effort::High), Duration::from_secs(30)),
        )
        .await;
        assert_eq!(result.unwrap().text, "Whole answer.");
        assert_eq!(pieces, ["Whole answer."]);
        assert!(fake.recorded("args.txt").contains("--effort high"));
    }

    #[tokio::test]
    async fn malformed_or_truncated_output_is_a_bad_response() {
        let fake = FakeCli::printing("not json\n{\"type\":\"result\",\"subtype\":\"succ\n", 0);
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap_err(), TextError::BadResponse);
        // Stray non-JSON lines before a valid result are tolerated.
        let fake = FakeCli::printing(&lines(&["warning: something", &result_line("ok")]), 0);
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap().text, "ok");
    }

    #[tokio::test]
    async fn not_signed_in_is_reported() {
        let output = lines(&[
            r#"{"type":"assistant","parent_tool_use_id":null,"error":"authentication_failed","message":{"content":[{"type":"text","text":"Not logged in · Please run /login"}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#,
        ]);
        let fake = FakeCli::printing(&output, 1);
        let (result, pieces) =
            run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap_err(), TextError::CliNotSignedIn);
        assert!(pieces.is_empty());

        // Result text only.
        let fake = FakeCli::printing(
            r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#,
            1,
        );
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap_err(), TextError::CliNotSignedIn);

        // A retry event for a rejected login stops the run at once.
        let fake = FakeCli::hanging(
            "{\"type\":\"system\",\"subtype\":\"api_retry\",\"attempt\":1,\"error\":\"authentication_failed\"}\r\n",
        );
        let started = Instant::now();
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap_err(), TextError::CliNotSignedIn);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn non_zero_exit_without_result_is_a_bad_response() {
        let fake = FakeCli::new("", "echo error: unknown option 1>&2\r\nexit /b 1");
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap_err(), TextError::BadResponse);
        let fake = FakeCli::printing(
            r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#,
            1,
        );
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap_err(), TextError::BadResponse);
    }

    #[tokio::test]
    async fn refusal_and_max_tokens() {
        let refusal = r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_delta","delta":{"stop_reason":"refusal"}}}"#;
        let fake = FakeCli::printing(&lines(&[refusal, &result_line("")]), 0);
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap_err(), TextError::ProviderRefused);
        let max = r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}}"#;
        let fake = FakeCli::printing(&lines(&[&delta("cut"), max, &result_line("cut")]), 0);
        let (result, _) = run_fake(&fake, request("haiku", None, Duration::from_secs(30))).await;
        assert_eq!(result.unwrap().finish, FinishReason::MaxTokens);
    }

    #[tokio::test]
    async fn hang_times_out() {
        let fake = FakeCli::hanging(&(delta("Hel") + "\r\n"));
        let started = Instant::now();
        let (result, pieces) =
            run_fake(&fake, request("haiku", None, Duration::from_millis(4000))).await;
        assert_eq!(result.unwrap_err(), TextError::Timeout);
        // The fake prints the delta right after starting (well inside 4 s, even under load).
        assert_eq!(pieces, ["Hel"]);
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(3900) && elapsed < Duration::from_secs(10),
            "{elapsed:?}"
        );
    }

    #[tokio::test]
    async fn cancel_is_prompt() {
        let fake = FakeCli::hanging(&(delta("Hel") + "\r\n"));
        let engine = ClaudeCliEngine::new(Some(fake.path_string()));
        crate::text_providers::factory::test_fixture::assert_cancel_is_prompt(
            &engine,
            request("haiku", None, Duration::from_secs(60)),
        )
        .await;
    }

    #[tokio::test]
    async fn missing_cli_and_bad_requests() {
        let missing = tempfile::tempdir().unwrap().path().join("claude.exe");
        let engine = ClaudeCliEngine::new(Some(missing.display().to_string()));
        let (result, _) = crate::text_providers::factory::test_fixture::run_engine(
            &engine,
            request("haiku", None, Duration::from_secs(5)),
        )
        .await;
        assert_eq!(result.unwrap_err(), TextError::CliNotInstalled);

        let fake = FakeCli::printing("", 0);
        for bad in [
            request(
                "--dangerously-skip-permissions",
                None,
                Duration::from_secs(5),
            ),
            request("haiku", Some(Effort::None), Duration::from_secs(5)),
            TextRequest {
                model: ModelRef::new(ProviderKind::Anthropic, "haiku", None),
                ..request("haiku", None, Duration::from_secs(5))
            },
        ] {
            let (result, _) = run_fake(&fake, bad).await;
            assert_eq!(result.unwrap_err(), TextError::Unsupported);
        }
        // Nothing was started.
        assert_eq!(fake.recorded("args.txt"), "");
    }

    #[test]
    fn effective_timeout_is_capped() {
        let long = request("haiku", None, Duration::from_secs(600));
        assert_eq!(effective_timeout(&long), cli_common::CLI_TIMEOUT);
        let short = request("haiku", None, Duration::from_secs(20));
        assert_eq!(effective_timeout(&short), Duration::from_secs(20));
    }
}
