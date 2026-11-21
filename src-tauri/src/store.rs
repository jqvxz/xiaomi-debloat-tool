use crate::operations::{Action, Preset, Value};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pending,
    Success,
    Failed,
    Unknown,
    Skipped,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Success => "Success",
            Self::Failed => "Failed",
            Self::Unknown => "Unknown",
            Self::Skipped => "Skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    #[serde(default, skip_deserializing)]
    pub id: i64,
    pub at: String,
    pub device: String,
    pub user: u32,
    pub label: String,
    pub action: Option<Action>,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub status: Status,
    pub detail: Option<String>,
    pub undo_of: Option<i64>,
    #[serde(default, skip_deserializing)]
    pub can_undo: bool,
}

impl Entry {
    pub fn new(
        device: &str,
        user: u32,
        label: String,
        action: Option<Action>,
        before: Option<Value>,
    ) -> Self {
        Self {
            id: 0,
            at: Utc::now().to_rfc3339(),
            device: device.into(),
            user,
            label,
            action,
            before,
            after: None,
            status: Status::Pending,
            detail: None,
            undo_of: None,
            can_undo: false,
        }
    }
}

pub struct Store(Mutex<Connection>);

fn db_error(error: rusqlite::Error) -> String {
    format!("Could not access the local activity database. {error}")
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(dir.join("activity.sqlite")).map_err(db_error)?;
        conn.execute_batch(
            "PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS journal (
               id INTEGER PRIMARY KEY, identity TEXT NOT NULL, entry TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS journal_identity ON journal (identity, id);
             CREATE TABLE IF NOT EXISTS preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS presets (name TEXT PRIMARY KEY COLLATE NOCASE, value TEXT NOT NULL);",
        ).map_err(db_error)?;
        let store = Self(Mutex::new(conn));
        // A pending entry at startup means the app exited between the write and read-back.
        for (_, mut entry) in store.entries(None)? {
            if entry.status == Status::Pending {
                entry.status = Status::Unknown;
                entry.detail = Some("The app closed before it verified this change.".into());
                store.update(&entry)?;
            }
        }
        Ok(store)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.0
            .lock()
            .map_err(|_| "The local database is unavailable.".into())
    }

    pub fn preference(&self, key: &str) -> Result<Option<String>> {
        self.lock()?
            .query_row("SELECT value FROM preferences WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_error)
    }

    pub fn save_preference(&self, key: &str, value: &str) -> Result<()> {
        self.lock()?
            .execute(
                "INSERT OR REPLACE INTO preferences (key, value) VALUES (?1, ?2)",
                [key, value],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn add(&self, identity: &str, mut entry: Entry) -> Result<Entry> {
        let serialized = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO journal (identity, entry) VALUES (?1, ?2)",
            params![identity, serialized],
        )
        .map_err(db_error)?;
        entry.id = conn.last_insert_rowid();
        Ok(entry)
    }

    pub fn update(&self, entry: &Entry) -> Result<()> {
        let serialized = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        self.lock()?
            .execute(
                "UPDATE journal SET entry=?1 WHERE id=?2",
                params![serialized, entry.id],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn entries(&self, identity: Option<&str>) -> Result<Vec<(String, Entry)>> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(
            "SELECT id, identity, entry FROM journal WHERE (?1 IS NULL OR identity=?1) ORDER BY id DESC",
        ).map_err(db_error)?;
        let rows = statement
            .query_map([identity], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (id, identity, raw) = row.map_err(db_error)?;
            let mut entry: Entry = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            entry.id = id;
            Ok((identity, entry))
        })
        .collect()
    }

    pub fn history(&self, identity: Option<&str>, limit: Option<i64>) -> Result<Vec<Entry>> {
        let entries = self.entries(identity)?;
        let undone: std::collections::HashSet<_> = entries
            .iter()
            .filter(|(_, entry)| entry.status == Status::Success)
            .filter_map(|(_, entry)| entry.undo_of)
            .collect();
        let mut latest = std::collections::HashSet::new();
        Ok(entries
            .into_iter()
            .map(|(device, mut entry)| {
                if !undone.contains(&entry.id)
                    && (entry.status != Status::Success || entry.undo_of.is_none())
                    && matches!(
                        entry.status,
                        Status::Success | Status::Unknown | Status::Pending
                    )
                {
                    if let Some(action) = &entry.action {
                        let first = latest.insert((device.clone(), action.id().to_owned()));
                        entry.can_undo = first
                            && entry.status == Status::Success
                            && entry.undo_of.is_none()
                            && entry.before.is_some()
                            && !matches!(entry.after, None | Some(Value::Package(None)));
                    }
                }
                entry
            })
            .take(limit.unwrap_or(i64::MAX) as usize)
            .collect())
    }

    pub fn presets(&self) -> Result<Vec<Preset>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT value FROM presets ORDER BY name")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        rows.map(|row| serde_json::from_str(&row.map_err(db_error)?).map_err(|e| e.to_string()))
            .collect()
    }

    pub fn save_preset(&self, preset: &Preset) -> Result<()> {
        let value = serde_json::to_string(preset).map_err(|e| e.to_string())?;
        let added = self
            .lock()?
            .execute(
                "INSERT OR IGNORE INTO presets (name, value) VALUES (?1, ?2)",
                params![preset.name, value],
            )
            .map_err(db_error)?;
        if added == 0 {
            return Err("A saved preset already has that name.".into());
        }
        Ok(())
    }

    pub fn delete_preset(&self, name: &str) -> Result<()> {
        self.lock()?
            .execute("DELETE FROM presets WHERE name=?1", [name])
            .map_err(db_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_writes_durably_and_recovers_interrupted_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let action = Action::Setting {
            id: "window_animation_scale".into(),
            value: "0.5".into(),
        };
        let entry = store
            .add(
                "device",
                Entry::new(
                    "Test phone",
                    0,
                    action.label(),
                    Some(action),
                    Some(Value::Setting(None)),
                ),
            )
            .unwrap();
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let history = store.history(Some("device"), None).unwrap();
        assert_eq!(history[0].id, entry.id);
        assert_eq!(history[0].status, Status::Unknown);
        assert_eq!(history[0].before, Some(Value::Setting(None)));
        assert!(
            store
                .history(Some("another phone"), None)
                .unwrap()
                .is_empty()
        );
    }
}
