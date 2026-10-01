//! Usage statistics kept in hourly buckets next to the history rows.
//! Buckets are only ever added to: deleting rows, clearing the history and
//! pruning leave them alone.

use std::path::Path;

use rusqlite::{params, Connection};

/// Typing speed assumed when estimating the time dictation saved.
pub const TYPING_CHARS_PER_MIN: u64 = 40;
/// Shorter clips are left out of the speaking speed figures.
pub const SPEED_MIN_AUDIO_MS: u64 = 3000;

/// Unicode scalars that are not whitespace.
pub fn count_chars(text: &str) -> u64 {
    text.chars().filter(|c| !c.is_whitespace()).count() as u64
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scripts {
    pub han: u64,
    pub latin: u64,
    pub kana: u64,
}

/// Counts Han, ASCII letters and kana; every other character is not counted.
pub fn scripts(text: &str) -> Scripts {
    let mut s = Scripts::default();
    for c in text.chars() {
        match c {
            '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' => s.han += 1,
            '\u{3040}'..='\u{30FF}' => s.kana += 1,
            c if c.is_ascii_alphabetic() => s.latin += 1,
            _ => {}
        }
    }
    s
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Totals {
    pub count: u64,
    pub chars: u64,
    pub audio_ms: u64,
    /// Rows with a known audio length, and their characters.
    pub timed_count: u64,
    pub timed_chars: u64,
    /// Rows long enough for the speed figures, their characters and audio.
    pub speed_chars: u64,
    pub speed_audio_ms: u64,
    pub asr_ms: u64,
    pub llm_ms: u64,
    pub llm_count: u64,
    pub changed: u64,
    pub degraded: u64,
    pub unrewritten: u64,
    pub dict_fixes: u64,
    pub snippets: u64,
    pub han: u64,
    pub latin: u64,
    pub kana: u64,
    pub max_chars: u64,
    pub max_audio_ms: u64,
    pub max_total_ms: u64,
    pub max_cpm: u64,
}

/// `day` is the number of local days since the epoch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DayRow {
    pub day: i64,
    pub count: u64,
    pub chars: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Named {
    pub name: String,
    pub count: u64,
    pub chars: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Stats {
    pub totals: Totals,
    /// None when no dictation has a known audio length.
    pub saved_ms: Option<u64>,
    /// Within the period, ascending.
    pub days: Vec<DayRow>,
    /// Counts by local hour of day.
    pub hours: Vec<u64>,
    /// By count, descending; an unknown application has an empty name.
    pub apps: Vec<Named>,
    pub profiles: Vec<Named>,
    /// All time, ascending.
    pub active_days: Vec<DayRow>,
}

/// What one dictation adds to its hour.
pub(crate) struct Sample<'a> {
    pub ts: i64,
    pub app_name: &'a str,
    pub profile_id: &'a str,
    pub text: &'a str,
    pub audio_ms: Option<u64>,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    pub changed: bool,
    pub degraded: bool,
    pub dict_fixes: u32,
    pub snippets: u32,
}

pub(crate) fn add(conn: &Connection, s: &Sample<'_>) -> rusqlite::Result<()> {
    let chars = count_chars(s.text);
    let sc = scripts(s.text);
    let audio = s.audio_ms.unwrap_or(0);
    let timed = s.audio_ms.is_some();
    let speed = s.audio_ms.is_some_and(|ms| ms >= SPEED_MIN_AUDIO_MS);
    let cpm = if speed { chars * 60_000 / audio } else { 0 };
    let total_ms = s.asr_ms + s.llm_ms.unwrap_or(0);
    conn.execute(
        "INSERT INTO stats_hourly (hour, app_name, profile_id, count, chars, audio_ms, timed_count, timed_chars,
            speed_chars, speed_audio_ms, asr_ms, llm_ms, llm_count, changed, degraded, unrewritten, dict_fixes, snippets,
            han, latin, kana, max_chars, max_audio_ms, max_total_ms, max_cpm)
         VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?4, ?5, ?21, ?22)
         ON CONFLICT (hour, app_name, profile_id) DO UPDATE SET
            count = count + 1, chars = chars + excluded.chars, audio_ms = audio_ms + excluded.audio_ms,
            timed_count = timed_count + excluded.timed_count, timed_chars = timed_chars + excluded.timed_chars,
            speed_chars = speed_chars + excluded.speed_chars, speed_audio_ms = speed_audio_ms + excluded.speed_audio_ms,
            asr_ms = asr_ms + excluded.asr_ms, llm_ms = llm_ms + excluded.llm_ms, llm_count = llm_count + excluded.llm_count,
            changed = changed + excluded.changed, degraded = degraded + excluded.degraded,
            unrewritten = unrewritten + excluded.unrewritten, dict_fixes = dict_fixes + excluded.dict_fixes,
            snippets = snippets + excluded.snippets, han = han + excluded.han, latin = latin + excluded.latin,
            kana = kana + excluded.kana, max_chars = MAX(max_chars, excluded.max_chars),
            max_audio_ms = MAX(max_audio_ms, excluded.max_audio_ms),
            max_total_ms = MAX(max_total_ms, excluded.max_total_ms), max_cpm = MAX(max_cpm, excluded.max_cpm)",
        params![
            s.ts - s.ts.rem_euclid(3600),
            s.app_name,
            s.profile_id,
            chars as i64,
            audio as i64,
            timed as i64,
            if timed { chars as i64 } else { 0 },
            if speed { chars as i64 } else { 0 },
            if speed { audio as i64 } else { 0 },
            s.asr_ms as i64,
            s.llm_ms.unwrap_or(0) as i64,
            s.llm_ms.is_some() as i64,
            s.changed as i64,
            s.degraded as i64,
            (s.llm_ms.is_none() && !s.degraded) as i64,
            s.dict_fixes as i64,
            s.snippets as i64,
            sc.han as i64,
            sc.latin as i64,
            sc.kana as i64,
            total_ms as i64,
            cpm as i64,
        ],
    )?;
    Ok(())
}

