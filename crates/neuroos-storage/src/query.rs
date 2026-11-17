//! Queries that do not need the embedder: focus history, activity, health signals, and the
//! interaction/nudge/profile tables (FR-STO-07/08/09; PRD §5.3). Hybrid search is in `search.rs`.

use std::sync::Arc;

use neuroos_proto::v1::{
    ActivityItem, ActivityResult, Domain, FocusHistoryResult, FocusWindow, HealthSignals,
    ProfileFact, QueryActivity, QueryFocusHistory, QueryHealthSignals, Taint,
};
use rusqlite::{Connection, params};

use crate::db::DbError;
use crate::ingest::adapters::focus_text;
use crate::ingest::apply::signed;
use crate::store::Store;

/// Most activity items one answer carries (a day of busy use stays well under this).
pub const ACTIVITY_MAX: usize = 500;

fn unsigned(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

fn window_row(r: &rusqlite::Row<'_>, contains: bool) -> rusqlite::Result<FocusWindow> {
    Ok(FocusWindow {
        app_id: r.get(0)?,
        title: r.get(1)?,
        start_ns: unsigned(r.get(2)?),
        end_ns: unsigned(r.get(3)?),
        contains_instant: contains,
    })
}

/// The window containing `at`, else the nearest edge within `window` (FR-STO-07).
fn find_focus(c: &Connection, at: i64, window: i64) -> Result<Option<FocusWindow>, DbError> {
    let inside = c
        .query_row(
            "SELECT app_id, title, start_ns, end_ns FROM focus_history
             WHERE start_ns <= ?1 AND end_ns >= ?1 ORDER BY start_ns DESC LIMIT 1",
            params![at],
            |r| window_row(r, true),
        )
        .ok();
    if inside.is_some() || window <= 0 {
        return Ok(inside);
    }
    let lo = at.saturating_sub(window);
    let hi = at.saturating_add(window);
    Ok(c.query_row(
        "SELECT app_id, title, start_ns, end_ns FROM focus_history
         WHERE (end_ns BETWEEN ?2 AND ?1 - 1) OR (start_ns BETWEEN ?1 + 1 AND ?3)
         ORDER BY MIN(ABS(start_ns - ?1), ABS(end_ns - ?1)) ASC, start_ns DESC LIMIT 1",
        params![at, lo, hi],
        |r| window_row(r, false),
    )
    .ok())
}

impl Store {
    /// `QueryFocusHistory` (FR-STO-07). An open window (not yet closed into history) counts.
    ///
    /// # Errors
    /// [`DbError`] on read failure.
    pub async fn focus_history(
        self: &Arc<Self>,
        q: QueryFocusHistory,
    ) -> Result<FocusHistoryResult, DbError> {
        let live = self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .window
            .clone();
        if let Some(w) = live
            && q.at_ns >= w.start_ns
        {
            return Ok(FocusHistoryResult {
                windows: vec![FocusWindow {
                    app_id: w.app_id,
                    title: w.title,
                    start_ns: w.start_ns,
                    end_ns: q.at_ns.max(w.start_ns),
                    contains_instant: true,
                }],
            });
        }
        let (at, window) = (signed(q.at_ns), signed(q.window_ns));
        let found = self.read(move |c| find_focus(c, at, window)).await?;
        Ok(FocusHistoryResult {
            windows: found.into_iter().collect(),
        })
    }

    /// `QueryActivity` (FR-STO-08): what happened in `[from, to]`, time-ordered, at most
    /// [`ACTIVITY_MAX`] items.
    ///
    /// # Errors
    /// [`DbError`] on read failure.
    pub async fn activity(self: &Arc<Self>, q: QueryActivity) -> Result<ActivityResult, DbError> {
        let (from, to) = (signed(q.from_ns), signed(q.to_ns));
        let open = self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .window
            .clone();
        let mut items = self
            .read(move |c| {
                let mut v: Vec<ActivityItem> = Vec::new();
                let mut s = c.prepare(
                    "SELECT app_id, title, start_ns, end_ns FROM focus_history
                     WHERE end_ns >= ?1 AND start_ns <= ?2 ORDER BY start_ns LIMIT ?3",
                )?;
                for row in s.query_map(params![from, to, ACTIVITY_MAX as i64], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                })? {
                    let (app, title, start, end) = row?;
                    v.push(ActivityItem {
                        domain: Domain::WindowFocus as i32,
                        label: focus_text(&app, &title),
                        start_ns: unsigned(start),
                        end_ns: unsigned(end),
                        taint: Some(Taint { flags: 0 }),
                    });
                }
                let mut s = c.prepare(
                    "SELECT domain, text, at_ns, taint FROM chunks
                     WHERE at_ns BETWEEN ?1 AND ?2 AND domain != ?4 ORDER BY at_ns LIMIT ?3",
                )?;
                for row in s.query_map(
                    params![from, to, ACTIVITY_MAX as i64, Domain::WindowFocus as i32],
                    |r| {
                        Ok((
                            r.get::<_, i32>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                        ))
                    },
                )? {
                    let (domain, text, at, taint) = row?;
                    v.push(ActivityItem {
                        domain,
                        label: text,
                        start_ns: unsigned(at),
                        end_ns: unsigned(at),
                        taint: Some(Taint {
                            flags: u32::try_from(taint).unwrap_or(0),
                        }),
                    });
                }
                Ok(v)
            })
            .await?;
        if let Some(w) = open
            && w.start_ns <= q.to_ns
        {
            items.push(ActivityItem {
                domain: Domain::WindowFocus as i32,
                label: focus_text(&w.app_id, &w.title),
                start_ns: w.start_ns,
                end_ns: q.to_ns.max(w.start_ns),
                taint: Some(Taint { flags: 0 }),
            });
        }
        items.sort_by_key(|i| (i.start_ns, i.end_ns));
        items.truncate(ACTIVITY_MAX);
        Ok(ActivityResult { items })
    }

    /// `QueryHealthSignals` (FR-STO-09): recent samples and the live flags.
    #[must_use]
    pub fn health_signals(&self, q: &QueryHealthSignals) -> HealthSignals {
        let live = self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        HealthSignals {
            samples: live.samples_since(q.since_ns),
            idle: live.idle,
            // Lock state is unknown in v1.0 (ADR-0011); `locked_known = false` says so.
            locked: false,
            fullscreen: live.fullscreen,
            media_playing: live.media_playing(),
            active_since_ns: live.active_since_ns,
            locked_known: false,
        }
    }

    /// Saves one interaction summary (domain `voice_interaction`, FR-NUD-11's neighbour in §7.3).
    ///
    /// # Errors
    /// [`DbError`] on write failure; [`DbError::Taint`] if the request carries no taint (R0-4).
    pub async fn store_interaction(
        self: &Arc<Self>,
        s: neuroos_proto::v1::StoreInteraction,
    ) -> Result<(), DbError> {
        let taint = taint_of(s.taint.as_ref())?;
        let text = format!("Asked: {} Answered: {}", s.question, s.answer_summary);
        let item = crate::ingest::Item {
            domain: Domain::VoiceInteraction,
            kind: "interaction".into(),
            label: format!("turn-{}", s.at_ns),
            app_id: String::new(),
            text,
            taint,
            at_ns: s.at_ns,
        };
        self.ingest(vec![crate::ingest::Output::Promote(item)])
            .await
            .map(|_| ())
    }

    /// Records a nudge outcome (`nudge_log`, FR-NUD-11).
    ///
    /// # Errors
    /// [`DbError`] on write failure.
    pub async fn store_nudge(
        self: &Arc<Self>,
        n: neuroos_proto::v1::StoreNudgeLog,
    ) -> Result<(), DbError> {
        self.db
            .writer()
            .exec(move |c| {
                c.execute(
                    "INSERT INTO nudge_log (rule_id, outcome, at_ns) VALUES (?1, ?2, ?3)",
                    params![n.rule_id, n.outcome, signed(n.at_ns)],
                )?;
                Ok(vec![])
            })
            .await
    }

    /// The nudge log rows the nudge engine needs to rebuild its state (ADR-0014): since
    /// `since_ns`, the rows that start a cooldown or a snooze (`SPOKEN`, `NOTIFIED`, `SNOOZED`),
    /// plus every switch row (`ENABLED`, `DISABLED`) whatever its age, oldest first. Held,
    /// dropped and answered rows are history for the owner, not state, so a log full of them can
    /// never push a cooldown out of the answer. Switches are settings, so retention and
    /// time-range forget keep them.
    ///
    /// # Errors
    /// [`DbError`] on read failure.
    pub async fn nudge_log_since(
        self: &Arc<Self>,
        since_ns: u64,
    ) -> Result<Vec<neuroos_proto::v1::NudgeLogEntry>, DbError> {
        use neuroos_proto::v1::NudgeOutcome as O;
        let timed = [O::Spoken as i64, O::Notified as i64, O::Snoozed as i64];
        let switches = [O::Enabled as i64, O::Disabled as i64];
        self.read(move |c| {
            let mut s = c.prepare(
                "SELECT rule_id, outcome, at_ns FROM nudge_log \
                 WHERE (at_ns >= ?1 AND outcome IN (?2, ?3, ?4)) OR outcome IN (?5, ?6) \
                 ORDER BY at_ns, id",
            )?;
            let rows = s
                .query_map(
                    params![
                        signed(since_ns),
                        timed[0],
                        timed[1],
                        timed[2],
                        switches[0],
                        switches[1]
                    ],
                    |r| {
                        Ok(neuroos_proto::v1::NudgeLogEntry {
                            rule_id: r.get(0)?,
                            outcome: i32::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
                            at_ns: unsigned(r.get(2)?),
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    /// Lists personal facts; a non-empty `relevant_to` keeps those sharing a word with it.
    ///
    /// # Errors
    /// [`DbError`] on read failure.
    pub async fn profile_list(
        self: &Arc<Self>,
        relevant_to: String,
    ) -> Result<Vec<ProfileFact>, DbError> {
        self.read(move |c| {
            let mut s =
                c.prepare("SELECT id, text, created_ns, taint FROM profile_facts ORDER BY id")?;
            let all = s
                .query_map([], |r| {
                    Ok(ProfileFact {
                        id: unsigned(r.get(0)?),
                        text: r.get(1)?,
                        saved_at_ns: unsigned(r.get(2)?),
                        taint: Some(Taint {
                            flags: u32::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                        }),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let words: Vec<String> = relevant_to
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|w| w.len() >= 3)
                .map(str::to_lowercase)
                .collect();
            if words.is_empty() {
                return Ok(all);
            }
            Ok(all
                .into_iter()
                .filter(|f| {
                    let t = f.text.to_lowercase();
                    words.iter().any(|w| t.contains(w.as_str()))
                })
                .collect())
        })
        .await
    }

    /// Adds a personal fact (the kernel only; R0-8).
    ///
    /// # Errors
    /// [`DbError::Taint`] without a taint; [`DbError`] on write failure.
    pub async fn profile_add(
        self: &Arc<Self>,
        a: neuroos_proto::v1::ProfileAdd,
        now_ns: u64,
    ) -> Result<u64, DbError> {
        let taint = taint_of(a.taint.as_ref())?;
        let id = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let id2 = Arc::clone(&id);
        self.db
            .writer()
            .exec(move |c| {
                c.execute(
                    "INSERT INTO profile_facts (text, created_ns, last_used_ns, taint) VALUES (?1, ?2, ?2, ?3)",
                    params![a.text, signed(now_ns), i64::from(taint.bits())],
                )?;
                id2.store(unsigned(c.last_insert_rowid()), std::sync::atomic::Ordering::Relaxed);
                Ok(vec![])
            })
            .await?;
        Ok(id.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Removes personal facts by id.
    ///
    /// # Errors
    /// [`DbError`] on write failure.
    pub async fn profile_remove(self: &Arc<Self>, ids: Vec<u64>) -> Result<u64, DbError> {
        let n = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let n2 = Arc::clone(&n);
        self.db
            .writer()
            .exec(move |c| {
                for id in ids {
                    let k = c.execute(
                        "DELETE FROM profile_facts WHERE id = ?1",
                        params![signed(id)],
                    )?;
                    n2.fetch_add(k as u64, std::sync::atomic::Ordering::Relaxed);
                }
                Ok(vec![])
            })
            .await?;
        Ok(n.load(std::sync::atomic::Ordering::Relaxed))
    }
}

/// A taint from the wire: present and without unknown bits, else an error (R0-4).
///
/// # Errors
/// [`DbError::Taint`].
pub fn taint_of(t: Option<&Taint>) -> Result<neuroos_taint::TaintFlags, DbError> {
    neuroos_taint::TaintFlags::decode(t.map(|t| t.flags)).map_err(|_| DbError::Taint)
}
