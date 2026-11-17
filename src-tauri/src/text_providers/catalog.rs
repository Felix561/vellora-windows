//! Curated Text Assist models: allowed efforts, prices, colors and lifecycle.
//!
//! OpenAI facts were checked on the model pages on 2026-11-17:
//! - <https://developers.openai.com/api/docs/models/gpt-6-luna>
//! - <https://developers.openai.com/api/docs/models/gpt-5.6-luna>
//! - <https://developers.openai.com/api/docs/models/gpt-6-astra>
//! - <https://developers.openai.com/api/docs/models/gpt-5.6-terra>
//!
//! Anthropic facts were checked on 2026-11-17:
//! - Prices: <https://platform.claude.com/docs/en/about-claude/pricing>
//! - Efforts, defaults and thinking-off rules:
//!   <https://platform.claude.com/docs/en/build-with-claude/effort>
//!
//! Gemini facts were checked on 2026-11-17:
//! - Prices: <https://aistudio.google.com/docs/pricing.md> (paid tier, standard, text)
//! - Thinking levels and defaults: <https://aistudio.google.com/docs/thinking.md>
//! - Model pages: <https://aistudio.google.com/docs/models/gemini-3.1-flash-lite.md>,
//!   `.../gemini-3.6-flash.md`, `.../gemini-3.8-flash.md` (all stable)
//!
//! CLI bridge facts (M6) were checked on 2026-11-17 on this machine without a model request:
//! - Codex: `codex debug models --bundled` (the catalog shipped with codex-cli 0.169.0) lists
//!   `gpt-6-luna` (low..max, default medium), `gpt-6.1-sol` and `gpt-6-astra` (low..max plus
//!   `ultra`, default low). `ultra` has no Vellora effort and is not offered. The models a
//!   ChatGPT account may use depend on its plan.
//! - Claude Code 2.1.325: `--model` takes the aliases `haiku`, `sonnet`, `opus`; `--effort`
//!   takes low..max ("available levels depend on the model"). No default is sent, so the CLI
//!   applies its own.
//! - CLI runs are billed to the plan (`ProviderKind::billing`), so their price is 0.
//!
//! Prices are standard text rates in USD per 1M tokens. Cached-input discounts are not
//! modeled, so estimates are an upper bound. No page listed a shutdown date.

use chrono::NaiveDate;

use super::{offline::OFFLINE_MODEL_ID, Effort, ModelRef, ProviderKind, TextError, TokenUsage};

/// How an Anthropic model turns thinking off for [`Effort::None`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingOff {
    /// `thinking: {type: "disabled"}`.
    Disabled,
    /// `thinking: {type: "between_tools"}` (Claude Sonnet 5.5 only).
    BetweenTools,
}

/// A price change already announced by the provider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceChange {
    /// First day (`YYYY-MM-DD`, UTC) the new prices apply.
    pub from: &'static str,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogModel {
    pub provider: ProviderKind,
    pub id: &'static str,
    pub label: &'static str,
    /// Efforts the provider accepts for this model, in ascending order. Empty: the model has
    /// no effort parameter and none may be sent.
    pub efforts: &'static [Effort],
    /// Provider default as documented; sent when the run requests no effort. `None` means the
    /// parameter is omitted and the provider applies its own default.
    pub default_effort: Option<Effort>,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
    /// Default halo/history color, `#rrggbb`.
    pub color: &'static str,
    /// Provider shutdown date (`YYYY-MM-DD`), when announced.
    pub shutdown: Option<&'static str>,
    /// Offered for Quick Polish (fast and cheap).
    pub quick_candidate: bool,
    /// Anthropic only: the thinking setting sent for [`Effort::None`]. `None` when the model
    /// cannot turn thinking off (and so does not list `none`).
    pub thinking_off: Option<ThinkingOff>,
    /// Announced change to the prices above, if any.
    pub price_change: Option<PriceChange>,
}

/// `none`, `low`, `medium`, `high`, `xhigh`, `max` (`minimal` is not listed).
const NONE_TO_MAX: &[Effort] = &[
    Effort::None,
    Effort::Low,
    Effort::Medium,
    Effort::High,
    Effort::Xhigh,
    Effort::Max,
];