/// Adds every existing history row to its bucket and records the audio
/// length of the recordings that are still on disk. Runs once per database.
pub(crate) fn backfill(
    conn: &Connection,
    recordings_dir: &Path,
) -> Result<(), crate::history::HistoryError> {
    let done = conn
        .prepare("SELECT 1 FROM stats_meta WHERE key = 'backfilled'")?
        .exists([])?;
    if done {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    // An earlier attempt may have failed after dictations were counted; start from nothing.
    tx.execute("DELETE FROM stats_hourly", [])?;
    struct Row {
        id: i64,
        ts: i64,
        profile_id: String,
        app_name: Option<String>,
        raw_text: String,
        refined_text: String,
        final_text: String,
        degraded: bool,
        audio_path: Option<String>,
        asr_ms: u64,
        llm_ms: Option<u64>,
    }
    let rows: Vec<Row> = {
        let mut stmt = tx.prepare(
            "SELECT id, ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path, asr_ms, llm_ms
             FROM utterances",
        )?;
        let mapped = stmt.query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                ts: r.get(1)?,
                profile_id: r.get(2)?,
                app_name: r.get(3)?,
                raw_text: r.get(4)?,
                refined_text: r.get(5)?,
                final_text: r.get(6)?,
                degraded: r.get::<_, i64>(7)? != 0,
                audio_path: r.get(8)?,
                asr_ms: r.get::<_, i64>(9)? as u64,
                llm_ms: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            })
        })?;
        mapped.collect::<Result<_, _>>()?
    };
    for row in rows {
        let audio_ms = row.audio_path.as_deref().and_then(|p| {
            let p = Path::new(p);
            if crate::history::inside(p, recordings_dir) {
                crate::audio::wav_duration_ms(p)
            } else {
                None
            }
        });
        if let Some(ms) = audio_ms {
            tx.execute(
                "UPDATE utterances SET audio_ms = ?1 WHERE id = ?2",
                params![ms as i64, row.id],
            )?;
        }
        add(
            &tx,
            &Sample {
                ts: row.ts,
                app_name: row.app_name.as_deref().unwrap_or(""),
                profile_id: &row.profile_id,
                text: &row.final_text,
                audio_ms,
                asr_ms: row.asr_ms,
                llm_ms: row.llm_ms,
                changed: row.llm_ms.is_some() && row.raw_text != row.refined_text,
                degraded: row.degraded,
                dict_fixes: 0,
                snippets: 0,
            },
        )?;
    }
    tx.execute(
        "INSERT INTO stats_meta (key, value) VALUES ('backfilled', '1')",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

/// Seconds into the local day of a bucket. SQLite's `%` keeps the sign of the
/// dividend, so the double modulo is what makes it correct for negative values.
const LOCAL_SECS: &str = "(((hour + ?2) % 86400 + 86400) % 86400)";

fn day_rows(conn: &Connection, from: i64, tz: i64) -> rusqlite::Result<Vec<DayRow>> {
    let sql = format!(
        "SELECT (hour + ?2 - {LOCAL_SECS}) / 86400 AS day, SUM(count), SUM(chars)
         FROM stats_hourly WHERE hour >= ?1 GROUP BY day ORDER BY day"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from, tz], |r| {
        Ok(DayRow {
            day: r.get(0)?,
            count: r.get::<_, i64>(1)? as u64,
            chars: r.get::<_, i64>(2)? as u64,
        })
    })?;
    rows.collect()
}

