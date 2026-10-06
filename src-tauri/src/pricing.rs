use std::collections::BTreeMap;

use chrono::{Datelike, Duration, Local, NaiveDate};

use crate::types::{
    CleanupMode, CostSummary, DailyCostStat, ModelCostStat, SttModel, TranscriptHistoryItem,
};

pub const PRICING_VERSION: &str = "openai-pricing-2026-05-29";
const WORDS_PER_AUDIO_MINUTE: f64 = 150.0;
const MIN_RELIABLE_RECORDING_MS: u64 = 1_000;

pub fn calculate_cost_summary(items: &[TranscriptHistoryItem], today: NaiveDate) -> CostSummary {
    let mut week_map = BTreeMap::new();
    for offset in (0..7).rev() {
        let date = today - Duration::days(offset);
        week_map.insert(date, (0.0_f64, 0usize, 0u64));
    }

    let mut model_map: BTreeMap<String, (f64, usize, u64)> = BTreeMap::new();
    let mut estimated_total_usd = 0.0;
    let mut estimated_today_usd = 0.0;
    let mut estimated_week_usd = 0.0;
    let mut estimated_transcription_usd = 0.0;
    let mut estimated_cleanup_usd = 0.0;
    let mut billable_audio_seconds = 0u64;
    let mut measured_audio_seconds = 0u64;
    let mut estimated_audio_seconds = 0u64;
    let mut estimated_from_words_count = 0usize;
    let mut unknown_model_count = 0usize;

    for item in items {
        let created_date = chrono::DateTime::parse_from_rfc3339(&item.created_at)
            .ok()
            .map(|date| date.with_timezone(&Local).date_naive());

        let duration = billable_audio_duration(item);
        if duration.estimated_from_words {
            estimated_from_words_count += 1;
            estimated_audio_seconds += duration.audio_seconds;
        } else {
            measured_audio_seconds += duration.audio_seconds;
        }
        billable_audio_seconds += duration.audio_seconds;

        let Some(price_per_minute) = transcription_price_per_minute(&item.stt_model) else {
            unknown_model_count += 1;
            continue;
        };

        let transcription_cost = (duration.audio_seconds as f64 / 60.0) * price_per_minute;
        let cleanup_cost = cleanup_cost(item);
        let total_cost = transcription_cost + cleanup_cost;

        estimated_total_usd += total_cost;
        estimated_transcription_usd += transcription_cost;
        estimated_cleanup_usd += cleanup_cost;

        if created_date == Some(today) {
            estimated_today_usd += total_cost;
        }

        if let Some(date) = created_date {
            if let Some(day) = week_map.get_mut(&date) {
                day.0 += total_cost;
                day.1 += 1;
                day.2 += duration.audio_seconds;
                estimated_week_usd += total_cost;
            }
        }

        let entry = model_map
            .entry(item.stt_model.as_str().to_string())
            .or_insert((0.0, 0, 0));
        entry.0 += total_cost;
        entry.1 += 1;
        entry.2 += duration.audio_seconds;
    }

    let week = week_map
        .into_iter()
        .map(
            |(date, (estimated_cost_usd, transcription_count, audio_seconds))| DailyCostStat {
                date: date.to_string(),
                day_label: date.weekday().to_string()[0..3].to_string(),
                estimated_cost_usd,
                transcription_count,
                audio_seconds,
            },
        )
        .collect();

    let model_breakdown = model_map
        .into_iter()
        .map(
            |(model, (estimated_cost_usd, transcription_count, audio_seconds))| ModelCostStat {
                model,
                estimated_cost_usd,
                transcription_count,
                audio_seconds,
            },
        )
        .collect();

    CostSummary {
        pricing_version: PRICING_VERSION.to_string(),
        estimated_total_usd,
        estimated_today_usd,
        estimated_week_usd,
        estimated_transcription_usd,
        estimated_cleanup_usd,
        billable_audio_seconds,
        measured_audio_seconds,
        estimated_audio_seconds,
        estimated_from_words_count,
        unknown_model_count,
        week,
        model_breakdown,
        note: "Estimated from OpenAI public pricing and local Vellora history. Not official billing data.".to_string(),
    }
}

