//! Provider-neutral text generation for Text Assist.
//!
//! Every provider adapter implements [`TextEngine`]. Requests carry the selected text as a
//! separate input, never merged into the instructions. Errors never contain prompts, selected
//! text, response bodies or keys.

// Wired into the Text Assist job runner in M2/M3; until then some items are unused.
#![allow(dead_code)]

pub mod anthropic;
pub mod catalog;
pub mod cli_claude;
pub mod cli_codex;
pub mod cli_common;
pub mod factory;
pub mod gemini;
pub mod http;
pub mod offline;
pub mod openai;
pub mod openai_compat;

use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

/// Vellora's provider-neutral reasoning effort. The catalog lists which values each model
/// accepts and how each maps to the provider's own parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::None => "none",
            Effort::Minimal => "minimal",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

/// Where a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI Responses API with the user's API key.
    Openai,
    /// Anthropic Messages API with the user's API key (M5).
    Anthropic,
    /// Google Gemini API with an AI Studio key (M5).
    Gemini,
    /// A user-defined Chat Completions-compatible endpoint (M5).
    OpenaiCompatible,
    /// The user's own signed-in Codex CLI, billed to the ChatGPT plan (M6).
    CodexCli,
    /// The user's own signed-in Claude Code CLI, billed to the Claude plan (M6).
    ClaudeCli,
    /// Developer-only offline transform. Never makes a network request.
    Offline,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Openai => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::OpenaiCompatible => "openai_compatible",
            ProviderKind::CodexCli => "codex_cli",
            ProviderKind::ClaudeCli => "claude_cli",
            ProviderKind::Offline => "offline",
        }
    }

    pub fn billing(self) -> Billing {
        match self {
            ProviderKind::Openai | ProviderKind::Anthropic | ProviderKind::Gemini => Billing::Api,
            // Compatible endpoints may be paid APIs or local servers; the catalog/settings
            // decide. Default to API so costs are never silently hidden.
            ProviderKind::OpenaiCompatible => Billing::Api,
            ProviderKind::CodexCli => Billing::ChatgptPlan,
            ProviderKind::ClaudeCli => Billing::ClaudePlan,
            ProviderKind::Offline => Billing::Local,
        }
    }
}

/// How a run is paid for. Stored in history as snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Billing {
    Api,
    ChatgptPlan,
    ClaudePlan,
    Local,
}

impl Billing {
    pub fn as_str(self) -> &'static str {
        match self {
            Billing::Api => "api",
            Billing::ChatgptPlan => "chatgpt_plan",
            Billing::ClaudePlan => "claude_plan",
            Billing::Local => "local",
        }
    }
}

/// A provider, model and effort chosen for one run. Saved in settings (camelCase JSON).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub provider: ProviderKind,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// For [`ProviderKind::OpenaiCompatible`]: the endpoint id from settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl ModelRef {
    pub fn new(provider: ProviderKind, model: &str, effort: Option<Effort>) -> Self {
        Self {
            provider,
            model: model.to_string(),
            effort,
            endpoint: None,
        }
    }

    /// Stable key used for per-model colors, e.g. `openai/gpt-6-luna`.
    pub fn color_key(&self) -> String {
        format!("{}/{}", self.provider.as_str(), self.model)
    }
}

/// One generation request.
#[derive(Debug, Clone)]
pub struct TextRequest {
    pub model: ModelRef,
    /// System/developer instructions.
    pub instructions: String,
    /// The selected text, sent as a separate user input. Untrusted data.
    pub input: String,
    pub max_output_tokens: u32,
    /// Total budget for the request including one transient retry.
    pub timeout: Duration,
}

impl TextRequest {
    /// Output cap sent to the provider: the answer budget plus room for reasoning tokens.
    /// OpenAI, Anthropic and Gemini count reasoning/thinking tokens against the same limit, so
    /// a cap sized for the answer alone truncates or empties results at higher efforts.
    /// `effort` is the resolved effort; `None` means the provider default, which may reason.
    pub fn output_cap(&self, effort: Option<Effort>) -> u32 {
        let reasoning: u32 = match effort {
            Some(Effort::None) => 0,
            Some(Effort::Minimal) => 1_024,
            Some(Effort::Low) => 4_096,
            None | Some(Effort::Medium) => 8_192,
            Some(Effort::High) => 16_384,
            Some(Effort::Xhigh) | Some(Effort::Max) => 32_000,
        };
        self.max_output_tokens
            .saturating_add(reasoning)
            .min(MAX_OUTPUT_CAP)
    }
}

/// Upper bound for [`TextRequest::output_cap`]; within every catalog model's output limit.
pub const MAX_OUTPUT_CAP: u32 = 64_000;

/// Token usage reported by the provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Reasoning tokens, when reported. Already included in `output_tokens` for OpenAI.
    pub reasoning_tokens: u64,
}

/// Why generation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Normal completion.
    Completed,
    /// Output token limit reached; the text is truncated.
    MaxTokens,
    /// The model refused.
    Refused,
    /// A provider content filter stopped generation.
    ContentFilter,
    /// Anything else the provider reported as not a normal completion.
    Other,
}

/// Result of a finished request.
#[derive(Debug, Clone, PartialEq)]
pub struct TextOutcome {
    /// Full generated text, exactly as received (no trimming).
    pub text: String,
    pub finish: FinishReason,
    pub usage: Option<TokenUsage>,
    /// Milliseconds from sending the request to the first text delta.
    pub first_token_ms: Option<u64>,
}

