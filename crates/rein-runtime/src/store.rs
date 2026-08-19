//! The durable ledger (§11): SQLite in WAL mode, **append-only by trigger**.
//!
//! Receipts and events can be inserted and never updated or deleted — the
//! database itself raises on the attempt, so "append-only" is a property an
//! attacker (or a bug) has to fight SQLite over, not a convention (invariant
//! 22, decision C3 re-pointed here from M0's in-memory form).
//!
//! Entity tables (missions, epochs, plans, tasks, packs, attempts, runs) are
//! records, not the ledger; sealed things (packs, sealed epochs) refuse
//! replacement at the API layer.

use rein_core::context_pack::ContextPack;
use rein_core::entities::{Epoch, Mission, Plan, TaskVersion};
use rein_core::hand::SequencedEvent;
use rein_core::ids::{AttemptId, IdGen, ReceiptId, RunId, TaskRef};
use rein_core::receipts::{ReceiptBody, ReceiptEnvelope, ReceiptLog};
use rein_core::time::Timestamp;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{kind} `{key}` not found")]
    NotFound { kind: &'static str, key: String },
    #[error("{kind} `{key}` already exists and is sealed/immutable")]
    Immutable { kind: &'static str, key: String },
    #[error("timestamp: {0}")]
    Time(#[from] rein_core::time::TimeError),
    #[error("id parse: {0}")]
    Id(#[from] rein_core::ids::IdError),
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT NOT NULL);

CREATE TABLE IF NOT EXISTS receipts(
  seq        INTEGER PRIMARY KEY AUTOINCREMENT,
  receipt_id TEXT NOT NULL UNIQUE,
  attempt_id TEXT NOT NULL,
  at         TEXT NOT NULL,
  kind       TEXT NOT NULL,
  body       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS receipts_attempt ON receipts(attempt_id, seq);
CREATE TRIGGER IF NOT EXISTS receipts_append_only_u BEFORE UPDATE ON receipts
  BEGIN SELECT RAISE(ABORT, 'ledger is append-only'); END;
CREATE TRIGGER IF NOT EXISTS receipts_append_only_d BEFORE DELETE ON receipts
  BEGIN SELECT RAISE(ABORT, 'ledger is append-only'); END;

CREATE TABLE IF NOT EXISTS events(
  run_id TEXT NOT NULL,
  seq    INTEGER NOT NULL,
  at_ms  INTEGER NOT NULL,
  body   TEXT NOT NULL,
  PRIMARY KEY(run_id, seq)
);
CREATE TRIGGER IF NOT EXISTS events_append_only_u BEFORE UPDATE ON events
  BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS events_append_only_d BEFORE DELETE ON events
  BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;

CREATE TABLE IF NOT EXISTS packs(
  context_pack_id TEXT PRIMARY KEY,
  context_hash    TEXT NOT NULL,
  body            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS missions(
  mission_ref TEXT PRIMARY KEY,
  status      TEXT NOT NULL DEFAULT 'open',
  body        TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS epochs(
  epoch_ref TEXT PRIMARY KEY,
  sealed    INTEGER NOT NULL DEFAULT 0,
  body      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS plans(
  plan_ref TEXT PRIMARY KEY,
  body     TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks(
  task_ref TEXT PRIMARY KEY,
  plan_ref TEXT,
  body     TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS attempts(
  attempt_id      TEXT PRIMARY KEY,
  task_ref        TEXT NOT NULL,
  context_pack_id TEXT NOT NULL,
  context_hash    TEXT NOT NULL,
  generation      INTEGER NOT NULL,
  created_at      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS runs(
  run_id           TEXT PRIMARY KEY,
  attempt_id       TEXT NOT NULL,
  fence_generation INTEGER NOT NULL,
  hand_selector    TEXT NOT NULL,
  started_at       TEXT NOT NULL
);
"#;

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct AttemptRow {
    pub attempt_id: AttemptId,
    pub task_ref: TaskRef,
    pub context_pack_id: rein_core::ids::ContextPackId,
    pub context_hash: rein_core::canon::Sha256Digest,
    pub generation: u64,
    pub created_at: Timestamp,
}

impl Store {
    pub fn open(db_path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    pub fn raw(&self) -> &Connection {
        &self.conn
    }

    // ---- id high-water mark ------------------------------------------------

    /// An [`IdGen`] resumed from the persisted high-water mark: ids never
    /// collide across sessions, and never depend on ambient randomness.
    pub fn id_gen(&self) -> Result<IdGen, StoreError> {
        let issued: Option<String> = self
            .conn
            .query_row("SELECT v FROM meta WHERE k='ids_issued'", [], |r| r.get(0))
            .optional()?;
        Ok(IdGen::starting_at(
            issued.and_then(|v| v.parse().ok()).unwrap_or(0),
        ))
    }

    pub fn save_id_gen(&mut self, ids: &IdGen) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO meta(k,v) VALUES('ids_issued',?1)
             ON CONFLICT(k) DO UPDATE SET v=?1",
            params![ids.issued().to_string()],
        )?;
        Ok(())
    }

    // ---- receipts ----------------------------------------------------------

    fn receipt_kind(body: &ReceiptBody) -> Result<String, StoreError> {
        let v = serde_json::to_value(body)?;
        Ok(v.get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("unknown")
            .to_string())
    }

    /// Persist the tail of an in-memory log (everything from index `from`).
    /// The engine pattern: load → operate with the M0 pure functions →
    /// persist the new tail.
    pub fn persist_receipts_from(
        &mut self,
        log: &ReceiptLog,
        from: usize,
    ) -> Result<usize, StoreError> {
        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO receipts(receipt_id, attempt_id, at, kind, body)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
            )?;
            for e in log.iter().skip(from) {
                stmt.execute(params![
                    e.receipt_id.as_str(),
                    e.attempt_id.as_str(),
                    e.at.canonical(),
                    Self::receipt_kind(&e.body)?,
                    serde_json::to_string(&e.body)?,
                ])?;
                n += 1;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    fn row_to_envelope(row: &rusqlite::Row<'_>) -> Result<ReceiptEnvelope, StoreError> {
        let receipt_id: String = row.get(0)?;
        let attempt_id: String = row.get(1)?;
        let at: String = row.get(2)?;
        let body: String = row.get(3)?;
        Ok(ReceiptEnvelope {
            receipt_id: ReceiptId::parse(&receipt_id)?,
            attempt_id: AttemptId::parse(&attempt_id)?,
            at: Timestamp::parse(&at)?,
            body: serde_json::from_str(&body)?,
        })
    }

    /// The full ledger in append order (admission scans, selection).
    pub fn load_full_log(&self) -> Result<ReceiptLog, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT receipt_id, attempt_id, at, body FROM receipts ORDER BY seq")?;
        let mut rows = stmt.query([])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push(Self::row_to_envelope(row)?);
        }
        Ok(ReceiptLog::from_envelopes(entries))
    }

    pub fn load_attempt_log(&self, attempt_id: &AttemptId) -> Result<ReceiptLog, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT receipt_id, attempt_id, at, body FROM receipts WHERE attempt_id=?1 ORDER BY seq",
        )?;
        let mut rows = stmt.query(params![attempt_id.as_str()])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push(Self::row_to_envelope(row)?);
        }
        Ok(ReceiptLog::from_envelopes(entries))
    }

    pub fn receipt_count(&self) -> Result<u64, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))?)
    }

    // ---- events ------------------------------------------------------------

    pub fn persist_events(&mut self, events: &[SequencedEvent]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO events(run_id, seq, at_ms, body) VALUES(?1, ?2, ?3, ?4)")?;
            for ev in events {
                stmt.execute(params![
                    ev.run_id.as_str(),
                    ev.seq as i64,
                    ev.at.0 as i64,
                    serde_json::to_string(ev)?,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_events(&self, run_id: &RunId) -> Result<Vec<SequencedEvent>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT body FROM events WHERE run_id=?1 ORDER BY seq")?;
        let mut rows = stmt.query(params![run_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let body: String = row.get(0)?;
            out.push(serde_json::from_str(&body)?);
        }
        Ok(out)
    }

    pub fn load_events_for_attempt(
        &self,
        attempt_id: &AttemptId,
    ) -> Result<Vec<SequencedEvent>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT e.body FROM events e JOIN runs r ON e.run_id = r.run_id
             WHERE r.attempt_id = ?1 ORDER BY r.started_at, e.run_id, e.seq",
        )?;
        let mut rows = stmt.query(params![attempt_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let body: String = row.get(0)?;
            out.push(serde_json::from_str(&body)?);
        }
        Ok(out)
    }

    // ---- entities ----------------------------------------------------------

    pub fn put_pack(&mut self, pack: &ContextPack) -> Result<(), StoreError> {
        let hash = pack
            .context_hash
            .as_ref()
            .map(|h| h.as_str().to_string())
            .unwrap_or_default();
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT context_hash FROM packs WHERE context_pack_id=?1",
                params![pack.context_pack_id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = existing {
            // Packs are frozen: replacing one under the same id is refused.
            if prior != hash {
                return Err(StoreError::Immutable {
                    kind: "context pack",
                    key: pack.context_pack_id.as_str().to_string(),
                });
            }
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO packs(context_pack_id, context_hash, body) VALUES(?1,?2,?3)",
            params![
                pack.context_pack_id.as_str(),
                hash,
                serde_json::to_string(pack)?
            ],
        )?;
        Ok(())
    }

    pub fn get_pack(&self, id: &rein_core::ids::ContextPackId) -> Result<ContextPack, StoreError> {
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM packs WHERE context_pack_id=?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        let body = body.ok_or_else(|| StoreError::NotFound {
            kind: "context pack",
            key: id.as_str().to_string(),
        })?;
        Ok(serde_json::from_str(&body)?)
    }

    pub fn put_mission(&mut self, m: &Mission) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO missions(mission_ref, status, body) VALUES(?1,'open',?2)
             ON CONFLICT(mission_ref) DO UPDATE SET body=excluded.body",
            params![m.mission_ref.as_str(), serde_json::to_string(m)?],
        )?;
        Ok(())
    }

    pub fn set_mission_status(&mut self, r: &str, status: &str) -> Result<(), StoreError> {
        let n = self.conn.execute(
            "UPDATE missions SET status=?2 WHERE mission_ref=?1",
            params![r, status],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound {
                kind: "mission",
                key: r.to_string(),
            });
        }
        Ok(())
    }

    pub fn list_missions(&self) -> Result<Vec<(Mission, String)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT body, status FROM missions ORDER BY mission_ref")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let body: String = row.get(0)?;
            let status: String = row.get(1)?;
            out.push((serde_json::from_str(&body)?, status));
        }
        Ok(out)
    }

    pub fn put_epoch(&mut self, e: &Epoch) -> Result<(), StoreError> {
        let sealed: Option<bool> = self
            .conn
            .query_row(
                "SELECT sealed FROM epochs WHERE epoch_ref=?1",
                params![e.epoch_ref.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if sealed == Some(true) {
            return Err(StoreError::Immutable {
                kind: "epoch",
                key: e.epoch_ref.as_str().to_string(),
            });
        }
        self.conn.execute(
            "INSERT INTO epochs(epoch_ref, sealed, body) VALUES(?1,?2,?3)
             ON CONFLICT(epoch_ref) DO UPDATE SET sealed=excluded.sealed, body=excluded.body",
            params![e.epoch_ref.as_str(), e.sealed, serde_json::to_string(e)?],
        )?;
        Ok(())
    }

    pub fn get_epoch(&self, r: &str) -> Result<(Epoch, bool), StoreError> {
        let row: Option<(String, bool)> = self
            .conn
            .query_row(
                "SELECT body, sealed FROM epochs WHERE epoch_ref=?1",
                params![r],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (body, sealed) = row.ok_or_else(|| StoreError::NotFound {
            kind: "epoch",
            key: r.to_string(),
        })?;
        Ok((serde_json::from_str(&body)?, sealed))
    }

    pub fn list_epochs(&self) -> Result<Vec<(Epoch, bool)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT body, sealed FROM epochs ORDER BY epoch_ref")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let body: String = row.get(0)?;
            let sealed: bool = row.get(1)?;
            out.push((serde_json::from_str(&body)?, sealed));
        }
        Ok(out)
    }

    pub fn put_plan(&mut self, p: &Plan) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO plans(plan_ref, body) VALUES(?1,?2)
             ON CONFLICT(plan_ref) DO UPDATE SET body=excluded.body",
            params![p.plan_ref.as_str(), serde_json::to_string(p)?],
        )?;
        Ok(())
    }

    pub fn get_plan(&self, r: &str) -> Result<Plan, StoreError> {
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM plans WHERE plan_ref=?1",
                params![r],
                |row| row.get(0),
            )
            .optional()?;
        let body = body.ok_or_else(|| StoreError::NotFound {
            kind: "plan",
            key: r.to_string(),
        })?;
        Ok(serde_json::from_str(&body)?)
    }

    pub fn put_task(&mut self, t: &TaskVersion) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO tasks(task_ref, plan_ref, body) VALUES(?1,?2,?3)
             ON CONFLICT(task_ref) DO UPDATE SET plan_ref=excluded.plan_ref, body=excluded.body",
            params![
                t.task_ref.as_str(),
                t.plan_ref.as_str(),
                serde_json::to_string(t)?
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, r: &TaskRef) -> Result<TaskVersion, StoreError> {
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM tasks WHERE task_ref=?1",
                params![r.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let body = body.ok_or_else(|| StoreError::NotFound {
            kind: "task",
            key: r.as_str().to_string(),
        })?;
        Ok(serde_json::from_str(&body)?)
    }

    pub fn list_tasks(&self) -> Result<Vec<TaskVersion>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT body FROM tasks ORDER BY task_ref")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let body: String = row.get(0)?;
            out.push(serde_json::from_str(&body)?);
        }
        Ok(out)
    }

    pub fn insert_attempt(&mut self, a: &rein_core::entities::Attempt) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO attempts(attempt_id, task_ref, context_pack_id, context_hash, generation, created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                a.attempt_id.as_str(),
                a.task_ref.as_str(),
                a.context_pack_id.as_str(),
                a.context_hash.as_str(),
                a.generation as i64,
                a.created_at.canonical(),
            ],
        )?;
        Ok(())
    }

    pub fn get_attempt(&self, id: &AttemptId) -> Result<AttemptRow, StoreError> {
        let row: Option<AttemptRow> = self
            .conn
            .query_row(
                "SELECT attempt_id, task_ref, context_pack_id, context_hash, generation, created_at
                 FROM attempts WHERE attempt_id=?1",
                params![id.as_str()],
                |row| {
                    let attempt_id: String = row.get(0)?;
                    let task_ref: String = row.get(1)?;
                    let pack: String = row.get(2)?;
                    let hash: String = row.get(3)?;
                    let generation: i64 = row.get(4)?;
                    let created_at: String = row.get(5)?;
                    Ok((attempt_id, task_ref, pack, hash, generation, created_at))
                },
            )
            .optional()?
            .map(
                |(attempt_id, task_ref, pack, hash, generation, created_at)| {
                    Ok::<_, StoreError>(AttemptRow {
                        attempt_id: AttemptId::parse(&attempt_id)?,
                        task_ref: TaskRef::parse(&task_ref)?,
                        context_pack_id: rein_core::ids::ContextPackId::parse(&pack)?,
                        context_hash: rein_core::canon::Sha256Digest::parse(&hash).map_err(
                            |_| StoreError::NotFound {
                                kind: "attempt digest",
                                key: hash.clone(),
                            },
                        )?,
                        generation: generation as u64,
                        created_at: Timestamp::parse(&created_at)?,
                    })
                },
            )
            .transpose()?;
        row.ok_or_else(|| StoreError::NotFound {
            kind: "attempt",
            key: id.as_str().to_string(),
        })
    }

    pub fn attempts_for_task(&self, task: &TaskRef) -> Result<Vec<AttemptId>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT attempt_id FROM attempts WHERE task_ref=?1 ORDER BY attempt_id")?;
        let mut rows = stmt.query(params![task.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            out.push(AttemptId::parse(&id)?);
        }
        Ok(out)
    }

    pub fn list_attempts(&self) -> Result<Vec<AttemptRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT attempt_id FROM attempts ORDER BY attempt_id")?;
        let mut rows = stmt.query([])?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            ids.push(AttemptId::parse(&id)?);
        }
        ids.into_iter().map(|id| self.get_attempt(&id)).collect()
    }

    pub fn insert_run(
        &mut self,
        run_id: &RunId,
        attempt_id: &AttemptId,
        fence_generation: u64,
        hand_selector: &str,
        started_at: Timestamp,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO runs(run_id, attempt_id, fence_generation, hand_selector, started_at)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                run_id.as_str(),
                attempt_id.as_str(),
                fence_generation as i64,
                hand_selector,
                started_at.canonical()
            ],
        )?;
        Ok(())
    }

    pub fn runs_for_attempt(
        &self,
        attempt_id: &AttemptId,
    ) -> Result<Vec<(RunId, String)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT run_id, hand_selector FROM runs WHERE attempt_id=?1 ORDER BY started_at, run_id",
        )?;
        let mut rows = stmt.query(params![attempt_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let hand: String = row.get(1)?;
            out.push((RunId::parse(&id)?, hand));
        }
        Ok(out)
    }

    /// Doctor: integrity + the append-only triggers must exist.
    pub fn doctor(&self) -> Result<Vec<String>, StoreError> {
        let mut notes = Vec::new();
        let integrity: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        notes.push(format!("sqlite integrity: {integrity}"));
        let triggers: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name LIKE '%append_only%'",
            [],
            |r| r.get(0),
        )?;
        notes.push(format!("append-only triggers: {triggers}/4"));
        if triggers != 4 {
            notes.push("FAIL: ledger append-only triggers missing".to_string());
        }
        Ok(notes)
    }
}