fn transcription_price_per_minute(model: &SttModel) -> Option<f64> {
    match model {
        SttModel::Whisper1 => Some(0.006),
        SttModel::Gpt4oMiniTranscribe => Some(0.003),
        SttModel::Gpt4oTranscribe => Some(0.006),
        SttModel::Gpt4oTranscribeDiarize => Some(0.006),
    }
}

struct AudioDurationEstimate {
    audio_seconds: u64,
    estimated_from_words: bool,
}

fn billable_audio_duration(item: &TranscriptHistoryItem) -> AudioDurationEstimate {
    if let Some(ms) = item.recording_duration_ms {
        if ms >= MIN_RELIABLE_RECORDING_MS {
            return AudioDurationEstimate {
                audio_seconds: ((ms as f64) / 1000.0).ceil() as u64,
                estimated_from_words: false,
            };
        }
    }

    AudioDurationEstimate {
        audio_seconds: ((item.word_count as f64 / WORDS_PER_AUDIO_MINUTE) * 60.0).ceil() as u64,
        estimated_from_words: true,
    }
}

fn cleanup_cost(item: &TranscriptHistoryItem) -> f64 {
    if item.cleanup_mode != CleanupMode::Light {
        return 0.0;
    }

    let Some(model) = item.cleanup_model.as_deref() else {
        return 0.0;
    };

    let (input_per_million, output_per_million) = cleanup_prices(model);
    let raw_words = item.raw_text.split_whitespace().count();
    let final_words = item.final_text.split_whitespace().count();
    let input_tokens = ((raw_words as f64) * 1.33).ceil() + 120.0;
    let output_tokens = ((final_words as f64) * 1.33).ceil();
    (input_tokens / 1_000_000.0 * input_per_million)
        + (output_tokens / 1_000_000.0 * output_per_million)
}

fn cleanup_prices(model: &str) -> (f64, f64) {
    match model {
        "gpt-5-nano-2025-08-07" | "gpt-5-nano" => (0.05, 0.40),
        _ => (0.05, 0.40),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CleanupMode, Provider};

    fn item(
        model: SttModel,
        recording_duration_ms: Option<u64>,
        words: usize,
    ) -> TranscriptHistoryItem {
        TranscriptHistoryItem {
            id: "1".to_string(),
            created_at: "2026-05-29T12:00:00Z".to_string(),
            provider: Provider::Openai,
            stt_model: model,
            cleanup_model: None,
            cleanup_mode: CleanupMode::Off,
            raw_text: "hello world".to_string(),
            final_text: "hello world".to_string(),
            duration_ms: None,
            word_count: words,
            char_count: 11,
            pasted: true,
            error: None,
            pipeline_started_at: None,
            recording_duration_ms,
            transcription_duration_ms: None,
            cleanup_duration_ms: None,
            paste_duration_ms: None,
            total_pipeline_duration_ms: None,
        }
    }

    #[test]
    fn whisper_ten_minutes_costs_six_cents() {
        let stats = calculate_cost_summary(
            &[item(SttModel::Whisper1, Some(600_000), 100)],
            NaiveDate::from_ymd_opt(2026, 5, 29).unwrap(),
        );
        assert!((stats.estimated_total_usd - 0.06).abs() < 0.0001);
    }

    #[test]
    fn mini_transcribe_ten_minutes_costs_three_cents() {
        let stats = calculate_cost_summary(
            &[item(SttModel::Gpt4oMiniTranscribe, Some(600_000), 100)],
            NaiveDate::from_ymd_opt(2026, 5, 29).unwrap(),
        );
        assert!((stats.estimated_total_usd - 0.03).abs() < 0.0001);
    }

    #[test]
    fn old_rows_estimate_audio_from_words() {
        let stats = calculate_cost_summary(
            &[item(SttModel::Whisper1, None, 150)],
            NaiveDate::from_ymd_opt(2026, 5, 29).unwrap(),
        );
        assert_eq!(stats.estimated_from_words_count, 1);
        assert_eq!(stats.estimated_audio_seconds, 60);
    }
}
