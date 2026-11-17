use std::collections::BTreeMap;

use anyhow::Context;
use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    brand, pricing, settings,
    text_providers::Billing,
    types::{
        CleanupMode, DailyStat, DashboardStats, ModelUsageStat, Provider, SttModel,
        TranscriptHistoryItem,
    },
};

fn db_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(settings::app_data_dir()?.join(brand::DATABASE_NAME))
}

fn connection() -> anyhow::Result<Connection> {
    let conn = Connection::open(db_path()?).context("Could not open local history database")?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Creates or upgrades every history table. Idempotent.
fn init_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS transcripts (
          id TEXT PRIMARY KEY,
          created_at TEXT NOT NULL,
          provider TEXT NOT NULL,
          stt_model TEXT NOT NULL,
          cleanup_model TEXT,
          cleanup_mode TEXT NOT NULL,
          raw_text TEXT NOT NULL,
          final_text TEXT NOT NULL,
          duration_ms INTEGER,
          word_count INTEGER NOT NULL,
          char_count INTEGER NOT NULL,
          pasted INTEGER NOT NULL,
          error TEXT
        );
        "#,
    )
    .context("Could not initialize history database")?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    conn.pragma_update(None, "secure_delete", true)?;
    migrate(conn)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS transcripts_created_at ON transcripts(created_at)",
        [],
    )?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS text_assist_runs (
          id TEXT PRIMARY KEY,
          created_at TEXT NOT NULL,
          kind TEXT NOT NULL,
          preset_id TEXT,
          ensemble_run_id TEXT,
          provider TEXT NOT NULL,
          model TEXT NOT NULL,
          effort TEXT,
          billing TEXT NOT NULL,
          original_text TEXT NOT NULL,
          result_text TEXT,
          outcome TEXT NOT NULL,
          reason_code TEXT,
          target_app TEXT,
          duration_ms INTEGER,
          first_token_ms INTEGER,
          input_tokens INTEGER,
          output_tokens INTEGER,
          estimated_cost_usd REAL
        );
        CREATE INDEX IF NOT EXISTS text_assist_runs_created ON text_assist_runs(created_at);
        "#,
    )
    .context("Could not initialize Text Assist history")?;
    Ok(())
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(transcripts)")
        .context("Could not inspect history schema")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()
        .context("Could not read history columns")?;

    let migrations = [
        (
            "pipeline_started_at",
            "ALTER TABLE transcripts ADD COLUMN pipeline_started_at TEXT",
        ),
        (
            "recording_duration_ms",
            "ALTER TABLE transcripts ADD COLUMN recording_duration_ms INTEGER",
        ),
        (
            "transcription_duration_ms",
            "ALTER TABLE transcripts ADD COLUMN transcription_duration_ms INTEGER",
        ),
        (
            "cleanup_duration_ms",
            "ALTER TABLE transcripts ADD COLUMN cleanup_duration_ms INTEGER",
        ),
        (
            "paste_duration_ms",
            "ALTER TABLE transcripts ADD COLUMN paste_duration_ms INTEGER",
        ),
        (
            "total_pipeline_duration_ms",
            "ALTER TABLE transcripts ADD COLUMN total_pipeline_duration_ms INTEGER",
        ),
    ];

    for (column, sql) in migrations {
        if !columns.iter().any(|existing| existing == column) {
            conn.execute(sql, [])
                .with_context(|| format!("Could not add history column {column}"))?;
        }
    }
    Ok(())
}

