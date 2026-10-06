use std::collections::BTreeMap;

use anyhow::Context;
use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    brand, pricing, settings,
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
    migrate(&conn)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS transcripts_created_at ON transcripts(created_at)",
        [],
    )?;
    Ok(conn)
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
}
