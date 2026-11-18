//! Usage overview for the Dashboard: dictation and Text Assist side by side.
//!
//! One pass over the transcripts and one over the Text Assist runs. Every row updates the
//! accumulators of each period it falls in (today, last 7 days, last 30 days, all time,
//! in local time) and its day in the 30-day series. Word diffs run only for applied runs
//! and are cached by run id, because the overview is rebuilt after every new run.
//!
//! Time saved (conservative estimates, shown in the UI):
//! - Dictation: one minute per 40 dictated words (typing speed), as in `DashboardStats`.
//! - Text Assist: 6 seconds per corrected word in an applied run (spotting the mistake,
//!   moving the cursor and retyping). Runs that were not applied save nothing.
//!
//! The overview never contains original, result or transcript text.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

use chrono::{Local, NaiveDate};

use crate::{
    history::{
        TextAssistRun, OUTCOME_APPLIED, OUTCOME_CANCELLED, OUTCOME_FAILED, OUTCOME_KEPT,
        OUTCOME_NO_CHANGE,
    },
    pricing,
    text_assist::guard::{word_diff, DiffOp},
    text_providers::Billing,
    types::{
        DictationUsage, ReasonCount, TextAssistAppUsage, TextAssistInsights, TextAssistModelUsage,
        TextAssistOutcomeCounts, TextAssistPresetUsage, TextAssistUsage, TimingSummary,
        TranscriptHistoryItem, UsageAssumptions, UsageDay, UsageOverview, UsagePeriod,
        UsagePeriods,
    },
};

pub const DICTATION_WORDS_PER_MINUTE: u32 = 40;
pub const SECONDS_PER_CORRECTED_WORD: u32 = 6;
const SERIES_DAYS: i64 = 30;
const MAX_APPS: usize = 12;
const MAX_CACHED_DIFFS: usize = 50_000;

/// Changed words of an applied run, keyed by run id and text lengths.
type CorrectionCache = HashMap<(String, usize, usize), usize>;

fn correction_cache() -> &'static Mutex<CorrectionCache> {
    static CACHE: OnceLock<Mutex<CorrectionCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Words in `text`: runs of letters/digits (inner apostrophes kept), like the diff tokens.
fn count_words(text: &str) -> usize {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .filter(|part| part.chars().any(char::is_alphanumeric))
        .count()
}

/// Corrected words between `original` and `result`, from the Text Assist word diff.
///
/// Each change hunk counts the larger of its deleted and inserted word counts, so a
/// replaced word counts once. A hunk that only changes punctuation counts as one; a hunk
/// that only changes whitespace counts as zero. Line endings and outer whitespace are
/// ignored.
pub fn corrected_words(original: &str, result: &str) -> usize {
    let original = original.replace("\r\n", "\n");
    let result = result.replace("\r\n", "\n");
    let (original, result) = (original.trim(), result.trim());
    if original == result {
        return 0;
    }
    let mut total = 0;
    let mut hunk = (0usize, 0usize, false);
    let mut close = |hunk: &mut (usize, usize, bool)| {
        let words = hunk.0.max(hunk.1);
        total += if words == 0 && hunk.2 { 1 } else { words };
        *hunk = (0, 0, false);
    };
    for op in word_diff(original, result) {
        match op {
            DiffOp::Equal(_) => close(&mut hunk),
            DiffOp::Delete(text) => {
                hunk.0 += count_words(&text);
                hunk.2 |= text.chars().any(|c| !c.is_whitespace());
            }
            DiffOp::Insert(text) => {
                hunk.1 += count_words(&text);
                hunk.2 |= text.chars().any(|c| !c.is_whitespace());
            }
        }
    }
    close(&mut hunk);
    total
}

fn local_date(value: &str) -> Option<NaiveDate> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Local).date_naive())
}

fn is_plan(billing: &str) -> bool {
    billing == Billing::ChatgptPlan.as_str() || billing == Billing::ClaudePlan.as_str()
}

fn api_cost(run: &TextAssistRun) -> f64 {
    match run.estimated_cost_usd {
        Some(cost) if cost.is_finite() && cost > 0.0 && run.billing == Billing::Api.as_str() => {
            cost
        }
        _ => 0.0,
    }
}