/// `low`, `medium`, `high`, `xhigh`, `max` (`none` is not accepted).
const LOW_TO_MAX: &[Effort] = &[
    Effort::Low,
    Effort::Medium,
    Effort::High,
    Effort::Xhigh,
    Effort::Max,
];

/// Codex CLI `model_reasoning_effort` low..max (as listed by the bundled Codex catalog).
const CODEX_LOW_TO_MAX: &[Effort] = LOW_TO_MAX;

/// Claude Code CLI `--effort` values.
const CLAUDE_CLI_EFFORTS: &[Effort] = LOW_TO_MAX;

/// Gemini `thinkingLevel` `minimal`, `low`, `medium`, `high`.
const MINIMAL_TO_HIGH: &[Effort] = &[Effort::Minimal, Effort::Low, Effort::Medium, Effort::High];

/// Gemini `thinkingLevel` `low`, `medium`, `high` (`minimal` returns an error).
const LOW_TO_HIGH: &[Effort] = &[Effort::Low, Effort::Medium, Effort::High];

/// Gemini 3.6 / 3.8 Flash list $0.75 / $3.75 through 2026-12-31, then $1.50 / $7.50.
const GEMINI_FLASH_2027: PriceChange = PriceChange {
    from: "2027-01-01",
    input_usd_per_million: 1.50,
    output_usd_per_million: 7.50,
};

