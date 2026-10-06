//! Durable delivery shared by the composer and the chief of staff. Provider execution stays in Engine.
use crate::{
    skill::DocBlock,
    store::{Store, StoreError},
};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub(crate) fn install(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prompt_deliveries (
      sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE,
      session_id TEXT NOT NULL, mode TEXT NOT NULL, doc TEXT NOT NULL,
      state TEXT NOT NULL, outcome TEXT NOT NULL DEFAULT '',
      goal_id TEXT, contract_revision INTEGER, assignment_id TEXT, expected_turn TEXT,
      FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE);
      CREATE INDEX IF NOT EXISTS prompt_deliveries_pending ON prompt_deliveries(state, sequence);",
    )?;
    let has_assignment = conn
        .prepare("PRAGMA table_info(prompt_deliveries)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|c| c == "assignment_id");
    if !has_assignment {
        conn.execute_batch("ALTER TABLE prompt_deliveries ADD COLUMN assignment_id TEXT;")?;
    }
    let columns = conn
        .prepare("PRAGMA table_info(prompt_deliveries)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|c| c == "expected_turn") {
        conn.execute_batch("ALTER TABLE prompt_deliveries ADD COLUMN expected_turn TEXT;")?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDelivery {
    pub id: String,
    pub session_id: String,
    pub mode: String,
    pub doc: Vec<DocBlock>,
    pub state: String,
    pub outcome: String,
    pub goal_id: Option<String>,
    pub contract_revision: Option<u64>,
    pub assignment_id: Option<String>,
    pub expected_turn: Option<String>,
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<PromptDelivery> {
    let doc: String = row.get(3)?;
    Ok(PromptDelivery {
        id: row.get(0)?,
        session_id: row.get(1)?,
        mode: row.get(2)?,
        doc: serde_json::from_str(&doc).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?,
        state: row.get(4)?,
        outcome: row.get(5)?,
        goal_id: row.get(6)?,
        contract_revision: row.get(7)?,
        assignment_id: row.get(8)?,
        expected_turn: row.get(9)?,
    })
}
const COLUMNS: &str =
    "id,session_id,mode,doc,state,outcome,goal_id,contract_revision,assignment_id,expected_turn";

impl Store {
    /// Acceptance is durable history, not continuing authorization to send a delayed prompt.
    pub(crate) fn require_delivery_send(&self, id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let delivery = conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM prompt_deliveries WHERE id=?1"),
                [id],
                decode,
            )
            .optional()?;
        if let Some(d) = delivery {
            if !matches!(d.state.as_str(), "submitting" | "accepted") || !authorized_on(&conn, &d)?
            {
                return Err(StoreError::InvalidAssistant(
                    "delivery authority changed before Provider send".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn enqueue_delivery(
        &self,
        delivery: &PromptDelivery,
    ) -> Result<PromptDelivery, StoreError> {
        if delivery.id.trim().is_empty()
            || delivery.id.len() > 512
            || !matches!(delivery.mode.as_str(), "queue" | "steer")
            || crate::skill::canonical_doc_text(&delivery.doc)
                .trim()
                .is_empty()
        {
            return Err(StoreError::InvalidAssistant(
                "invalid prompt delivery".into(),
            ));
        }
        let conn = self.conn.lock().unwrap();
        let existing = conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM prompt_deliveries WHERE id=?1"),
                [&delivery.id],
                decode,
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing.session_id != delivery.session_id
                || existing.mode != delivery.mode
                || serde_json::to_string(&existing.doc)? != serde_json::to_string(&delivery.doc)?
                || existing.goal_id != delivery.goal_id
                || existing.contract_revision != delivery.contract_revision
                || existing.assignment_id != delivery.assignment_id
                || existing.expected_turn != delivery.expected_turn
            {
                return Err(StoreError::InvalidAssistant(
                    "delivery id already belongs to different content".into(),
                ));
            }
            return Ok(existing);
        }
        conn.execute("INSERT INTO prompt_deliveries(id,session_id,mode,doc,state,goal_id,contract_revision,assignment_id,expected_turn) VALUES(?1,?2,?3,?4,'queued',?5,?6,?7,?8)",
            rusqlite::params![delivery.id,delivery.session_id,delivery.mode,serde_json::to_string(&delivery.doc)?,delivery.goal_id,delivery.contract_revision,delivery.assignment_id,delivery.expected_turn])?;
        let mut result = delivery.clone();
        result.state = "queued".into();
        result.outcome.clear();
        Ok(result)
    }

    pub fn prompt_delivery(&self, id: &str) -> Result<Option<PromptDelivery>, StoreError> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                &format!("SELECT {COLUMNS} FROM prompt_deliveries WHERE id=?1"),
                [id],
                decode,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn pending_deliveries(&self) -> Result<Vec<PromptDelivery>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM prompt_deliveries WHERE state IN ('queued','submitting') AND sequence IN (SELECT MIN(sequence) FROM prompt_deliveries WHERE state IN ('queued','submitting') GROUP BY session_id,mode) ORDER BY sequence"))?;
        let result = stmt
            .query_map([], decode)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(result)
    }

    pub fn claim_delivery(&self, id: &str) -> Result<bool, StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let delivery = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM prompt_deliveries WHERE id=?1 AND state='queued'"),
                [id],
                decode,
            )
            .optional()?;
        let Some(delivery) = delivery else {
            return Ok(false);
        };
        if !authorized_on(&tx, &delivery)? {
            tx.execute("UPDATE prompt_deliveries SET state='cancelled',outcome='Goal version, assignment or authority changed' WHERE id=?1",[id])?;
            tx.commit()?;
            return Ok(false);
        }
        tx.execute(
            "UPDATE prompt_deliveries SET state='submitting' WHERE id=?1",
            [id],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn delivery_send_authorized(&self, id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().unwrap();
        let d = conn.query_row(
            &format!("SELECT {COLUMNS} FROM prompt_deliveries WHERE id=?1"),
            [id],
            decode,
        )?;
        Ok(d.state == "submitting" && authorized_on(&conn, &d)?)
    }

    pub fn finish_delivery(&self, id: &str, state: &str, outcome: &str) -> Result<(), StoreError> {
        if !matches!(state, "accepted" | "failed" | "unknown" | "cancelled") {
            return Err(StoreError::InvalidAssistant(
                "invalid delivery result".into(),
            ));
        }
        self.conn.lock().unwrap().execute("UPDATE prompt_deliveries SET state=?2,outcome=?3 WHERE id=?1 AND state IN ('queued','submitting')", rusqlite::params![id,state,outcome])?;
        Ok(())
    }

    pub fn cancel_session_deliveries(&self, session: &str) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute("UPDATE prompt_deliveries SET state='cancelled',outcome='Session control changed' WHERE session_id=?1 AND state='queued'", [session])?;
        Ok(())
    }

    pub fn delivery_position(&self, id: &str) -> Result<usize, StoreError> {
        Ok(self.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM prompt_deliveries WHERE state='queued' AND session_id=(SELECT session_id FROM prompt_deliveries WHERE id=?1) AND sequence<=(SELECT sequence FROM prompt_deliveries WHERE id=?1)", [id], |row| row.get(0))?)
    }
}

/// Used within the queue claim and prompt acceptance transactions. No authority snapshot can
/// race with a saved requirement/control change between validation and its durable effect.
pub(crate) fn authorized_on(
    conn: &Connection,
    delivery: &PromptDelivery,
) -> Result<bool, StoreError> {
    if delivery.mode == "steer" {
        let activity: Option<Option<String>> = conn
            .query_row(
                "SELECT activity_json FROM sessions WHERE id=?1",
                [&delivery.session_id],
                |r| r.get(0),
            )
            .optional()?;
        let activity: crate::session::SessionActivity = activity
            .flatten()
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_default();
        let current = match &activity.state {
            crate::session::SessionRunState::Running { turn_id, .. }
            | crate::session::SessionRunState::AwaitingInput { turn_id, .. } => {
                Some(turn_id.as_str())
            }
            _ => None,
        };
        if current.is_none() || current != delivery.expected_turn.as_deref() {
            return Ok(false);
        }
    }
    if delivery.goal_id.is_none() {
        return Ok(true);
    };
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM assistant_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let state: crate::assistant::AssistantState = body
        .map(|b| serde_json::from_str(&b))
        .transpose()?
        .unwrap_or_default();
    Ok(authorized(&state, delivery))
}
pub(crate) fn authorized(
    state: &crate::assistant::AssistantState,
    delivery: &PromptDelivery,
) -> bool {
    let Some(goal_id) = &delivery.goal_id else {
        return true;
    };
    state
        .goals
        .iter()
        .find(|g| &g.id == goal_id)
        .is_some_and(|g| {
            state
                .settings
                .as_ref()
                .is_some_and(|s| s.enabled && s.projects.contains(&g.project_path))
                && delivery.contract_revision == Some(g.contract_revision)
                && g.status == crate::assistant::GoalStatus::Active
                && g.assignments.last().is_some_and(|a| {
                    delivery.assignment_id.as_deref() == Some(&a.id)
                        && a.session_id.as_deref() == Some(&delivery.session_id)
                        && a.owned
                        && !a.taken_over
                        && !a.stop_requested
                })
        })
}
pub(crate) fn cancel_obsolete_on(
    conn: &Connection,
    state: &crate::assistant::AssistantState,
) -> Result<(), StoreError> {
    let deliveries = {
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM prompt_deliveries WHERE state='queued' AND goal_id IS NOT NULL"
        ))?;
        let rows = stmt
            .query_map([], decode)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for d in deliveries {
        if !authorized(state, &d) {
            conn.execute("UPDATE prompt_deliveries SET state='cancelled',outcome='Goal version, assignment or authority changed' WHERE id=?1",[d.id])?;
        }
    }
    Ok(())
}
pub(crate) fn require_delivery_on(conn: &Connection, id: &str) -> Result<(), StoreError> {
    let d = conn.query_row(
        &format!("SELECT {COLUMNS} FROM prompt_deliveries WHERE id=?1"),
        [id],
        decode,
    )?;
    if d.state != "submitting" || !authorized_on(conn, &d)? {
        return Err(StoreError::InvalidAssistant(
            "delivery authority changed before Engine acceptance".into(),
        ));
    }
    Ok(())
}
