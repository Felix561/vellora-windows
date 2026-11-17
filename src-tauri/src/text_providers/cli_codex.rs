//! ChatGPT plan bridge: the user's own signed-in Codex CLI (M6, personal use).
//!
//! Runs `codex exec` (non-interactive) in a fresh empty directory with the selected text on
//! stdin. Flags verified with `codex exec --help` (codex-cli 0.169.0) and
//! <https://learn.chatgpt.com/docs/non-interactive-mode> /
//! <https://learn.chatgpt.com/docs/config-file/config-reference> on 2026-11-17:
//!
//! - `exec ... -`: the prompt is read from stdin.
//! - `--json`: JSONL events (`thread.started`, `turn.started`, `item.*`, `turn.completed` with
//!   `usage`, `turn.failed`, `error`). The docs list no text-delta events, so the final agent
//!   message is delivered as one piece when the turn completes.
//! - `--ephemeral` (no session files), `--skip-git-repo-check` (the cwd is not a repo),
//!   `--sandbox read-only`, `--color never`.
//! - `--ignore-user-config` (no `config.toml`: no user MCP servers, hooks, profiles or model
//!   defaults; auth still uses `CODEX_HOME`) and `--ignore-rules` (no execpolicy rules).
//! - `--disable <feature>` for every tool-providing feature listed by `codex features list`
//!   (shell, apps/connectors, plugins, hooks, sub-agents, browser/computer use, images);
//!   `-c web_search="disabled"` removes web search; `-c project_doc_max_bytes=0` reads no
//!   `AGENTS.md`.
//! - `-c developer_instructions=<TOML string>` adds the instructions as developer
//!   instructions next to Codex's built-in ones. The instructions (never the selected text)
//!   are therefore on the command line.
//! - `-c model_reasoning_effort=<effort>`; `--model <id>`.
//! - `CODEX_API_KEY` / `OPENAI_API_KEY` are removed from the environment, so a run uses the
//!   ChatGPT login.
//!
//! Warm mode (`codex app-server`) is not implemented: the subcommand is marked
//! `[experimental]` in 0.169.0 and its JSON-RPC payloads (thread/turn lifecycle, approval
//! requests, streamed item events) cannot be verified without a live model request. A
//! long-lived server would also share one process tree and cwd across runs instead of one
//! fresh, killable tree per run. Expect several seconds of startup per run in exec mode.
//!
//! Max output tokens cannot be set through a flag; `TextRequest::max_output_tokens` is not
//! applied.

use std::{ffi::OsString, time::Instant};

use serde_json::Value;

use super::{
    catalog,
    cli_common::{self, CliKind, RunDir, RunSpec},
    CancelToken, DeltaSink, Effort, FinishReason, ProviderKind, TextEngine, TextError, TextFuture,
    TextOutcome, TextRequest, TokenUsage,
};

/// Features turned off for every run (names from `codex features list`, 0.169.0).
const DISABLED_FEATURES: &[&str] = &[
    "shell_tool",
    "apps",
    "plugins",
    "hooks",
    "multi_agent",
    "browser_use",
    "computer_use",
    "image_generation",
    "view_image",
];

/// Engine for the Codex CLI. Cheap to build; the CLI is located on every run.
#[derive(Debug, Clone, Default)]
pub struct CodexCliEngine {
    /// Settings path (`textAssist.cli.codex.path`); `None` searches `PATH`.
    path: Option<String>,
}

impl CodexCliEngine {
    pub fn new(path: Option<String>) -> Self {
        Self { path }
    }