/// Provider errors. Messages are safe to show and to log: they never contain prompts,
/// selected text, response bodies or keys.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    #[error("No API key is saved for this provider")]
    MissingKey,
    #[error("The provider rejected the API key")]
    InvalidKey,
    #[error("The provider rate limit or quota was reached")]
    RateLimited,
    #[error("The network is unavailable")]
    Network,
    #[error("The request timed out")]
    Timeout,
    #[error("The provider returned a server error ({0})")]
    Server(u16),
    #[error("The provider rejected the request ({0})")]
    Rejected(u16),
    #[error("The provider returned an unexpected response")]
    BadResponse,
    #[error("The model refused the request")]
    ProviderRefused,
    #[error("This model or effort is not supported")]
    Unsupported,
    #[error("The CLI is not installed")]
    CliNotInstalled,
    #[error("The CLI is not signed in")]
    CliNotSignedIn,
    #[error("Cancelled")]
    Cancelled,
}

impl TextError {
    /// Sanitized code stored in history (`reason_code`).
    pub fn reason_code(&self) -> &'static str {
        match self {
            TextError::MissingKey => "missing_key",
            TextError::InvalidKey => "invalid_key",
            TextError::RateLimited => "rate_limited",
            TextError::Network => "network",
            TextError::Timeout => "timeout",
            TextError::Server(_) => "server_error",
            TextError::Rejected(_) => "rejected",
            TextError::BadResponse => "bad_response",
            TextError::ProviderRefused => "provider_refused",
            TextError::Unsupported => "unsupported",
            TextError::CliNotInstalled => "cli_not_installed",
            TextError::CliNotSignedIn => "cli_not_signed_in",
            TextError::Cancelled => "cancelled",
        }
    }
}

/// Cooperative cancellation shared between a job and its request.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<CancelInner>);

#[derive(Debug, Default)]
struct CancelInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Completes once [`CancelToken::cancel`] was called (immediately if it already was).
    pub async fn cancelled(&self) {
        loop {
            let notified = self.0.notify.notified();
            tokio::pin!(notified);
            // Register before checking the flag so a concurrent cancel cannot be missed.
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// Callback for streamed text. Called with each new text fragment, in order.
pub type DeltaSink<'a> = &'a (dyn Fn(&str) + Send + Sync);

/// Boxed future returned by [`TextEngine::stream`], so engines can be used as trait objects.
pub type TextFuture<'a> = Pin<Box<dyn Future<Output = Result<TextOutcome, TextError>> + Send + 'a>>;

/// A text generation backend.
///
/// Implementations must:
/// - send `request.input` as separate user input and `request.instructions` as instructions;
/// - call `on_delta` for each streamed text fragment;
/// - stop promptly and return [`TextError::Cancelled`] when `cancel` fires;
/// - respect `request.timeout` as the total budget and return [`TextError::Timeout`];
/// - never enable tools.
pub trait TextEngine: Send + Sync {
    fn stream<'a>(
        &'a self,
        request: TextRequest,
        cancel: CancelToken,
        on_delta: DeltaSink<'a>,
    ) -> TextFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_cap_adds_room_for_reasoning() {
        let request = |max| TextRequest {
            model: ModelRef::new(ProviderKind::Openai, "m", None),
            instructions: String::new(),
            input: String::new(),
            max_output_tokens: max,
            timeout: std::time::Duration::from_secs(1),
        };
        // Thinking off: the answer budget only.
        assert_eq!(request(290).output_cap(Some(Effort::None)), 290);
        assert_eq!(request(290).output_cap(Some(Effort::Low)), 290 + 4_096);
        // No effort means the provider default, which may reason.
        assert_eq!(request(290).output_cap(None), 290 + 8_192);
        assert_eq!(request(290).output_cap(Some(Effort::High)), 290 + 16_384);
        assert_eq!(
            request(32_000).output_cap(Some(Effort::Max)),
            MAX_OUTPUT_CAP
        );
    }

    #[test]
    fn model_ref_serializes_as_camel_case_and_omits_empty_fields() {
        let model = ModelRef::new(ProviderKind::Openai, "gpt-6-luna", Some(Effort::None));
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "provider": "openai", "model": "gpt-6-luna", "effort": "none" })
        );
        let back: ModelRef = serde_json::from_value(json).unwrap();
        assert_eq!(back, model);
        assert_eq!(model.color_key(), "openai/gpt-6-luna");
    }

    #[test]
    fn provider_billing_labels() {
        assert_eq!(ProviderKind::CodexCli.billing(), Billing::ChatgptPlan);
        assert_eq!(ProviderKind::ClaudeCli.billing(), Billing::ClaudePlan);
        assert_eq!(ProviderKind::Offline.billing(), Billing::Local);
        assert_eq!(Billing::ChatgptPlan.as_str(), "chatgpt_plan");
    }

    #[tokio::test]
    async fn cancel_token_wakes_waiters_and_is_sticky() {
        let token = CancelToken::new();
        let waiter = {
            let token = token.clone();
            tokio::spawn(async move { token.cancelled().await })
        };
        tokio::task::yield_now().await;
        token.cancel();
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();
        // Already cancelled: returns immediately.
        tokio::time::timeout(Duration::from_millis(100), token.cancelled())
            .await
            .unwrap();
    }
}