fn returned_result(outcome: &str) -> bool {
    matches!(outcome, OUTCOME_APPLIED | OUTCOME_KEPT | OUTCOME_NO_CHANGE)
}

#[derive(Default)]
struct ModelAcc {
    runs: usize,
    applied: usize,
    failed: usize,
    duration_total: u64,
    duration_count: u64,
    cost: f64,
}

#[derive(Default)]
struct PeriodAcc {
    dictation: DictationUsage,
    text_assist: TextAssistUsage,
    outcomes: TextAssistOutcomeCounts,
    durations: Vec<u64>,
    first_tokens: Vec<u64>,
    models: BTreeMap<(String, String, Option<String>, String), ModelAcc>,
    apps: BTreeMap<Option<String>, TextAssistAppUsage>,
    presets: BTreeMap<(String, Option<String>), TextAssistPresetUsage>,
    ensembles: HashSet<String>,
    failure_reasons: BTreeMap<String, usize>,
    kept_reasons: BTreeMap<String, usize>,
}

impl PeriodAcc {
    fn add_transcript(&mut self, words: usize, cost: f64) {
        self.dictation.transcripts += 1;
        self.dictation.words += words;
        self.dictation.estimated_cost_usd += cost;
    }

    fn add_run(&mut self, run: &TextAssistRun, original_words: usize, corrected: usize) {
        let outcome = run.outcome.as_str();
        let applied = outcome == OUTCOME_APPLIED;
        let failed = outcome == OUTCOME_FAILED;
        let captured = !run.original_text.is_empty();
        let cost = api_cost(run);

        let usage = &mut self.text_assist;
        usage.runs += 1;
        usage.applied += usize::from(applied);
        usage.failed += usize::from(failed);
        usage.words_corrected += corrected;
        usage.estimated_cost_usd += cost;
        if captured && is_plan(&run.billing) {
            usage.plan_runs += 1;
        }
        match &run.ensemble_run_id {
            // Ensemble members share one selection: count its words once.
            Some(id) => {
                if self.ensembles.insert(id.clone()) {
                    usage.words_processed += original_words;
                }
            }
            None => usage.words_processed += original_words,
        }

        let counts = &mut self.outcomes;
        match outcome {
            OUTCOME_APPLIED => counts.applied += 1,
            OUTCOME_KEPT => counts.kept += 1,
            OUTCOME_NO_CHANGE => counts.no_change += 1,
            OUTCOME_CANCELLED => counts.cancelled += 1,
            OUTCOME_FAILED => counts.failed += 1,
            _ => counts.other += 1,
        }
        if returned_result(outcome) {
            self.durations.extend(run.duration_ms);
            self.first_tokens.extend(run.first_token_ms);
        }

        let reason = || run.reason_code.clone().unwrap_or_else(|| "unknown".into());
        if failed {
            *self.failure_reasons.entry(reason()).or_default() += 1;
        } else if outcome == OUTCOME_KEPT {
            *self.kept_reasons.entry(reason()).or_default() += 1;
        }

        // Attempts that never captured text did not reach a model.
        if captured {
            let key = (
                run.provider.clone(),
                run.model.clone(),
                run.effort.clone(),
                run.billing.clone(),
            );
            let model = self.models.entry(key).or_default();
            model.runs += 1;
            model.applied += usize::from(applied);
            model.failed += usize::from(failed);
            model.cost += cost;
            if let (Some(duration), true) = (run.duration_ms, returned_result(outcome)) {
                model.duration_total += duration;
                model.duration_count += 1;
            }
        }

        let app_key = run
            .target_app
            .as_deref()
            .map(str::trim)
            .filter(|app| !app.is_empty())
            .map(str::to_lowercase);
        let app = self
            .apps
            .entry(app_key.clone())
            .or_insert_with(|| TextAssistAppUsage {
                target_app: app_key,
                ..TextAssistAppUsage::default()
            });
        app.runs += 1;
        app.applied += usize::from(applied);
        app.failed += usize::from(failed);

        if captured {
            let key = (run.kind.clone(), run.preset_id.clone());
            let preset = self
                .presets
                .entry(key)
                .or_insert_with(|| TextAssistPresetUsage {
                    kind: run.kind.clone(),
                    preset_id: run.preset_id.clone(),
                    ..TextAssistPresetUsage::default()
                });
            preset.runs += 1;
            preset.applied += usize::from(applied);
        }
    }