    async fn run(
        &self,
        request: TextRequest,
        cancel: CancelToken,
        on_delta: DeltaSink<'_>,
    ) -> Result<TextOutcome, TextError> {
        if request.model.provider != ProviderKind::CodexCli
            || !cli_common::valid_model_arg(&request.model.model)
        {
            return Err(TextError::Unsupported);
        }
        let effort = catalog::resolve_effort(&request.model)?;
        let args = codex_args(&request.model.model, effort, &request.instructions)?;
        let program = cli_common::locate(CliKind::Codex, self.path.as_deref())
            .ok_or(TextError::CliNotInstalled)?;
        let dir = RunDir::new()?;
        let spec = RunSpec {
            program,
            args,
            cwd: dir.work().to_path_buf(),
            timeout: request.timeout.min(cli_common::CLI_TIMEOUT),
            stdin: Some(request.input.into_bytes()),
            env_remove: CliKind::Codex.env_remove(),
            pid_probe: None,
        };
        let mut state = EventState::default();
        let started = Instant::now();
        let exit = cli_common::run(spec, &cancel, &mut |line| state.apply(line)).await?;
        drop(dir);
        let outcome = state.finish(exit.code, &exit.stderr)?;
        let first_token_ms = if outcome.text.is_empty() {
            None
        } else {
            on_delta(&outcome.text);
            Some(started.elapsed().as_millis() as u64)
        };
        Ok(TextOutcome {
            first_token_ms,
            ..outcome
        })
    }
}