pub fn insert(item: &TranscriptHistoryItem) -> anyhow::Result<()> {
    let conn = connection()?;
    conn.execute(
        r#"
        INSERT OR REPLACE INTO transcripts (
          id, created_at, provider, stt_model, cleanup_model, cleanup_mode,
          raw_text, final_text, duration_ms, word_count, char_count, pasted, error,
          pipeline_started_at, recording_duration_ms, transcription_duration_ms,
          cleanup_duration_ms, paste_duration_ms, total_pipeline_duration_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        "#,
        params![
            item.id,
            item.created_at,
            "openai",
            item.stt_model.as_str(),
            item.cleanup_model,
            match item.cleanup_mode { CleanupMode::Off => "off", CleanupMode::Light => "light" },
            item.raw_text,
            item.final_text,
            item.duration_ms.map(|value| value as i64),
            item.word_count as i64,
            item.char_count as i64,
            if item.pasted { 1 } else { 0 },
            item.error,
            item.pipeline_started_at,
            item.recording_duration_ms.map(|value| value as i64),
            item.transcription_duration_ms.map(|value| value as i64),
            item.cleanup_duration_ms.map(|value| value as i64),
            item.paste_duration_ms.map(|value| value as i64),
            item.total_pipeline_duration_ms.map(|value| value as i64),
        ],
    )
    .context("Could not insert transcript history item")?;
    Ok(())
}

pub fn list(limit: usize) -> anyhow::Result<Vec<TranscriptHistoryItem>> {
    let conn = connection()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, created_at, stt_model, cleanup_model, cleanup_mode,
                   raw_text, final_text, duration_ms, word_count, char_count, pasted, error,
                   pipeline_started_at, recording_duration_ms, transcription_duration_ms,
                   cleanup_duration_ms, paste_duration_ms, total_pipeline_duration_ms
            FROM transcripts
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )
        .context("Could not prepare history query")?;

    let rows = stmt.query_map([limit as i64], |row| {
        let stt_model_text: String = row.get(2)?;
        let cleanup_mode_text: String = row.get(4)?;
        Ok(TranscriptHistoryItem {
            id: row.get(0)?,
            created_at: row.get(1)?,
            provider: Provider::Openai,
            stt_model: parse_stt_model(&stt_model_text),
            cleanup_model: row.get(3)?,
            cleanup_mode: if cleanup_mode_text == "light" {
                CleanupMode::Light
            } else {
                CleanupMode::Off
            },
            raw_text: row.get(5)?,
            final_text: row.get(6)?,
            duration_ms: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
            word_count: row.get::<_, i64>(8)? as usize,
            char_count: row.get::<_, i64>(9)? as usize,
            pasted: row.get::<_, i64>(10)? == 1,
            error: row.get(11)?,
            pipeline_started_at: row.get(12)?,
            recording_duration_ms: row.get::<_, Option<i64>>(13)?.map(|value| value as u64),
            transcription_duration_ms: row.get::<_, Option<i64>>(14)?.map(|value| value as u64),
            cleanup_duration_ms: row.get::<_, Option<i64>>(15)?.map(|value| value as u64),
            paste_duration_ms: row.get::<_, Option<i64>>(16)?.map(|value| value as u64),
            total_pipeline_duration_ms: row.get::<_, Option<i64>>(17)?.map(|value| value as u64),
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .context("Could not read transcript history")
}

pub fn get(id: &str) -> anyhow::Result<Option<TranscriptHistoryItem>> {
    let conn = connection()?;
    conn.query_row(
        r#"
        SELECT id, created_at, stt_model, cleanup_model, cleanup_mode,
               raw_text, final_text, duration_ms, word_count, char_count, pasted, error,
               pipeline_started_at, recording_duration_ms, transcription_duration_ms,
               cleanup_duration_ms, paste_duration_ms, total_pipeline_duration_ms
        FROM transcripts
        WHERE id = ?1
        "#,
        [id],
        |row| {
            let stt_model_text: String = row.get(2)?;
            let cleanup_mode_text: String = row.get(4)?;
            Ok(TranscriptHistoryItem {
                id: row.get(0)?,
                created_at: row.get(1)?,
                provider: Provider::Openai,
                stt_model: parse_stt_model(&stt_model_text),
                cleanup_model: row.get(3)?,
                cleanup_mode: if cleanup_mode_text == "light" {
                    CleanupMode::Light
                } else {
                    CleanupMode::Off
                },
                raw_text: row.get(5)?,
                final_text: row.get(6)?,
                duration_ms: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
                word_count: row.get::<_, i64>(8)? as usize,
                char_count: row.get::<_, i64>(9)? as usize,
                pasted: row.get::<_, i64>(10)? == 1,
                error: row.get(11)?,
                pipeline_started_at: row.get(12)?,
                recording_duration_ms: row.get::<_, Option<i64>>(13)?.map(|value| value as u64),
                transcription_duration_ms: row.get::<_, Option<i64>>(14)?.map(|value| value as u64),
                cleanup_duration_ms: row.get::<_, Option<i64>>(15)?.map(|value| value as u64),
                paste_duration_ms: row.get::<_, Option<i64>>(16)?.map(|value| value as u64),
                total_pipeline_duration_ms: row
                    .get::<_, Option<i64>>(17)?
                    .map(|value| value as u64),
            })
        },
    )
    .optional()
    .context("Could not load transcript")
}

pub fn delete(id: &str) -> anyhow::Result<bool> {
    let conn = connection()?;
    delete_from(&conn, id)
}

fn delete_from(conn: &Connection, id: &str) -> anyhow::Result<bool> {
    Ok(conn.execute("DELETE FROM transcripts WHERE id = ?1", [id])? > 0)
}

pub fn clear() -> anyhow::Result<usize> {
    let conn = connection()?;
    clear_from(&conn)
}

fn clear_from(conn: &Connection) -> anyhow::Result<usize> {
    conn.execute("DELETE FROM transcripts", [])
        .context("Could not clear local history")
}

// Text Assist runs (spec section 7.3). Wired into the job orchestrator in Task G; until then
// the items below are only used by tests, hence `allow(dead_code)`.

#[allow(dead_code)]
pub const KIND_POLISH: &str = "polish";
#[allow(dead_code)]
pub const KIND_REWRITE: &str = "rewrite";
#[allow(dead_code)]
pub const KIND_ENSEMBLE: &str = "ensemble";

#[allow(dead_code)]
pub const OUTCOME_APPLIED: &str = "applied";
#[allow(dead_code)]
pub const OUTCOME_NO_CHANGE: &str = "no_change";
#[allow(dead_code)]
pub const OUTCOME_KEPT: &str = "kept";
#[allow(dead_code)]
pub const OUTCOME_CANCELLED: &str = "cancelled";
#[allow(dead_code)]
pub const OUTCOME_FAILED: &str = "failed";

/// One Text Assist job (or one ensemble member). `kind` is a `KIND_*` value, `outcome` an
/// `OUTCOME_*` value, `provider` a [`ProviderKind`](crate::text_providers::ProviderKind)
/// string, `billing` a [`Billing`] string. `reason_code` is sanitized and `target_app` is the
/// executable file name only.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistRun {
    pub id: String,
    pub created_at: String,
    pub kind: String,
    pub preset_id: Option<String>,
    pub ensemble_run_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
    pub billing: String,
    pub original_text: String,
    pub result_text: Option<String>,
    pub outcome: String,
    pub reason_code: Option<String>,
    pub target_app: Option<String>,
    pub duration_ms: Option<u64>,
    pub first_token_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
}

/// Dashboard totals for Text Assist, separate from dictation (spec section 7.4).
#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistCostTotals {
    pub runs: usize,
    pub applied_runs: usize,
    /// Sum of estimated API cost over all runs. Plan and local runs cost nothing extra.
    pub estimated_cost_usd: f64,
    /// The same sum for runs created on `today` (local time).
    pub estimated_cost_today_usd: f64,
    /// Runs billed to a ChatGPT or Claude plan.
    pub plan_runs: usize,
}

#[allow(dead_code)]
pub fn insert_text_assist_run(run: &TextAssistRun) -> anyhow::Result<()> {
    insert_text_assist_run_in(&connection()?, run)
}

#[allow(dead_code)]
pub fn update_text_assist_run_outcome(
    id: &str,
    outcome: &str,
    reason_code: Option<&str>,
) -> anyhow::Result<bool> {
    update_text_assist_run_outcome_in(&connection()?, id, outcome, reason_code)
}

#[allow(dead_code)]
pub fn list_text_assist_runs(limit: usize) -> anyhow::Result<Vec<TextAssistRun>> {
    list_text_assist_runs_in(&connection()?, limit)
}

#[allow(dead_code)]
pub fn delete_text_assist_run(id: &str) -> anyhow::Result<bool> {
    delete_text_assist_run_in(&connection()?, id)
}

#[allow(dead_code)]
pub fn clear_text_assist_runs() -> anyhow::Result<usize> {
    clear_text_assist_runs_in(&connection()?)
}

#[allow(dead_code)]
pub fn text_assist_cost_totals(today: NaiveDate) -> anyhow::Result<TextAssistCostTotals> {
    text_assist_cost_totals_in(&connection()?, today)
}

fn insert_text_assist_run_in(conn: &Connection, run: &TextAssistRun) -> anyhow::Result<()> {
    conn.execute(
        r#"
        INSERT OR REPLACE INTO text_assist_runs (
          id, created_at, kind, preset_id, ensemble_run_id, provider, model, effort, billing,
          original_text, result_text, outcome, reason_code, target_app, duration_ms,
          first_token_ms, input_tokens, output_tokens, estimated_cost_usd
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        "#,
        params![
            run.id,
            run.created_at,
            run.kind,
            run.preset_id,
            run.ensemble_run_id,
            run.provider,
            run.model,
            run.effort,
            run.billing,
            run.original_text,
            run.result_text,
            run.outcome,
            run.reason_code,
            run.target_app,
            run.duration_ms.map(|value| value as i64),
            run.first_token_ms.map(|value| value as i64),
            run.input_tokens.map(|value| value as i64),
            run.output_tokens.map(|value| value as i64),
            run.estimated_cost_usd,
        ],
    )
    .context("Could not insert Text Assist history item")?;
    Ok(())
}

fn update_text_assist_run_outcome_in(
    conn: &Connection,
    id: &str,
    outcome: &str,
    reason_code: Option<&str>,
) -> anyhow::Result<bool> {
    let changed = conn
        .execute(
            "UPDATE text_assist_runs SET outcome = ?2, reason_code = ?3 WHERE id = ?1",
            params![id, outcome, reason_code],
        )
        .context("Could not update Text Assist history item")?;
    Ok(changed > 0)
}

fn list_text_assist_runs_in(conn: &Connection, limit: usize) -> anyhow::Result<Vec<TextAssistRun>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, created_at, kind, preset_id, ensemble_run_id, provider, model, effort,
                   billing, original_text, result_text, outcome, reason_code, target_app,
                   duration_ms, first_token_ms, input_tokens, output_tokens, estimated_cost_usd
            FROM text_assist_runs
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )
        .context("Could not prepare Text Assist history query")?;
    let optional_u64 = |value: Option<i64>| value.map(|value| value.max(0) as u64);
    let rows = stmt.query_map([limit.min(i64::MAX as usize) as i64], |row| {
        Ok(TextAssistRun {
            id: row.get(0)?,
            created_at: row.get(1)?,
            kind: row.get(2)?,
            preset_id: row.get(3)?,
            ensemble_run_id: row.get(4)?,
            provider: row.get(5)?,
            model: row.get(6)?,
            effort: row.get(7)?,
            billing: row.get(8)?,
            original_text: row.get(9)?,
            result_text: row.get(10)?,
            outcome: row.get(11)?,
            reason_code: row.get(12)?,
            target_app: row.get(13)?,
            duration_ms: optional_u64(row.get(14)?),
            first_token_ms: optional_u64(row.get(15)?),
            input_tokens: optional_u64(row.get(16)?),
            output_tokens: optional_u64(row.get(17)?),
            estimated_cost_usd: row.get(18)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .context("Could not read Text Assist history")
}

fn delete_text_assist_run_in(conn: &Connection, id: &str) -> anyhow::Result<bool> {
    Ok(conn
        .execute("DELETE FROM text_assist_runs WHERE id = ?1", [id])
        .context("Could not delete Text Assist history item")?
        > 0)
}

fn clear_text_assist_runs_in(conn: &Connection) -> anyhow::Result<usize> {
    conn.execute("DELETE FROM text_assist_runs", [])
        .context("Could not clear Text Assist history")
}

fn text_assist_cost_totals_in(
    conn: &Connection,
    today: NaiveDate,
) -> anyhow::Result<TextAssistCostTotals> {
    let mut stmt = conn
        .prepare("SELECT created_at, outcome, billing, estimated_cost_usd FROM text_assist_runs")
        .context("Could not prepare Text Assist totals query")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<f64>>(3)?,
        ))
    })?;
    let mut totals = TextAssistCostTotals::default();
    for row in rows {
        let (created_at, outcome, billing, cost) =
            row.context("Could not read Text Assist totals")?;
        totals.runs += 1;
        if outcome == OUTCOME_APPLIED {
            totals.applied_runs += 1;
        }
        if billing == Billing::ChatgptPlan.as_str() || billing == Billing::ClaudePlan.as_str() {
            totals.plan_runs += 1;
        }
        let cost = cost.filter(|cost| cost.is_finite() && *cost > 0.0);
        if let (Some(cost), true) = (cost, billing == Billing::Api.as_str()) {
            totals.estimated_cost_usd += cost;
            if parse_local_date(&created_at) == Some(today) {
                totals.estimated_cost_today_usd += cost;
            }
        }
    }
    Ok(totals)
}

pub fn counts(text: &str) -> (usize, usize) {
    let words = text
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .count();
    let chars = text.chars().count();
    (words, chars)
}

fn parse_stt_model(value: &str) -> SttModel {
    match value {
        "gpt-4o-mini-transcribe" => SttModel::Gpt4oMiniTranscribe,
        "gpt-4o-transcribe" => SttModel::Gpt4oTranscribe,
        "gpt-4o-transcribe-diarize" => SttModel::Gpt4oTranscribeDiarize,
        _ => SttModel::Whisper1,
    }
}

pub fn dashboard_stats() -> anyhow::Result<DashboardStats> {
    let items = list(10_000)?;
    Ok(calculate_dashboard_stats(&items, Local::now().date_naive()))
}

pub fn calculate_dashboard_stats(
    items: &[TranscriptHistoryItem],
    today: NaiveDate,
) -> DashboardStats {
    let total_words = items.iter().map(|item| item.word_count).sum::<usize>();
    let total_transcripts = items.len();
    let estimated_minutes_saved = total_words / 40;
    let average_words_per_transcript = total_words.checked_div(total_transcripts).unwrap_or(0);

    let mut week_map = BTreeMap::new();
    for offset in (0..7).rev() {
        let date = today - Duration::days(offset);
        week_map.insert(date, (0usize, 0usize));
    }

    let mut model_counts: BTreeMap<String, (usize, u64, usize)> = BTreeMap::new();
    let mut transcripts_today = 0usize;
    let mut words_today = 0usize;

    for item in items {
        let date = parse_local_date(&item.created_at);
        if date == Some(today) {
            transcripts_today += 1;
            words_today += item.word_count;
        }
        if let Some(date) = date {
            if let Some((count, words)) = week_map.get_mut(&date) {
                *count += 1;
                *words += item.word_count;
            }
        }

        let entry = model_counts
            .entry(item.stt_model.as_str().to_string())
            .or_default();
        entry.0 += 1;
        if let Some(duration) = item.transcription_duration_ms.or(item.duration_ms) {
            entry.1 += duration;
            entry.2 += 1;
        }
    }

    let week = week_map
        .iter()
        .map(|(date, (transcript_count, word_count))| DailyStat {
            date: date.to_string(),
            day_label: day_label(*date),
            transcript_count: *transcript_count,
            word_count: *word_count,
        })
        .collect::<Vec<_>>();

    let most_active_day = week
        .iter()
        .max_by_key(|day| day.transcript_count)
        .filter(|day| day.transcript_count > 0)
        .map(|day| day.day_label.clone());

    let model_usage = model_counts
        .into_iter()
        .map(
            |(model, (transcript_count, total_duration, duration_count))| ModelUsageStat {
                model,
                transcript_count,
                average_duration_ms: if duration_count == 0 {
                    None
                } else {
                    Some(total_duration / duration_count as u64)
                },
            },
        )
        .collect();

    DashboardStats {
        total_words,
        total_transcripts,
        transcripts_today,
        words_today,
        estimated_minutes_saved,
        average_words_per_transcript,
        most_active_day,
        week,
        model_usage,
        cost: pricing::calculate_cost_summary(items, today),
    }
}

fn parse_local_date(value: &str) -> Option<NaiveDate> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Local).date_naive())
}