    fn finish(self) -> UsagePeriod {
        let PeriodAcc {
            mut dictation,
            mut text_assist,
            outcomes,
            durations,
            first_tokens,
            models,
            apps,
            presets,
            ensembles,
            failure_reasons,
            kept_reasons,
        } = self;
        dictation.minutes_saved = dictation.words as f64 / f64::from(DICTATION_WORDS_PER_MINUTE);
        text_assist.minutes_saved =
            text_assist.words_corrected as f64 * f64::from(SECONDS_PER_CORRECTED_WORD) / 60.0;
        text_assist.ensemble_runs = ensembles.len();

        let returned_results = outcomes.applied + outcomes.kept + outcomes.no_change;
        let apply_rate =
            (returned_results > 0).then(|| outcomes.applied as f64 / returned_results as f64);

        let mut models = models
            .into_iter()
            .map(
                |((provider, model, effort, billing), acc)| TextAssistModelUsage {
                    provider,
                    model,
                    effort,
                    billing,
                    runs: acc.runs,
                    applied: acc.applied,
                    failed: acc.failed,
                    average_duration_ms: acc.duration_total.checked_div(acc.duration_count),
                    estimated_cost_usd: acc.cost,
                },
            )
            .collect::<Vec<_>>();
        models.sort_by_key(|model| std::cmp::Reverse(model.runs));

        let mut apps = apps.into_values().collect::<Vec<_>>();
        apps.sort_by_key(|app| std::cmp::Reverse(app.runs));
        apps.truncate(MAX_APPS);

        let mut presets = presets.into_values().collect::<Vec<_>>();
        presets.sort_by_key(|preset| std::cmp::Reverse(preset.runs));

        UsagePeriod {
            dictation,
            text_assist,
            insights: TextAssistInsights {
                outcomes,
                returned_results,
                apply_rate,
                duration: timing(durations),
                first_token: timing(first_tokens),
                models,
                apps,
                presets,
                failure_reasons: reasons(failure_reasons),
                kept_reasons: reasons(kept_reasons),
            },
        }
    }
}

fn timing(mut values: Vec<u64>) -> TimingSummary {
    if values.is_empty() {
        return TimingSummary::default();
    }
    values.sort_unstable();
    let count = values.len();
    let total = values.iter().map(|&value| u128::from(value)).sum::<u128>();
    let median = if count % 2 == 1 {
        values[count / 2]
    } else {
        // Average of the two middle values, rounded down, without overflow.
        let (low, high) = (values[count / 2 - 1], values[count / 2]);
        low + (high - low) / 2
    };
    TimingSummary {
        count,
        average_ms: Some((total / count as u128) as u64),
        median_ms: Some(median),
    }
}

fn reasons(map: BTreeMap<String, usize>) -> Vec<ReasonCount> {
    let mut list = map
        .into_iter()
        .map(|(code, count)| ReasonCount { code, count })
        .collect::<Vec<_>>();
    // Stable sort keeps codes with equal counts in alphabetical order.
    list.sort_by_key(|reason| std::cmp::Reverse(reason.count));
    list
}

/// Builds the overview in one pass over each source. `today` is the local date.
pub struct UsageAggregator<'c> {
    today: NaiveDate,
    periods: [PeriodAcc; 4],
    days: Vec<UsageDay>,
    cache: &'c mut CorrectionCache,
    estimated_from_words: usize,
    unknown_cleanup_model: usize,
}

impl<'c> UsageAggregator<'c> {
    pub fn new(today: NaiveDate, cache: &'c mut CorrectionCache) -> Self {
        let days = (0..SERIES_DAYS)
            .rev()
            .map(|offset| {
                let date = today - chrono::Duration::days(offset);
                UsageDay {
                    date: date.to_string(),
                    day_label: date.format("%a").to_string(),
                    ..UsageDay::default()
                }
            })
            .collect();
        Self {
            today,
            periods: Default::default(),
            days,
            cache,
            estimated_from_words: 0,
            unknown_cleanup_model: 0,
        }
    }