impl TextEngine for CodexCliEngine {
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
pub(crate) fn codex_args(
    model: &str,
    effort: Option<Effort>,
    instructions: &str,
) -> Result<Vec<OsString>, TextError> {
    let mut args: Vec<OsString> = [
        "exec",
        "--json",
        "--color",
        "never",
        "--ephemeral",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--ignore-user-config",
        "--ignore-rules",
        "--model",
        model,
        "-c",
        "web_search=\"disabled\"",
        "-c",
        "project_doc_max_bytes=0",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    for feature in DISABLED_FEATURES {
        args.push("--disable".into());
        args.push((*feature).into());
    }
    args.push("-c".into());
    args.push(format!("developer_instructions={}", toml_string(instructions)).into());
    match effort {
        None => {}
        Some(
            effort @ (Effort::Low | Effort::Medium | Effort::High | Effort::Xhigh | Effort::Max),
        ) => {
            args.push("-c".into());
            args.push(format!("model_reasoning_effort=\"{}\"", effort.as_str()).into());
        }
        Some(Effort::None | Effort::Minimal) => return Err(TextError::Unsupported),
    }
    // Read the prompt from stdin.
    args.push("-".into());
    Ok(args)
}

/// A single-line TOML basic string. JSON string escapes are valid TOML escapes; TOML also
/// forbids a raw DEL character.
fn toml_string(text: &str) -> String {
    serde_json::to_string(text)
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace('\u{7f}', "\\u007F")
}

/// Accumulates `codex exec --json` events.
#[derive(Debug, Default)]
struct EventState {
    /// Text of the last completed agent message (the final answer).
    last_message: Option<String>,
    completed: bool,
    usage: Option<TokenUsage>,
    /// Message of the last `error` / `turn.failed` event (classified, never surfaced).
    failure: Option<String>,
}

impl EventState {
    /// Applies one stdout line. Non-JSON lines are ignored; the run must still complete.
    fn apply(&mut self, line: &str) -> Result<(), TextError> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Ok(());
        };
        let str_at = |pointer: &str| value.pointer(pointer).and_then(Value::as_str);
        match str_at("/type").unwrap_or_default() {
            "item.completed" => {
                let kind = str_at("/item/type").or(str_at("/item/item_type"));
                if matches!(kind, Some("agent_message" | "assistant_message")) {
                    let text = str_at("/item/text").ok_or(TextError::BadResponse)?;
                    self.last_message = Some(text.to_string());
                }
            }
            "turn.completed" => {
                self.completed = true;
                self.usage = value.get("usage").and_then(read_usage);
            }
            "turn.failed" => {
                // Terminal: stop the run now.
                let message = str_at("/error/message").unwrap_or_default();
                return Err(cli_common::classify_failure(message));
            }
            "error" => {
                // May be followed by a retry; decided at the end.
                self.failure = Some(str_at("/message").unwrap_or_default().to_string());
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(self, exit_code: Option<i32>, stderr: &str) -> Result<TextOutcome, TextError> {
        if self.completed && exit_code == Some(0) {
            return Ok(TextOutcome {
                text: self.last_message.unwrap_or_default(),
                finish: FinishReason::Completed,
                usage: self.usage,
                first_token_ms: None,
            });
        }
        if let Some(failure) = self.failure {
            return Err(cli_common::classify_failure(&failure));
        }
        if exit_code != Some(0) {
            return Err(cli_common::classify_failure(stderr));
        }
        // Exit 0 without a completed turn: truncated or unknown output.
        Err(TextError::BadResponse)
    }
}

/// `turn.completed.usage`: `input_tokens` (cached tokens are a subset, so it stays an upper
/// bound), `output_tokens`, `reasoning_output_tokens`.
fn read_usage(usage: &Value) -> Option<TokenUsage> {
    let field = |name: &str| usage.get(name).and_then(Value::as_u64);
    Some(TokenUsage {
        input_tokens: field("input_tokens")?,
        output_tokens: field("output_tokens")?,
        reasoning_tokens: field("reasoning_output_tokens").unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::text_providers::{
        cli_common::fake::FakeCli, factory::test_fixture::run_engine, ModelRef,
    };

    fn request(model: &str, effort: Option<Effort>, timeout: Duration) -> TextRequest {
        TextRequest {
            model: ModelRef::new(ProviderKind::CodexCli, model, effort),
            instructions: "Fix grammar.\nReply with \"the text\" only.".into(),
            input: "SELECTED-TEXT-c0d3x teh text".into(),
            max_output_tokens: 1024,
            timeout,
        }
    }

    fn engine(fake: &FakeCli) -> CodexCliEngine {
        CodexCliEngine::new(Some(fake.path_string()))
    }

    const STARTED: &str = r#"{"type":"thread.started","thread_id":"t1"}"#;
    const TURN: &str = r#"{"type":"turn.started"}"#;
    const DONE: &str = r#"{"type":"turn.completed","usage":{"input_tokens":120,"cached_input_tokens":100,"output_tokens":9,"reasoning_output_tokens":4}}"#;

    fn message(text: &str) -> String {
        serde_json::json!({
            "type": "item.completed",
            "item": {"id": "item_1", "type": "agent_message", "text": text}
        })
        .to_string()
    }

    fn lines(lines: &[&str]) -> String {
        lines.join("\n") + "\n"
    }

    #[tokio::test]
    async fn returns_the_final_agent_message_once() {
        let reasoning = r#"{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"thinking"}}"#;
        let output = lines(&[STARTED, TURN, reasoning, &message("The ü text 👋"), DONE]);
        let fake = FakeCli::printing(&output, 0);
        let (result, pieces) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", Some(Effort::Low), Duration::from_secs(30)),
        )
        .await;
        let outcome = result.unwrap();
        assert_eq!(outcome.text, "The ü text 👋");
        assert_eq!(pieces, ["The ü text 👋"]);
        assert_eq!(outcome.finish, FinishReason::Completed);
        assert_eq!(
            outcome.usage,
            Some(TokenUsage {
                input_tokens: 120,
                output_tokens: 9,
                reasoning_tokens: 4
            })
        );
        assert!(outcome.first_token_ms.is_some());

        assert!(fake.recorded("stdin.txt").contains("SELECTED-TEXT-c0d3x"));
        let args = fake.recorded("args.txt");
        assert!(!args.contains("SELECTED-TEXT"), "{args}");
        for flag in [
            "exec",
            "--json",
            "--ephemeral",
            "--skip-git-repo-check",
            "--sandbox read-only",
            "--ignore-user-config",
            "--model gpt-6-luna",
            "--disable shell_tool",
            "model_reasoning_effort=",
            "developer_instructions=",
        ] {
            assert!(args.contains(flag), "{flag} missing in {args}");
        }
        assert!(args.trim_end().ends_with(" -"), "{args}");
        assert!(cli_common::fake::listing_is_empty(
            &fake.recorded("listing.txt")
        ));
    }

    #[test]
    fn instructions_become_one_toml_string_argument() {
        let args = codex_args("gpt-6.1-sol", None, "Line 1\nSay \"hi\" \\ ok\u{7f}").unwrap();
        let instructions = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .find(|arg| arg.starts_with("developer_instructions="))
            .unwrap();
        assert_eq!(
            instructions,
            r#"developer_instructions="Line 1\nSay \"hi\" \\ ok\u007F""#
        );
        assert!(!args
            .iter()
            .any(|arg| arg.to_string_lossy().contains("model_reasoning_effort")));
        assert_eq!(args.last().unwrap(), "-");
        assert_eq!(
            codex_args("gpt-6-luna", Some(Effort::None), "x").unwrap_err(),
            TextError::Unsupported
        );
    }

    #[tokio::test]
    async fn malformed_or_truncated_output_is_a_bad_response() {
        let fake = FakeCli::printing("garbage\n{\"type\":\"item.completed\",\"item\":{\n", 0);
        let (result, _) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", None, Duration::from_secs(30)),
        )
        .await;
        assert_eq!(result.unwrap_err(), TextError::BadResponse);
    }

    #[tokio::test]
    async fn failures_are_classified() {
        let failed = r#"{"type":"turn.failed","error":{"message":"You've hit your usage limit. private echoed input"}}"#;
        let fake = FakeCli::hanging(&lines(&[STARTED, failed]));
        let started = Instant::now();
        let (result, _) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", None, Duration::from_secs(30)),
        )
        .await;
        let err = result.unwrap_err();
        assert_eq!(err, TextError::RateLimited);
        crate::text_providers::factory::test_fixture::assert_safe(&err, &["SELECTED-TEXT"]);
        assert!(started.elapsed() < Duration::from_secs(10));

        let error = r#"{"type":"error","message":"unexpected status 401 Unauthorized"}"#;
        let fake = FakeCli::printing(&lines(&[STARTED, error]), 1);
        let (result, _) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", None, Duration::from_secs(30)),
        )
        .await;
        assert_eq!(result.unwrap_err(), TextError::CliNotSignedIn);

        // Not signed in, reported on stderr only.
        let fake = FakeCli::new("", "echo Not logged in. Run codex login. 1>&2\r\nexit /b 1");
        let (result, _) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", None, Duration::from_secs(30)),
        )
        .await;
        assert_eq!(result.unwrap_err(), TextError::CliNotSignedIn);

        // Other non-zero exits.
        let fake = FakeCli::new("", "echo error: unexpected argument 1>&2\r\nexit /b 2");
        let (result, _) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", None, Duration::from_secs(30)),
        )
        .await;
        assert_eq!(result.unwrap_err(), TextError::BadResponse);