static MODELS: &[CatalogModel] = &[
    // Checked 2026-11-17: efforts none..max, default medium; $0.10 in / $0.50 out
    // (cached input $0.01).
    CatalogModel {
        provider: ProviderKind::Openai,
        id: "gpt-6-luna",
        label: "GPT-6 Luna",
        efforts: NONE_TO_MAX,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 0.10,
        output_usd_per_million: 0.50,
        color: "#10a37f",
        shutdown: None,
        quick_candidate: true,
        thinking_off: None,
        price_change: None,
    },
    // Checked 2026-11-17: efforts none..max, default medium; $0.20 in / $1.20 out
    // (cached input $0.02).
    CatalogModel {
        provider: ProviderKind::Openai,
        id: "gpt-5.6-luna",
        label: "GPT-5.6 Luna",
        efforts: NONE_TO_MAX,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 0.20,
        output_usd_per_million: 1.20,
        color: "#19c39c",
        shutdown: None,
        quick_candidate: true,
        thinking_off: None,
        price_change: None,
    },
    // Checked 2026-11-17: efforts low..max (no `none`); the page states no default, so
    // none is sent unless the run asks for one. $10 in / $50 out (cached input $1).
    CatalogModel {
        provider: ProviderKind::Openai,
        id: "gpt-6-astra",
        label: "GPT-6 Astra",
        efforts: LOW_TO_MAX,
        default_effort: None,
        input_usd_per_million: 10.0,
        output_usd_per_million: 50.0,
        color: "#0b7a5f",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    // Checked 2026-11-17: efforts none..max, default medium; $2 in / $12 out (cached input
    // $0.20). Prompts above 272K input tokens cost 2x input / 1.5x output; Text Assist
    // inputs are far below that.
    CatalogModel {
        provider: ProviderKind::Openai,
        id: "gpt-5.6-terra",
        label: "GPT-5.6 Terra",
        efforts: NONE_TO_MAX,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 2.0,
        output_usd_per_million: 12.0,
        color: "#5fb89a",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    // Checked 2026-11-17: efforts low..max, default medium; `thinking: disabled` accepted at
    // high or below (sent for `none`, with effort `low`). $0.10 in / $0.50 out for prompts up
    // to 100K tokens ($0.50 / $2.50 above; Text Assist prompts are far below that).
    CatalogModel {
        provider: ProviderKind::Anthropic,
        id: "claude-haiku-5-5",
        label: "Claude Haiku 5.5",
        efforts: NONE_TO_MAX,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 0.10,
        output_usd_per_million: 0.50,
        color: "#e9a07f",
        shutdown: None,
        quick_candidate: true,
        thinking_off: Some(ThinkingOff::Disabled),
        price_change: None,
    },
    // Checked 2026-11-17: efforts low..max, default high; `disabled` is a 400, thinking off is
    // `thinking: between_tools` at high or below (sent for `none`, with effort `low`).
    // $2 in / $10 out.
    CatalogModel {
        provider: ProviderKind::Anthropic,
        id: "claude-sonnet-5-5",
        label: "Claude Sonnet 5.5",
        efforts: NONE_TO_MAX,
        default_effort: Some(Effort::High),
        input_usd_per_million: 2.0,
        output_usd_per_million: 10.0,
        color: "#d97757",
        shutdown: None,
        quick_candidate: false,
        thinking_off: Some(ThinkingOff::BetweenTools),
        price_change: None,
    },
    // Checked 2026-11-17: efforts low..max, default medium; thinking cannot be turned off
    // (`disabled` is a 400 at every effort), so `none` is not offered. $4 in / $20 out.
    CatalogModel {
        provider: ProviderKind::Anthropic,
        id: "claude-opus-5-5",
        label: "Claude Opus 5.5",
        efforts: LOW_TO_MAX,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 4.0,
        output_usd_per_million: 20.0,
        color: "#b5563a",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    // Checked 2026-11-17: thinkingLevel minimal..high, default minimal (thinking cannot be
    // fully turned off). $0.25 in / $1.50 out (output includes thinking tokens).
    CatalogModel {
        provider: ProviderKind::Gemini,
        id: "gemini-3.1-flash-lite",
        label: "Gemini 3.1 Flash-Lite",
        efforts: MINIMAL_TO_HIGH,
        default_effort: Some(Effort::Minimal),
        input_usd_per_million: 0.25,
        output_usd_per_million: 1.50,
        color: "#7baaf7",
        shutdown: None,
        quick_candidate: true,
        thinking_off: None,
        price_change: None,
    },
    // Checked 2026-11-17: thinkingLevel minimal..high, default medium. $0.75 in / $3.75 out
    // through 2026-12-31, then $1.50 / $7.50.
    CatalogModel {
        provider: ProviderKind::Gemini,
        id: "gemini-3.6-flash",
        label: "Gemini 3.6 Flash",
        efforts: MINIMAL_TO_HIGH,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 0.75,
        output_usd_per_million: 3.75,
        color: "#4285f4",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: Some(GEMINI_FLASH_2027),
    },
    // Checked 2026-11-17: thinkingLevel low..high (no `minimal`), default medium. Same prices
    // and price change as 3.6 Flash.
    CatalogModel {
        provider: ProviderKind::Gemini,
        id: "gemini-3.8-flash",
        label: "Gemini 3.8 Flash",
        efforts: LOW_TO_HIGH,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 0.75,
        output_usd_per_million: 3.75,
        color: "#2b67d1",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: Some(GEMINI_FLASH_2027),
    },
    // ChatGPT plan through the user's Codex CLI (M6). Plan billing: no marginal price.
    CatalogModel {
        provider: ProviderKind::CodexCli,
        id: "gpt-6-luna",
        label: "GPT-6 Luna (ChatGPT plan)",
        efforts: CODEX_LOW_TO_MAX,
        default_effort: Some(Effort::Medium),
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
        color: "#3cb98f",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    CatalogModel {
        provider: ProviderKind::CodexCli,
        id: "gpt-6.1-sol",
        label: "GPT-6.1 Sol (ChatGPT plan)",
        efforts: CODEX_LOW_TO_MAX,
        default_effort: Some(Effort::Low),
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
        color: "#0e8f6e",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    CatalogModel {
        provider: ProviderKind::CodexCli,
        id: "gpt-6-astra",
        label: "GPT-6 Astra (ChatGPT plan)",
        efforts: CODEX_LOW_TO_MAX,
        default_effort: Some(Effort::Low),
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
        color: "#086b52",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    // Claude plan through the user's Claude Code CLI (M6). Aliases follow the newest model.
    CatalogModel {
        provider: ProviderKind::ClaudeCli,
        id: "haiku",
        label: "Claude Haiku (Claude plan)",
        efforts: CLAUDE_CLI_EFFORTS,
        default_effort: None,
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
        color: "#f0b597",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    CatalogModel {
        provider: ProviderKind::ClaudeCli,
        id: "sonnet",
        label: "Claude Sonnet (Claude plan)",
        efforts: CLAUDE_CLI_EFFORTS,
        default_effort: None,
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
        color: "#e08a68",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    CatalogModel {
        provider: ProviderKind::ClaudeCli,
        id: "opus",
        label: "Claude Opus (Claude plan)",
        efforts: CLAUDE_CLI_EFFORTS,
        default_effort: None,
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
        color: "#a34d33",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
    // Developer-only offline transform (M2). No network request, no cost.
    CatalogModel {
        provider: ProviderKind::Offline,
        id: OFFLINE_MODEL_ID,
        label: "Offline uppercase (dev)",
        efforts: &[],
        default_effort: None,
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
        color: "#8a8f98",
        shutdown: None,
        quick_candidate: false,
        thinking_off: None,
        price_change: None,
    },
];

pub fn models() -> &'static [CatalogModel] {
    MODELS
}

pub fn find(provider: ProviderKind, id: &str) -> Option<&'static CatalogModel> {
    MODELS
        .iter()
        .find(|model| model.provider == provider && model.id == id)
}

/// Effort to send: the ModelRef's effort if allowed, else the default. Catalog models with
/// a disallowed effort -> Err(TextError::Unsupported). Unknown model IDs: Ok(None) when no
/// effort was requested, Err(Unsupported) when one was.
pub fn resolve_effort(model: &ModelRef) -> Result<Option<Effort>, TextError> {
    match (find(model.provider, &model.model), model.effort) {
        (Some(entry), Some(effort)) if entry.efforts.contains(&effort) => Ok(Some(effort)),
        (Some(_), Some(_)) => Err(TextError::Unsupported),
        (Some(entry), None) => Ok(entry.default_effort),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(TextError::Unsupported),
    }
}

/// Provider parameter(s) sent for `effort` on a catalog model, for display and diagnostics,
/// e.g. `reasoning.effort=none` or `output_config.effort=low, thinking.type=disabled`.
/// `None` when the model does not accept `effort`.
pub fn effort_parameter(entry: &CatalogModel, effort: Effort) -> Option<String> {
    if !entry.efforts.contains(&effort) {
        return None;
    }
    Some(match entry.provider {
        ProviderKind::Openai => format!("reasoning.effort={}", effort.as_str()),
        ProviderKind::Anthropic => match (effort, entry.thinking_off) {
            (Effort::None, Some(ThinkingOff::Disabled)) => {
                "output_config.effort=low, thinking.type=disabled".to_string()
            }
            (Effort::None, Some(ThinkingOff::BetweenTools)) => {
                "output_config.effort=low, thinking.type=between_tools".to_string()
            }
            (Effort::None, None) => return None,
            (effort, _) => format!("output_config.effort={}", effort.as_str()),
        },
        ProviderKind::Gemini => format!("thinkingConfig.thinkingLevel={}", effort.as_str()),
        ProviderKind::CodexCli => format!("model_reasoning_effort={}", effort.as_str()),
        ProviderKind::ClaudeCli => format!("--effort {}", effort.as_str()),
        _ => return None,
    })
}

/// Input and output price per 1M tokens on `date` (UTC), including announced changes.
pub fn prices_on(entry: &CatalogModel, date: NaiveDate) -> (f64, f64) {
    match entry.price_change {
        Some(change)
            if NaiveDate::parse_from_str(change.from, "%Y-%m-%d")
                .is_ok_and(|from| date >= from) =>
        {
            (change.input_usd_per_million, change.output_usd_per_million)
        }
        _ => (entry.input_usd_per_million, entry.output_usd_per_million),
    }
}

/// Current input and output price per 1M tokens.
pub fn current_prices(entry: &CatalogModel) -> (f64, f64) {
    prices_on(entry, chrono::Utc::now().date_naive())
}

/// Estimated cost from provider-reported usage and the catalog price that applies today.
/// `None` for models without a catalog price. Reasoning tokens are already part of
/// `output_tokens`.
pub fn estimate_cost_usd(model: &ModelRef, usage: &TokenUsage) -> Option<f64> {
    estimate_cost_usd_on(model, usage, chrono::Utc::now().date_naive())
}

/// [`estimate_cost_usd`] at the prices of `date` (UTC).
pub fn estimate_cost_usd_on(model: &ModelRef, usage: &TokenUsage, date: NaiveDate) -> Option<f64> {
    let entry = find(model.provider, &model.model)?;
    let (input, output) = prices_on(entry, date);
    Some(
        usage.input_tokens as f64 * input / 1_000_000.0
            + usage.output_tokens as f64 * output / 1_000_000.0,
    )
}

/// Color for models without a catalog entry.
pub fn default_color(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Openai => "#10a37f",
        ProviderKind::Anthropic | ProviderKind::ClaudeCli => "#d97757",
        ProviderKind::Gemini => "#4285f4",
        ProviderKind::OpenaiCompatible | ProviderKind::CodexCli | ProviderKind::Offline => {
            "#8a8f98"
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn openai(model: &str, effort: Option<Effort>) -> ModelRef {
        ModelRef::new(ProviderKind::Openai, model, effort)
    }

    #[test]
    fn catalog_entries_are_unique_and_well_formed() {
        let mut keys = HashSet::new();
        for model in models() {
            assert!(keys.insert((model.provider, model.id)), "{}", model.id);
            assert!(model.color.len() == 7 && model.color.starts_with('#'));
            assert!(model.color[1..].bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(model.input_usd_per_million >= 0.0 && model.output_usd_per_million >= 0.0);
            assert!(model.efforts.windows(2).all(|pair| pair[0] < pair[1]));
            if let Some(default) = model.default_effort {
                assert!(model.efforts.contains(&default), "{}", model.id);
            }
        }
    }

    #[test]
    fn luna_models_are_quick_candidates_with_checked_prices() {
        let luna = find(ProviderKind::Openai, "gpt-6-luna").unwrap();
        assert!(luna.quick_candidate);
        assert_eq!(
            (luna.input_usd_per_million, luna.output_usd_per_million),
            (0.10, 0.50)
        );
        assert!(luna.efforts.contains(&Effort::None));
        assert!(!luna.efforts.contains(&Effort::Minimal));
        let older = find(ProviderKind::Openai, "gpt-5.6-luna").unwrap();
        assert!(older.quick_candidate);
        assert_eq!(
            (older.input_usd_per_million, older.output_usd_per_million),
            (0.20, 1.20)
        );
        assert!(
            !find(ProviderKind::Openai, "gpt-6-astra")
                .unwrap()
                .quick_candidate
        );
        assert!(find(ProviderKind::Anthropic, "gpt-6-luna").is_none());
    }

    #[test]
    fn resolve_effort_allows_listed_values_only() {
        assert_eq!(
            resolve_effort(&openai("gpt-6-luna", Some(Effort::None))),
            Ok(Some(Effort::None))
        );
        assert_eq!(
            resolve_effort(&openai("gpt-6-luna", Some(Effort::Minimal))),
            Err(TextError::Unsupported)
        );
        assert_eq!(
            resolve_effort(&openai("gpt-6-astra", Some(Effort::None))),
            Err(TextError::Unsupported)
        );
        assert_eq!(
            resolve_effort(&openai("gpt-6-astra", Some(Effort::Low))),
            Ok(Some(Effort::Low))
        );
    }

    #[test]
    fn resolve_effort_falls_back_to_the_default() {
        assert_eq!(
            resolve_effort(&openai("gpt-6-luna", None)),
            Ok(Some(Effort::Medium))
        );
        assert_eq!(resolve_effort(&openai("gpt-6-astra", None)), Ok(None));
        let offline = ModelRef::new(ProviderKind::Offline, OFFLINE_MODEL_ID, None);
        assert_eq!(resolve_effort(&offline), Ok(None));
        let offline = ModelRef::new(ProviderKind::Offline, OFFLINE_MODEL_ID, Some(Effort::Low));
        assert_eq!(resolve_effort(&offline), Err(TextError::Unsupported));
    }

    #[test]
    fn resolve_effort_for_unknown_models() {
        assert_eq!(resolve_effort(&openai("my-custom-model", None)), Ok(None));
        assert_eq!(
            resolve_effort(&openai("my-custom-model", Some(Effort::Low))),
            Err(TextError::Unsupported)
        );
    }

    #[test]
    fn estimates_cost_from_usage() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 2_000_000,
            reasoning_tokens: 500_000,
        };
        let cost = estimate_cost_usd(&openai("gpt-6-luna", None), &usage).unwrap();
        assert!((cost - 1.10).abs() < 1e-9, "{cost}");
        let offline = ModelRef::new(ProviderKind::Offline, OFFLINE_MODEL_ID, None);
        assert_eq!(estimate_cost_usd(&offline, &usage), Some(0.0));
        assert_eq!(
            estimate_cost_usd(&openai("my-custom-model", None), &usage),
            None
        );
    }

    #[test]
    fn anthropic_entries_have_checked_efforts_and_prices() {
        let haiku = find(ProviderKind::Anthropic, "claude-haiku-5-5").unwrap();
        assert!(haiku.quick_candidate);
        assert_eq!(
            (haiku.input_usd_per_million, haiku.output_usd_per_million),
            (0.10, 0.50)
        );
        assert_eq!(haiku.default_effort, Some(Effort::Medium));
        assert_eq!(haiku.thinking_off, Some(ThinkingOff::Disabled));
        let sonnet = find(ProviderKind::Anthropic, "claude-sonnet-5-5").unwrap();
        assert_eq!(sonnet.default_effort, Some(Effort::High));
        assert_eq!(sonnet.thinking_off, Some(ThinkingOff::BetweenTools));
        let opus = find(ProviderKind::Anthropic, "claude-opus-5-5").unwrap();
        assert_eq!(opus.default_effort, Some(Effort::Medium));
        assert_eq!(opus.thinking_off, None);
        assert!(!opus.efforts.contains(&Effort::None));
        assert_eq!(
            (opus.input_usd_per_million, opus.output_usd_per_million),
            (4.0, 20.0)
        );
        for entry in models() {
            // `none` is offered exactly when the model can turn thinking off.
            if entry.provider == ProviderKind::Anthropic {
                assert_eq!(
                    entry.efforts.contains(&Effort::None),
                    entry.thinking_off.is_some(),
                    "{}",
                    entry.id
                );
                assert!(!entry.efforts.contains(&Effort::Minimal));
            } else {
                assert_eq!(entry.thinking_off, None, "{}", entry.id);
            }
        }
    }

    #[test]
    fn gemini_entries_have_checked_levels_and_prices() {
        let lite = find(ProviderKind::Gemini, "gemini-3.1-flash-lite").unwrap();
        assert!(lite.quick_candidate);
        assert_eq!(lite.default_effort, Some(Effort::Minimal));
        assert_eq!(
            (lite.input_usd_per_million, lite.output_usd_per_million),
            (0.25, 1.50)
        );
        let flash38 = find(ProviderKind::Gemini, "gemini-3.8-flash").unwrap();
        assert!(!flash38.efforts.contains(&Effort::Minimal));
        assert_eq!(
            resolve_effort(&ModelRef::new(
                ProviderKind::Gemini,
                "gemini-3.8-flash",
                Some(Effort::Minimal)
            )),
            Err(TextError::Unsupported)
        );
        for entry in models()
            .iter()
            .filter(|m| m.provider == ProviderKind::Gemini)
        {
            for effort in entry.efforts {
                assert!(matches!(
                    effort,
                    Effort::Minimal | Effort::Low | Effort::Medium | Effort::High
                ));
            }
        }
    }

    #[test]
    fn scheduled_price_changes_apply_from_their_date() {
        let flash = find(ProviderKind::Gemini, "gemini-3.6-flash").unwrap();
        let date = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(prices_on(flash, date("2026-12-31")), (0.75, 3.75));
        assert_eq!(prices_on(flash, date("2027-01-01")), (1.50, 7.50));
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            reasoning_tokens: 0,
        };
        let model = ModelRef::new(ProviderKind::Gemini, "gemini-3.6-flash", None);
        let before = estimate_cost_usd_on(&model, &usage, date("2026-11-17")).unwrap();
        let after = estimate_cost_usd_on(&model, &usage, date("2027-02-01")).unwrap();
        assert!((before - 4.5).abs() < 1e-9, "{before}");
        assert!((after - 9.0).abs() < 1e-9, "{after}");
        let luna = find(ProviderKind::Openai, "gpt-6-luna").unwrap();
        assert_eq!(prices_on(luna, date("2030-01-01")), (0.10, 0.50));
        for entry in models() {
            if let Some(change) = entry.price_change {
                assert!(NaiveDate::parse_from_str(change.from, "%Y-%m-%d").is_ok());
            }
        }
    }

    #[test]
    fn effort_parameters_describe_the_provider_field() {
        let get = |provider, id| find(provider, id).unwrap();
        let param = |provider, id, effort| effort_parameter(get(provider, id), effort);
        assert_eq!(
            param(ProviderKind::Openai, "gpt-6-luna", Effort::None).as_deref(),
            Some("reasoning.effort=none")
        );
        assert_eq!(
            param(ProviderKind::Anthropic, "claude-haiku-5-5", Effort::None).as_deref(),
            Some("output_config.effort=low, thinking.type=disabled")
        );
        assert_eq!(
            param(ProviderKind::Anthropic, "claude-sonnet-5-5", Effort::None).as_deref(),
            Some("output_config.effort=low, thinking.type=between_tools")
        );
        assert_eq!(
            param(ProviderKind::Anthropic, "claude-opus-5-5", Effort::Xhigh).as_deref(),
            Some("output_config.effort=xhigh")
        );
        assert_eq!(
            param(ProviderKind::Anthropic, "claude-opus-5-5", Effort::None),
            None
        );
        assert_eq!(
            param(
                ProviderKind::Gemini,
                "gemini-3.1-flash-lite",
                Effort::Minimal
            )
            .as_deref(),
            Some("thinkingConfig.thinkingLevel=minimal")
        );
        assert_eq!(
            param(ProviderKind::Gemini, "gemini-3.8-flash", Effort::Minimal),
            None
        );
    }

    #[test]
    fn cli_bridge_entries_are_plan_billed() {
        use crate::text_providers::Billing;
        for (provider, ids) in [
            (
                ProviderKind::CodexCli,
                &["gpt-6-luna", "gpt-6.1-sol", "gpt-6-astra"][..],
            ),
            (ProviderKind::ClaudeCli, &["haiku", "sonnet", "opus"][..]),
        ] {
            for id in ids {
                let entry = find(provider, id).unwrap();
                assert_eq!(
                    (entry.input_usd_per_million, entry.output_usd_per_million),
                    (0.0, 0.0)
                );
                assert!(!entry.quick_candidate);
                assert!(!entry.efforts.contains(&Effort::None));
                assert!(!entry.efforts.contains(&Effort::Minimal));
            }
            assert_ne!(provider.billing(), Billing::Api);
        }
        let usage = TokenUsage {
            input_tokens: 1_000,
            output_tokens: 1_000,
            reasoning_tokens: 0,
        };
        let haiku = ModelRef::new(ProviderKind::ClaudeCli, "haiku", None);
        assert_eq!(estimate_cost_usd(&haiku, &usage), Some(0.0));
        assert_eq!(resolve_effort(&haiku), Ok(None));
        let luna = ModelRef::new(ProviderKind::CodexCli, "gpt-6-luna", None);
        assert_eq!(resolve_effort(&luna), Ok(Some(Effort::Medium)));
        assert_eq!(
            effort_parameter(
                find(ProviderKind::CodexCli, "gpt-6-luna").unwrap(),
                Effort::Low
            )
            .as_deref(),
            Some("model_reasoning_effort=low")
        );
        assert_eq!(
            effort_parameter(find(ProviderKind::ClaudeCli, "opus").unwrap(), Effort::Max)
                .as_deref(),
            Some("--effort max")
        );
    }

    #[test]
    fn default_colors_per_provider() {
        assert_eq!(default_color(ProviderKind::Openai), "#10a37f");
        assert_eq!(default_color(ProviderKind::Anthropic), "#d97757");
        assert_eq!(default_color(ProviderKind::ClaudeCli), "#d97757");
        assert_eq!(default_color(ProviderKind::Gemini), "#4285f4");
        assert_eq!(default_color(ProviderKind::CodexCli), "#8a8f98");
        assert_eq!(default_color(ProviderKind::OpenaiCompatible), "#8a8f98");
    }
}