    /// Periods (by index into `periods`) and the 30-day series slot of a timestamp.
    fn placement(&self, created_at: &str) -> ([bool; 4], Option<usize>) {
        let Some(date) = local_date(created_at) else {
            return ([false, false, false, true], None);
        };
        let days_ago = (self.today - date).num_days();
        let in_range = |days: i64| (0..days).contains(&days_ago);
        let slot = in_range(SERIES_DAYS).then(|| (SERIES_DAYS - 1 - days_ago) as usize);
        ([in_range(1), in_range(7), in_range(30), true], slot)
    }

    pub fn add_transcript(&mut self, item: &TranscriptHistoryItem) {
        // The existing estimator prices one item at a time when given a one-item slice.
        let summary = pricing::calculate_cost_summary(std::slice::from_ref(item), self.today);
        self.estimated_from_words += summary.estimated_from_words_count;
        self.unknown_cleanup_model += summary.unknown_cleanup_model_count;
        let cost = summary.estimated_total_usd;
        let (periods, slot) = self.placement(&item.created_at);
        for (acc, _) in self.periods.iter_mut().zip(periods).filter(|(_, hit)| *hit) {
            acc.add_transcript(item.word_count, cost);
        }
        if let Some(day) = slot.map(|slot| &mut self.days[slot]) {
            day.transcripts += 1;
            day.dictated_words += item.word_count;
            day.dictation_cost_usd += cost;
        }
    }

    pub fn add_run(&mut self, run: &TextAssistRun) {
        let original_words = count_words(&run.original_text);
        let corrected = match (&run.result_text, run.outcome == OUTCOME_APPLIED) {
            (Some(result), true) => {
                let key = (run.id.clone(), run.original_text.len(), result.len());
                match self.cache.get(&key) {
                    Some(&words) => words,
                    None => {
                        let words = corrected_words(&run.original_text, result);
                        if self.cache.len() >= MAX_CACHED_DIFFS {
                            self.cache.clear();
                        }
                        self.cache.insert(key, words);
                        words
                    }
                }
            }
            _ => 0,
        };
        let (periods, slot) = self.placement(&run.created_at);
        for (acc, _) in self.periods.iter_mut().zip(periods).filter(|(_, hit)| *hit) {
            acc.add_run(run, original_words, corrected);
        }
        if let Some(day) = slot.map(|slot| &mut self.days[slot]) {
            day.text_assist_runs += 1;
            day.text_assist_applied += usize::from(run.outcome == OUTCOME_APPLIED);
            day.words_corrected += corrected;
            day.text_assist_cost_usd += api_cost(run);
        }
    }

    pub fn finish(self) -> UsageOverview {
        let [today, last7_days, last30_days, all_time] = self.periods.map(PeriodAcc::finish);
        UsageOverview {
            today: self.today.to_string(),
            periods: UsagePeriods {
                today,
                last7_days,
                last30_days,
                all_time,
            },
            daily: self.days,
            assumptions: UsageAssumptions {
                dictation_words_per_minute: DICTATION_WORDS_PER_MINUTE,
                seconds_per_corrected_word: SECONDS_PER_CORRECTED_WORD,
            },
            dictation_cost_note: pricing::calculate_cost_summary(&[], self.today).note,
            dictation_estimated_from_words_count: self.estimated_from_words,
            dictation_unknown_cleanup_model_count: self.unknown_cleanup_model,
        }
    }
}