        // A transient error followed by a completed turn is fine.
        let retry = r#"{"type":"error","message":"Reconnecting... 1/5"}"#;
        let fake = FakeCli::printing(&lines(&[retry, &message("ok"), DONE]), 0);
        let (result, _) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", None, Duration::from_secs(30)),
        )
        .await;
        assert_eq!(result.unwrap().text, "ok");
    }

    #[tokio::test]
    async fn hang_times_out_and_cancel_is_prompt() {
        let fake = FakeCli::hanging(&lines(&[STARTED, TURN]));
        let started = Instant::now();
        let (result, pieces) = run_engine(
            &engine(&fake),
            request("gpt-6-luna", None, Duration::from_millis(1500)),
        )
        .await;
        assert_eq!(result.unwrap_err(), TextError::Timeout);
        assert!(pieces.is_empty());
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(1400) && elapsed < Duration::from_secs(8),
            "{elapsed:?}"
        );

        // Codex sends no deltas before the end, so cancel 300 ms into the run.
        let cancel = CancelToken::new();
        let canceller = {
            let cancel = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let at = Instant::now();
                cancel.cancel();
                at
            })
        };
        let engine = engine(&fake);
        let result = engine
            .stream(
                request("gpt-6-luna", None, Duration::from_secs(60)),
                cancel,
                &|_| {},
            )
            .await;
        let returned = Instant::now();
        assert_eq!(result.unwrap_err(), TextError::Cancelled);
        let cancelled_at = canceller.await.unwrap();
        assert!(returned.duration_since(cancelled_at) < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn missing_cli_is_not_installed() {
        let missing = tempfile::tempdir().unwrap().path().join("codex.exe");
        let engine = CodexCliEngine::new(Some(missing.display().to_string()));
        let (result, _) =
            run_engine(&engine, request("gpt-6-luna", None, Duration::from_secs(5))).await;
        assert_eq!(result.unwrap_err(), TextError::CliNotInstalled);
    }
}