fn named_rows(conn: &Connection, column: &str, from: i64) -> rusqlite::Result<Vec<Named>> {
    let sql = format!(
        "SELECT {column}, SUM(count), SUM(chars) FROM stats_hourly WHERE hour >= ?1
         GROUP BY {column} ORDER BY SUM(count) DESC, {column}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from], |r| {
        Ok(Named {
            name: r.get(0)?,
            count: r.get::<_, i64>(1)? as u64,
            chars: r.get::<_, i64>(2)? as u64,
        })
    })?;
    rows.collect()
}

pub(crate) fn query(conn: &Connection, since: i64, tz_offset_secs: i32) -> rusqlite::Result<Stats> {
    let from = since - since.rem_euclid(3600);
    let tz = i64::from(tz_offset_secs);
    let sums = [
        "count",
        "chars",
        "audio_ms",
        "timed_count",
        "timed_chars",
        "speed_chars",
        "speed_audio_ms",
        "asr_ms",
        "llm_ms",
        "llm_count",
        "changed",
        "degraded",
        "unrewritten",
        "dict_fixes",
        "snippets",
        "han",
        "latin",
        "kana",
    ];
    let maxes = ["max_chars", "max_audio_ms", "max_total_ms", "max_cpm"];
    let select: Vec<String> = sums
        .iter()
        .map(|c| format!("COALESCE(SUM({c}), 0)"))
        .chain(maxes.iter().map(|c| format!("COALESCE(MAX({c}), 0)")))
        .collect();
    let sql = format!(
        "SELECT {} FROM stats_hourly WHERE hour >= ?1",
        select.join(", ")
    );
    let totals = conn.query_row(&sql, params![from], |r| {
        let v = |i: usize| r.get::<_, i64>(i).map(|n| n as u64);
        Ok(Totals {
            count: v(0)?,
            chars: v(1)?,
            audio_ms: v(2)?,
            timed_count: v(3)?,
            timed_chars: v(4)?,
            speed_chars: v(5)?,
            speed_audio_ms: v(6)?,
            asr_ms: v(7)?,
            llm_ms: v(8)?,
            llm_count: v(9)?,
            changed: v(10)?,
            degraded: v(11)?,
            unrewritten: v(12)?,
            dict_fixes: v(13)?,
            snippets: v(14)?,
            han: v(15)?,
            latin: v(16)?,
            kana: v(17)?,
            max_chars: v(18)?,
            max_audio_ms: v(19)?,
            max_total_ms: v(20)?,
            max_cpm: v(21)?,
        })
    })?;

    let mut hours = vec![0u64; 24];
    {
        let sql = format!(
            "SELECT {LOCAL_SECS} / 3600 AS h, SUM(count) FROM stats_hourly WHERE hour >= ?1 GROUP BY h"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![from, tz], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (h, n) = row?;
            if let Some(slot) = hours.get_mut(h as usize) {
                *slot = n as u64;
            }
        }
    }

    let saved_ms = (totals.timed_count > 0).then(|| {
        let typing = totals.timed_chars * 60_000 / TYPING_CHARS_PER_MIN;
        let processing = (totals.asr_ms + totals.llm_ms) * totals.timed_count / totals.count;
        typing
            .saturating_sub(totals.audio_ms)
            .saturating_sub(processing)
    });

    Ok(Stats {
        saved_ms,
        days: day_rows(conn, from, tz)?,
        hours,
        apps: named_rows(conn, "app_name", from)?,
        profiles: named_rows(conn, "profile_id", from)?,
        active_days: day_rows(conn, i64::MIN, tz)?,
        totals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_exclude_whitespace() {
        assert_eq!(count_chars("今天 下午\n4 點"), 6);
        assert_eq!(count_chars(""), 0);
    }

    #[test]
    fn scripts_split_han_latin_and_kana() {
        assert_eq!(
            scripts("記得帶 MacBook です。123"),
            Scripts {
                han: 3,
                latin: 7,
                kana: 2
            }
        );
    }

    use crate::history::{HistoryStore, NewUtterance};
    use crate::types::{AudioClip, ProfileId};

    fn utt(ts: i64, text: &str, audio_ms: Option<u64>, llm_ms: Option<u64>) -> NewUtterance {
        NewUtterance {
            ts,
            profile_id: ProfileId("default".into()),
            app_name: Some("Notes".into()),
            raw_text: "raw".into(),
            refined_text: text.into(),
            final_text: text.into(),
            degraded: false,
            audio_path: None,
            asr_ms: 1000,
            llm_ms,
            audio_ms,
            dict_fixes: 2,
            snippets: 1,
            llm_changed: llm_ms.is_some(),
        }
    }

    #[test]
    fn an_insert_adds_to_its_hour_and_nothing_removes_it() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s
            .insert(utt(
                3 * 3600 + 5,
                "十個字十個字十個字字",
                Some(6000),
                Some(500),
            ))
            .unwrap();
        s.insert(utt(3 * 3600 + 900, "abc", None, None)).unwrap();
        let t = s.stats(0, 0).unwrap().totals;
        assert_eq!(
            (t.count, t.chars, t.audio_ms, t.timed_count, t.timed_chars),
            (2, 13, 6000, 1, 10)
        );
        assert_eq!(
            (t.speed_chars, t.speed_audio_ms, t.max_cpm),
            (10, 6000, 100)
        );
        assert_eq!(
            (t.asr_ms, t.llm_ms, t.llm_count, t.changed, t.unrewritten),
            (2000, 500, 1, 1, 1)
        );
        assert_eq!(
            (
                t.dict_fixes,
                t.snippets,
                t.max_chars,
                t.max_audio_ms,
                t.max_total_ms
            ),
            (4, 2, 10, 6000, 1500)
        );
        s.delete(id).unwrap();
        s.clear_all(Path::new("/nonexistent")).unwrap();
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 2);
        s.reset_stats().unwrap();
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 0);
    }

    #[test]
    fn a_short_clip_does_not_count_toward_speed() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(0, "好", Some(SPEED_MIN_AUDIO_MS - 1), None))
            .unwrap();
        let t = s.stats(0, 0).unwrap().totals;
        assert_eq!((t.timed_count, t.speed_chars, t.max_cpm), (1, 0, 0));
    }

    #[test]
    fn backfill_handles_rows_without_files() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::history::SCHEMA).unwrap();
        let wav = dir.path().join("a.wav");
        std::fs::write(
            &wav,
            crate::audio::encode_wav_pcm16(&AudioClip {
                samples: vec![0.0; 32000],
                sample_rate: 16000,
            }),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO utterances (ts, profile_id, app_name, raw_text, refined_text, final_text, audio_path, asr_ms, llm_ms) VALUES (10, 'default', 'Notes', 'x', '有錄音', '有錄音', ?1, 5, 5)",
            params![wav.to_string_lossy()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO utterances (ts, profile_id, raw_text, refined_text, final_text, audio_path) VALUES (20, 'default', 'same', 'same', 'same', '/gone/b.wav')",
            [],
        )
        .unwrap();
        crate::history::upgrade(&conn, dir.path()).unwrap();
        crate::history::upgrade(&conn, dir.path()).unwrap();
        let s = HistoryStore::from_connection(conn);
        let t = s.stats(0, 0).unwrap().totals;
        assert_eq!(
            (t.count, t.timed_count, t.audio_ms, t.changed),
            (2, 1, 2000, 1)
        );
        assert_eq!(
            s.list(10, None, false)
                .unwrap()
                .iter()
                .filter(|u| u.audio_ms == Some(2000))
                .count(),
            1
        );
    }

    #[test]
    fn the_query_groups_by_local_day_hour_application_and_profile() {
        let s = HistoryStore::open_in_memory().unwrap();
        let tz = 8 * 3600;
        for (ts, app) in [
            (86400 - 1800, "Notes"),
            (86400 + 15 * 3600 + 600, "Mail"),
            (86400 + 16 * 3600 + 1200, "Notes"),
        ] {
            let mut u = utt(ts, "四個字元", Some(4000), Some(100));
            u.app_name = Some(app.into());
            s.insert(u).unwrap();
        }
        let st = s.stats(0, tz).unwrap();
        assert_eq!(
            st.days,
            vec![
                DayRow {
                    day: 1,
                    count: 2,
                    chars: 8
                },
                DayRow {
                    day: 2,
                    count: 1,
                    chars: 4
                }
            ]
        );
        assert_eq!((st.hours[7], st.hours[23], st.hours[0]), (1, 1, 1));
        assert_eq!(
            st.apps[0],
            Named {
                name: "Notes".into(),
                count: 2,
                chars: 8
            }
        );
        assert_eq!(
            st.profiles,
            vec![Named {
                name: "default".into(),
                count: 3,
                chars: 12
            }]
        );
        assert_eq!(st.saved_ms, Some(18_000 - 12_000 - 3_300));
        let later = s.stats(86400 + 16 * 3600, tz).unwrap();
        assert_eq!(
            (
                later.totals.count,
                later.days.len(),
                later.active_days.len()
            ),
            (1, 1, 2)
        );
    }

    #[test]
    fn days_and_hours_floor_correctly_before_the_epoch() {
        let s = HistoryStore::open_in_memory().unwrap();
        // 1969-12-31 23:00 UTC is local 1969-12-31 19:00 at -4 h: day -1, hour 19.
        s.insert(utt(-3600, "a", None, None)).unwrap();
        let st = s.stats(i64::MIN / 2, -4 * 3600).unwrap();
        assert_eq!(
            st.days,
            vec![DayRow {
                day: -1,
                count: 1,
                chars: 1
            }]
        );
        assert_eq!(st.hours[19], 1);
    }

    #[test]
    fn an_empty_database_yields_zeros() {
        let st = HistoryStore::open_in_memory().unwrap().stats(0, 0).unwrap();
        assert_eq!(
            (st.totals, st.saved_ms, st.days.len(), st.hours.len()),
            (Totals::default(), None, 0, 24)
        );
    }
    #[test]
    fn a_backfill_that_runs_again_does_not_count_rows_twice() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(10, "abc", None, None)).unwrap();
        s.insert(utt(20, "abcd", None, None)).unwrap();
        {
            let conn = s.connection();
            conn.execute("DELETE FROM stats_meta", []).unwrap();
            backfill(&conn, Path::new("")).unwrap();
        }
        let t = s.stats(0, 0).unwrap().totals;
        assert_eq!((t.count, t.chars), (2, 7));
    }
}