/// Runs `build` with the process-wide diff cache.
pub fn with_cache<T>(build: impl FnOnce(&mut CorrectionCache) -> T) -> T {
    let mut cache = correction_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    build(&mut cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        history::KIND_POLISH,
        types::{CleanupMode, Provider, SttModel},
    };

    fn at(date: NaiveDate) -> String {
        // Noon local time keeps the date stable in every time zone.
        date.and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .to_rfc3339()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 18).unwrap()
    }

    fn days_ago(days: i64) -> String {
        at(today() - chrono::Duration::days(days))
    }

    fn transcript(id: &str, created_at: &str, words: usize) -> TranscriptHistoryItem {
        TranscriptHistoryItem {
            id: id.to_string(),
            created_at: created_at.to_string(),
            provider: Provider::Openai,
            stt_model: SttModel::Gpt4oMiniTranscribe,
            cleanup_model: None,
            cleanup_mode: CleanupMode::Off,
            raw_text: "secret dictation".to_string(),
            final_text: "secret dictation".to_string(),
            duration_ms: Some(100),
            word_count: words,
            char_count: 1,
            pasted: true,
            error: None,
            pipeline_started_at: None,
            recording_duration_ms: Some(60_000),
            transcription_duration_ms: Some(100),
            cleanup_duration_ms: None,
            paste_duration_ms: None,
            total_pipeline_duration_ms: None,
        }
    }

    fn run(id: &str, created_at: &str) -> TextAssistRun {
        TextAssistRun {
            id: id.to_string(),
            created_at: created_at.to_string(),
            kind: KIND_POLISH.to_string(),
            preset_id: Some("polish".to_string()),
            ensemble_run_id: None,
            provider: "openai".to_string(),
            model: "gpt-6-luna".to_string(),
            effort: Some("none".to_string()),
            billing: Billing::Api.as_str().to_string(),
            original_text: "teh quick brwn fox".to_string(),
            result_text: Some("the quick brown fox".to_string()),
            outcome: OUTCOME_APPLIED.to_string(),
            reason_code: None,
            target_app: Some("Notepad.exe".to_string()),
            duration_ms: Some(800),
            first_token_ms: Some(300),
            input_tokens: Some(400),
            output_tokens: Some(10),
            estimated_cost_usd: Some(0.01),
        }
    }

    /// A failed attempt that never captured text.
    fn failed_attempt(id: &str, created_at: &str, reason: &str) -> TextAssistRun {
        TextAssistRun {
            original_text: String::new(),
            result_text: None,
            outcome: OUTCOME_FAILED.to_string(),
            reason_code: Some(reason.to_string()),
            target_app: None,
            duration_ms: None,
            first_token_ms: None,
            input_tokens: None,
            output_tokens: None,
            estimated_cost_usd: None,
            ..run(id, created_at)
        }
    }

    fn overview(transcripts: &[TranscriptHistoryItem], runs: &[TextAssistRun]) -> UsageOverview {
        let mut cache = CorrectionCache::new();
        let mut agg = UsageAggregator::new(today(), &mut cache);
        transcripts.iter().for_each(|item| agg.add_transcript(item));
        runs.iter().for_each(|item| agg.add_run(item));
        agg.finish()
    }

    #[test]
    fn corrected_words_counts_changed_words_once() {
        assert_eq!(
            corrected_words("teh quick brwn fox", "the quick brown fox"),
            2
        );
        assert_eq!(corrected_words("same text", "same text"), 0);
        // Line endings and outer whitespace are not corrections.
        assert_eq!(corrected_words("a\r\nb ", "a\nb"), 0);
        // A punctuation-only fix counts as one; whitespace-only counts as zero.
        assert_eq!(corrected_words("Hello world", "Hello, world"), 1);
        assert_eq!(corrected_words("Hello  world", "Hello world"), 0);
        // Inserted and deleted words: the larger side of each hunk.
        assert_eq!(corrected_words("I go home", "I will go home now"), 2);
        assert_eq!(corrected_words("", "new words here"), 3);
    }

    #[test]
    fn empty_history_has_zero_periods_and_a_full_series() {
        let result = overview(&[], &[]);
        assert_eq!(result.today, "2026-11-18");
        assert_eq!(result.periods.all_time, UsagePeriod::default());
        assert_eq!(result.daily.len(), 30);
        assert_eq!(result.daily[0].date, "2026-10-20");
        assert_eq!(result.daily[29].date, "2026-11-18");
        assert_eq!(result.daily[29].day_label, "Wed");
        assert_eq!(result.periods.today.insights.apply_rate, None);
        assert_eq!(result.assumptions.seconds_per_corrected_word, 6);
        assert!(result
            .dictation_cost_note
            .contains("Not official billing data"));
    }

    #[test]
    fn rows_land_in_each_period_they_belong_to() {
        let transcripts = [
            transcript("t0", &days_ago(0), 40),
            transcript("t6", &days_ago(6), 80),
            transcript("t7", &days_ago(7), 120),
            transcript("t29", &days_ago(29), 4),
            transcript("t30", &days_ago(30), 400),
            transcript("bad", "not a date", 1),
        ];
        let runs = [
            run("r0", &days_ago(0)),
            run("r6", &days_ago(6)),
            run("r40", &days_ago(40)),
        ];
        let result = overview(&transcripts, &runs);
        let p = &result.periods;
        assert_eq!(p.today.dictation.transcripts, 1);
        assert_eq!(p.today.dictation.words, 40);
        assert!((p.today.dictation.minutes_saved - 1.0).abs() < 1e-9);
        assert_eq!(p.last7_days.dictation.transcripts, 2);
        assert_eq!(p.last30_days.dictation.transcripts, 4);
        assert_eq!(p.all_time.dictation.transcripts, 6);
        assert_eq!(p.all_time.dictation.words, 645);
        // 60 s of gpt-4o-mini-transcribe audio at $0.003 per minute.
        assert!((p.today.dictation.estimated_cost_usd - 0.003).abs() < 1e-9);
        assert_eq!(p.today.text_assist.runs, 1);
        assert_eq!(p.last7_days.text_assist.runs, 2);
        assert_eq!(p.last30_days.text_assist.runs, 2);
        assert_eq!(p.all_time.text_assist.runs, 3);

        assert_eq!(result.daily[29].transcripts, 1);
        assert_eq!(result.daily[23].transcripts, 1);
        assert_eq!(result.daily[22].transcripts, 1);
        assert_eq!(result.daily[0].dictated_words, 4);
        assert_eq!(result.daily[29].text_assist_runs, 1);
        assert_eq!(result.daily[29].words_corrected, 2);
        assert!((result.daily[29].text_assist_cost_usd - 0.01).abs() < 1e-12);
        assert_eq!(
            result
                .daily
                .iter()
                .map(|d| d.text_assist_runs)
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn text_assist_totals_count_failed_attempts_and_corrections() {
        let mut kept = run("kept", &days_ago(0));
        kept.outcome = OUTCOME_KEPT.to_string();
        kept.reason_code = Some("guard_preamble".to_string());
        let mut unchanged = run("same", &days_ago(0));
        unchanged.outcome = OUTCOME_NO_CHANGE.to_string();
        unchanged.result_text = Some(unchanged.original_text.clone());
        let mut cancelled = run("cancel", &days_ago(0));
        cancelled.outcome = OUTCOME_CANCELLED.to_string();
        let mut plan = run("plan", &days_ago(0));
        plan.provider = "codex_cli".to_string();
        plan.billing = Billing::ChatgptPlan.as_str().to_string();
        plan.estimated_cost_usd = Some(5.0);
        let mut api_failure = run("rate", &days_ago(0));
        api_failure.outcome = OUTCOME_FAILED.to_string();
        api_failure.result_text = None;
        api_failure.reason_code = Some("rate_limited".to_string());
        let mut plan_attempt = failed_attempt("np", &days_ago(0), "no_selection");
        plan_attempt.billing = Billing::ClaudePlan.as_str().to_string();
        let runs = [
            run("a1", &days_ago(0)),
            plan,
            kept,
            unchanged,
            cancelled,
            api_failure,
            failed_attempt("f1", &days_ago(0), "no_selection"),
            failed_attempt("f2", &days_ago(0), "clipboard_busy"),
            plan_attempt,
        ];
        let result = overview(&[], &runs);
        let today = &result.periods.today;
        let usage = &today.text_assist;
        assert_eq!(usage.runs, 9);
        assert_eq!(usage.applied, 2);
        assert_eq!(usage.failed, 4);
        assert_eq!(
            usage.plan_runs, 1,
            "attempts without text are not plan runs"
        );
        // 4 words in each of the six runs that captured text.
        assert_eq!(usage.words_processed, 24);
        // Only the two applied runs count their two corrected words.
        assert_eq!(usage.words_corrected, 4);
        assert!((usage.minutes_saved - 0.4).abs() < 1e-9);
        // API-billed runs only: the plan run's estimate and attempts without a cost add nothing.
        assert!((usage.estimated_cost_usd - 0.05).abs() < 1e-9);

        let insights = &today.insights;
        assert_eq!(
            insights.outcomes,
            TextAssistOutcomeCounts {
                applied: 2,
                kept: 1,
                no_change: 1,
                cancelled: 1,
                failed: 4,
                other: 0
            }
        );
        assert_eq!(insights.returned_results, 4);
        assert_eq!(insights.apply_rate, Some(0.5));
        assert_eq!(
            insights.failure_reasons,
            vec![
                ReasonCount {
                    code: "no_selection".into(),
                    count: 2
                },
                ReasonCount {
                    code: "clipboard_busy".into(),
                    count: 1
                },
                ReasonCount {
                    code: "rate_limited".into(),
                    count: 1
                },
            ]
        );
        assert_eq!(
            insights.kept_reasons,
            vec![ReasonCount {
                code: "guard_preamble".into(),
                count: 1
            }]
        );
    }

    #[test]
    fn breakdowns_by_model_app_preset_and_ensemble() {
        let mut slow = run("slow", &days_ago(1));
        slow.duration_ms = Some(2_000);
        slow.first_token_ms = Some(900);
        slow.target_app = Some("notepad.exe".to_string());
        let mut claude = run("claude", &days_ago(1));
        claude.provider = "anthropic".to_string();
        claude.model = "claude-haiku".to_string();
        claude.effort = None;
        claude.outcome = OUTCOME_KEPT.to_string();
        claude.duration_ms = Some(1_000);
        claude.first_token_ms = Some(500);
        claude.target_app = Some("Code.exe".to_string());
        claude.kind = "rewrite".to_string();
        claude.preset_id = Some("formal".to_string());
        claude.estimated_cost_usd = Some(0.02);
        let mut member_a = run("ea", &days_ago(1));
        member_a.kind = "ensemble".to_string();
        member_a.ensemble_run_id = Some("e1".to_string());
        member_a.duration_ms = Some(600);
        member_a.first_token_ms = Some(200);
        let mut member_b = member_a.clone();
        member_b.id = "eb".to_string();
        member_b.outcome = OUTCOME_KEPT.to_string();
        member_b.duration_ms = Some(400);
        member_b.first_token_ms = Some(100);
        let mut other_ensemble = member_b.clone();
        other_ensemble.id = "ec".to_string();
        other_ensemble.ensemble_run_id = Some("e2".to_string());
        other_ensemble.duration_ms = None;
        other_ensemble.first_token_ms = None;
        let runs = [
            run("a", &days_ago(1)),
            slow,
            claude,
            member_a,
            member_b,
            other_ensemble,
            failed_attempt("f", &days_ago(1), "no_selection"),
        ];
        let result = overview(&[], &runs);
        let week = &result.periods.last7_days;
        assert_eq!(result.periods.today.text_assist.runs, 0);
        assert_eq!(week.text_assist.ensemble_runs, 2);
        // Two ensembles count their shared selection once: 4 + 4 + 4 + 4 + 4 words.
        assert_eq!(week.text_assist.words_processed, 20);

        let insights = &week.insights;
        // Measured durations of the runs that returned results: 400 600 800 1000 2000.
        assert_eq!(insights.duration.count, 5);
        assert_eq!(insights.duration.median_ms, Some(800));
        assert_eq!(insights.duration.average_ms, Some(960));
        assert_eq!(insights.first_token.median_ms, Some(300));

        let luna = &insights.models[0];
        assert_eq!(
            (
                luna.provider.as_str(),
                luna.model.as_str(),
                luna.effort.as_deref()
            ),
            ("openai", "gpt-6-luna", Some("none"))
        );
        // The failed attempt without text is not attributed to a model.
        assert_eq!((luna.runs, luna.applied, luna.failed), (5, 3, 0));
        assert_eq!(luna.average_duration_ms, Some(950));
        assert!((luna.estimated_cost_usd - 0.05).abs() < 1e-9);
        let haiku = &insights.models[1];
        assert_eq!(
            (haiku.model.as_str(), haiku.runs, haiku.applied),
            ("claude-haiku", 1, 0)
        );
        assert_eq!(haiku.effort, None);
        assert!((haiku.estimated_cost_usd - 0.02).abs() < 1e-9);

        // App names are grouped case-insensitively; unknown apps are their own row.
        assert_eq!(
            insights.apps,
            vec![
                TextAssistAppUsage {
                    target_app: Some("notepad.exe".into()),
                    runs: 5,
                    applied: 3,
                    failed: 0
                },
                TextAssistAppUsage {
                    target_app: None,
                    runs: 1,
                    applied: 0,
                    failed: 1
                },
                TextAssistAppUsage {
                    target_app: Some("code.exe".into()),
                    runs: 1,
                    applied: 0,
                    failed: 0
                },
            ]
        );
        assert_eq!(
            insights.presets,
            vec![
                TextAssistPresetUsage {
                    kind: "ensemble".into(),
                    preset_id: Some("polish".into()),
                    runs: 3,
                    applied: 1
                },
                TextAssistPresetUsage {
                    kind: "polish".into(),
                    preset_id: Some("polish".into()),
                    runs: 2,
                    applied: 2
                },
                TextAssistPresetUsage {
                    kind: "rewrite".into(),
                    preset_id: Some("formal".into()),
                    runs: 1,
                    applied: 0
                },
            ]
        );
    }

    #[test]
    fn overview_json_is_camel_case_and_has_no_text() {
        let result = overview(
            &[transcript("t", &days_ago(0), 3)],
            &[
                run("r", &days_ago(0)),
                failed_attempt("f", &days_ago(0), "no_selection"),
            ],
        );
        let json = serde_json::to_string(&result).unwrap();
        for secret in ["teh", "brwn", "brown", "secret dictation"] {
            assert!(!json.contains(secret), "overview leaks text: {secret}");
        }
        let value = serde_json::to_value(&result).unwrap();
        let today = &value["periods"]["today"];
        assert_eq!(today["textAssist"]["wordsCorrected"], 2);
        assert_eq!(today["insights"]["applyRate"], 1.0);
        assert_eq!(
            today["insights"]["failureReasons"][0]["code"],
            "no_selection"
        );
        assert!(value["periods"]["last7Days"].is_object());
        assert!(value["periods"]["last30Days"].is_object());
        assert!(value["periods"]["allTime"].is_object());
        assert_eq!(value["assumptions"]["dictationWordsPerMinute"], 40);
        assert_eq!(value["daily"][29]["wordsCorrected"], 2);
    }

    #[test]
    fn corrections_are_cached_by_run() {
        let mut cache = CorrectionCache::new();
        let applied = run("cached", &days_ago(0));
        {
            let mut agg = UsageAggregator::new(today(), &mut cache);
            agg.add_run(&applied);
            agg.finish();
        }
        assert_eq!(cache.len(), 1);
        // A stale cached value wins over recomputing, proving the diff is skipped.
        *cache.values_mut().next().unwrap() = 7;
        let mut agg = UsageAggregator::new(today(), &mut cache);
        agg.add_run(&applied);
        let mut not_applied = run("kept", &days_ago(0));
        not_applied.outcome = OUTCOME_KEPT.to_string();
        agg.add_run(&not_applied);
        assert_eq!(agg.finish().periods.today.text_assist.words_corrected, 7);
        assert_eq!(cache.len(), 1, "no diff for runs that were not applied");
    }

    #[test]
    fn timing_median_handles_even_counts() {
        assert_eq!(timing(vec![]), TimingSummary::default());
        let summary = timing(vec![300, 100, 200, 1_000]);
        assert_eq!(summary.count, 4);
        assert_eq!(summary.median_ms, Some(250));
        assert_eq!(summary.average_ms, Some(400));
    }
}