fn day_label(date: NaiveDate) -> String {
    match date.weekday() {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_words_and_chars() {
        assert_eq!(counts("hello brave flow"), (3, 16));
    }

    #[test]
    fn calculates_time_saved() {
        let item = TranscriptHistoryItem {
            id: "1".to_string(),
            created_at: "2026-05-25T10:00:00+02:00".to_string(),
            provider: Provider::Openai,
            stt_model: SttModel::Whisper1,
            cleanup_model: None,
            cleanup_mode: CleanupMode::Off,
            raw_text: "x".to_string(),
            final_text: "x".to_string(),
            duration_ms: Some(100),
            word_count: 80,
            char_count: 1,
            pasted: true,
            error: None,
            pipeline_started_at: None,
            recording_duration_ms: None,
            transcription_duration_ms: Some(100),
            cleanup_duration_ms: None,
            paste_duration_ms: None,
            total_pipeline_duration_ms: None,
        };
        let stats =
            calculate_dashboard_stats(&[item], NaiveDate::from_ymd_opt(2026, 5, 25).unwrap());
        assert_eq!(stats.estimated_minutes_saved, 2);
        assert_eq!(stats.transcripts_today, 1);
    }
    #[test]
    fn deleting_history_is_scoped_and_clear_returns_count() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE transcripts (id TEXT PRIMARY KEY); INSERT INTO transcripts VALUES ('one'), ('two');").unwrap();
        assert!(!delete_from(&conn, "one' OR 1=1 --").unwrap());
        assert!(delete_from(&conn, "one").unwrap());
        assert!(!delete_from(&conn, "one").unwrap());
        assert_eq!(clear_from(&conn).unwrap(), 1);
        assert_eq!(clear_from(&conn).unwrap(), 0);
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
            original_text: "teh text".to_string(),
            result_text: Some("the text".to_string()),
            outcome: OUTCOME_APPLIED.to_string(),
            reason_code: None,
            target_app: Some("notepad.exe".to_string()),
            duration_ms: Some(820),
            first_token_ms: Some(310),
            input_tokens: Some(450),
            output_tokens: Some(12),
            estimated_cost_usd: Some(0.0001),
        }
    }

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type IN ('table', 'index') ORDER BY name",
            )
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn existing_transcripts_database_gains_text_assist_table_and_keeps_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        {
            // Schema written by the current release: transcripts only, all migrations applied.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE transcripts (
                  id TEXT PRIMARY KEY, created_at TEXT NOT NULL, provider TEXT NOT NULL,
                  stt_model TEXT NOT NULL, cleanup_model TEXT, cleanup_mode TEXT NOT NULL,
                  raw_text TEXT NOT NULL, final_text TEXT NOT NULL, duration_ms INTEGER,
                  word_count INTEGER NOT NULL, char_count INTEGER NOT NULL,
                  pasted INTEGER NOT NULL, error TEXT, pipeline_started_at TEXT,
                  recording_duration_ms INTEGER, transcription_duration_ms INTEGER,
                  cleanup_duration_ms INTEGER, paste_duration_ms INTEGER,
                  total_pipeline_duration_ms INTEGER
                );
                CREATE INDEX transcripts_created_at ON transcripts(created_at);
                INSERT INTO transcripts (id, created_at, provider, stt_model, cleanup_mode,
                  raw_text, final_text, word_count, char_count, pasted)
                VALUES ('t1', '2026-11-16T10:00:00+01:00', 'openai', 'whisper-1', 'off',
                  'hello', 'hello', 1, 5, 1);
                "#,
            )
            .unwrap();
        }
        let conn = Connection::open(&path).unwrap();
        init_schema(&conn).unwrap();
        // Idempotent: a second start changes nothing.
        init_schema(&conn).unwrap();
        let names = table_names(&conn);
        assert!(names.contains(&"text_assist_runs".to_string()));
        assert!(names.contains(&"text_assist_runs_created".to_string()));
        let transcripts: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcripts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(transcripts, 1);
        assert!(list_text_assist_runs_in(&conn, 10).unwrap().is_empty());
        insert_text_assist_run_in(&conn, &run("r1", "2026-11-17T09:00:00+01:00")).unwrap();
        assert_eq!(list_text_assist_runs_in(&conn, 10).unwrap().len(), 1);
    }

    #[test]
    fn text_assist_runs_round_trip_and_list_newest_first() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let mut sparse = run("old", "2026-11-15T09:00:00+01:00");
        sparse.kind = KIND_ENSEMBLE.to_string();
        sparse.preset_id = None;
        sparse.ensemble_run_id = Some("e1".to_string());
        sparse.effort = None;
        sparse.result_text = None;
        sparse.outcome = OUTCOME_FAILED.to_string();
        sparse.reason_code = Some("rate_limited".to_string());
        sparse.target_app = None;
        sparse.duration_ms = None;
        sparse.first_token_ms = None;
        sparse.input_tokens = None;
        sparse.output_tokens = None;
        sparse.estimated_cost_usd = None;
        let newest = run("new", "2026-11-17T09:00:00+01:00");
        let middle = run("mid", "2026-11-16T09:00:00+01:00");
        for item in [&sparse, &newest, &middle] {
            insert_text_assist_run_in(&conn, item).unwrap();
        }
        let listed = list_text_assist_runs_in(&conn, 10).unwrap();
        assert_eq!(listed, vec![newest.clone(), middle, sparse]);
        let limited = list_text_assist_runs_in(&conn, 1).unwrap();
        assert_eq!(limited, vec![newest]);
    }

    #[test]
    fn text_assist_outcome_update_delete_and_clear_are_scoped() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        insert_text_assist_run_in(&conn, &run("one", "2026-11-17T09:00:00+01:00")).unwrap();
        insert_text_assist_run_in(&conn, &run("two", "2026-11-17T10:00:00+01:00")).unwrap();

        assert!(update_text_assist_run_outcome_in(
            &conn,
            "one",
            OUTCOME_CANCELLED,
            Some("target_changed")
        )
        .unwrap());
        assert!(!update_text_assist_run_outcome_in(&conn, "missing", OUTCOME_KEPT, None).unwrap());
        let listed = list_text_assist_runs_in(&conn, 10).unwrap();
        let one = listed.iter().find(|item| item.id == "one").unwrap();
        let two = listed.iter().find(|item| item.id == "two").unwrap();
        assert_eq!(one.outcome, OUTCOME_CANCELLED);
        assert_eq!(one.reason_code.as_deref(), Some("target_changed"));
        assert_eq!(two.outcome, OUTCOME_APPLIED);
        assert!(update_text_assist_run_outcome_in(&conn, "one", OUTCOME_NO_CHANGE, None).unwrap());
        let listed = list_text_assist_runs_in(&conn, 10).unwrap();
        let one = listed.iter().find(|item| item.id == "one").unwrap();
        assert_eq!(one.outcome, OUTCOME_NO_CHANGE);
        assert_eq!(one.reason_code, None);

        assert!(!delete_text_assist_run_in(&conn, "one' OR 1=1 --").unwrap());
        assert!(delete_text_assist_run_in(&conn, "one").unwrap());
        assert!(!delete_text_assist_run_in(&conn, "one").unwrap());
        assert_eq!(list_text_assist_runs_in(&conn, 10).unwrap().len(), 1);
        assert_eq!(clear_text_assist_runs_in(&conn).unwrap(), 1);
        assert_eq!(clear_text_assist_runs_in(&conn).unwrap(), 0);
    }

    #[test]
    fn text_assist_runs_are_separate_from_transcripts() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO transcripts (id, created_at, provider, stt_model, cleanup_mode, raw_text, final_text, word_count, char_count, pasted) VALUES ('t', '2026-11-17T09:00:00+01:00', 'openai', 'whisper-1', 'off', 'x', 'x', 1, 1, 1)",
            [],
        )
        .unwrap();
        insert_text_assist_run_in(&conn, &run("r", "2026-11-17T09:00:00+01:00")).unwrap();
        assert_eq!(clear_text_assist_runs_in(&conn).unwrap(), 1);
        assert_eq!(clear_from(&conn).unwrap(), 1);
    }

    #[test]
    fn text_assist_cost_totals_count_runs_and_api_cost() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        assert_eq!(
            text_assist_cost_totals_in(&conn, NaiveDate::from_ymd_opt(2026, 11, 17).unwrap())
                .unwrap(),
            TextAssistCostTotals::default()
        );
        // Noon local time keeps the date stable in every time zone the test may run in.
        let today = Local::now().date_naive();
        let at = |date: NaiveDate| {
            date.and_hms_opt(12, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap()
                .to_rfc3339()
        };
        let yesterday = today - Duration::days(1);

        let mut api_today = run("a", &at(today));
        api_today.estimated_cost_usd = Some(0.25);
        let mut api_yesterday = run("b", &at(yesterday));
        api_yesterday.estimated_cost_usd = Some(0.5);
        api_yesterday.outcome = OUTCOME_KEPT.to_string();
        let mut plan = run("c", &at(today));
        plan.provider = "codex_cli".to_string();
        plan.billing = Billing::ChatgptPlan.as_str().to_string();
        plan.estimated_cost_usd = Some(9.0);
        let mut claude_plan = run("d", &at(today));
        claude_plan.billing = Billing::ClaudePlan.as_str().to_string();
        claude_plan.estimated_cost_usd = None;
        claude_plan.outcome = OUTCOME_FAILED.to_string();
        let mut local = run("e", &at(today));
        local.billing = Billing::Local.as_str().to_string();
        local.estimated_cost_usd = Some(0.0);
        let mut unknown_cost = run("f", &at(today));
        unknown_cost.estimated_cost_usd = None;
        unknown_cost.outcome = OUTCOME_NO_CHANGE.to_string();
        for item in [
            &api_today,
            &api_yesterday,
            &plan,
            &claude_plan,
            &local,
            &unknown_cost,
        ] {
            insert_text_assist_run_in(&conn, item).unwrap();
        }

        let totals = text_assist_cost_totals_in(&conn, today).unwrap();
        assert_eq!(totals.runs, 6);
        assert_eq!(totals.applied_runs, 3);
        assert_eq!(totals.plan_runs, 2);
        assert!((totals.estimated_cost_usd - 0.75).abs() < 1e-12);
        assert!((totals.estimated_cost_today_usd - 0.25).abs() < 1e-12);
    }

    #[test]
    fn text_assist_run_serializes_camel_case() {
        let json = serde_json::to_value(run("r", "2026-11-17T09:00:00+01:00")).unwrap();
        assert_eq!(json["originalText"], "teh text");
        assert_eq!(json["firstTokenMs"], 310);
        assert_eq!(json["estimatedCostUsd"], 0.0001);
        assert_eq!(json["targetApp"], "notepad.exe");
    }
}
