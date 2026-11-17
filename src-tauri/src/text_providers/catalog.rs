//! Curated Text Assist models: allowed efforts, prices, colors and lifecycle.
//!
//! OpenAI facts were checked on the model pages on 2026-11-17:
//! - <https://developers.openai.com/api/docs/models/gpt-6-luna>
//! - <https://developers.openai.com/api/docs/models/gpt-5.6-luna>
//! - <https://developers.openai.com/api/docs/models/gpt-6-astra>
//! - <https://developers.openai.com/api/docs/models/gpt-5.6-terra>
//!
//! Prices are standard text rates in USD per 1M tokens. Cached-input discounts are not
//! modeled, so estimates are an upper bound. No page listed a shutdown date.
//! Anthropic and Gemini entries are added in M5.

use super::{offline::OFFLINE_MODEL_ID, Effort, ModelRef, ProviderKind, TextError, TokenUsage};

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

/// Estimated cost from provider-reported usage and the catalog price. `None` for models
/// without a catalog price. Reasoning tokens are already part of `output_tokens`.
pub fn estimate_cost_usd(model: &ModelRef, usage: &TokenUsage) -> Option<f64> {
    let entry = find(model.provider, &model.model)?;
    Some(
        usage.input_tokens as f64 * entry.input_usd_per_million / 1_000_000.0
            + usage.output_tokens as f64 * entry.output_usd_per_million / 1_000_000.0,
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
    fn default_colors_per_provider() {
        assert_eq!(default_color(ProviderKind::Openai), "#10a37f");
        assert_eq!(default_color(ProviderKind::Anthropic), "#d97757");
        assert_eq!(default_color(ProviderKind::ClaudeCli), "#d97757");
        assert_eq!(default_color(ProviderKind::Gemini), "#4285f4");
        assert_eq!(default_color(ProviderKind::CodexCli), "#8a8f98");
        assert_eq!(default_color(ProviderKind::OpenaiCompatible), "#8a8f98");
    }
}
