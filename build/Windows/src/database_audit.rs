//! Optional value-safe SQLite audit logging for the native client.
//!
//! SQLite trace text may contain bound or literal values, so the audit file
//! records only the statement class and affected table. The UI/event log owns
//! user-visible messages; this file is a database-operation trail.

use rusqlite::{
    Connection,
    trace::{TraceEvent, TraceEventCodes},
};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
struct AuditState {
    enabled: bool,
    path: PathBuf,
}

static STATE: OnceLock<Mutex<AuditState>> = OnceLock::new();

fn state() -> &'static Mutex<AuditState> {
    STATE.get_or_init(|| Mutex::new(AuditState::default()))
}

pub fn configure(enabled: bool, log_directory: &Path) {
    let mut audit = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    audit.enabled = enabled;
    audit.path = log_directory.join("database-audit.log");
}

pub fn attach(connection: &mut Connection) {
    let enabled = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .enabled;
    if enabled {
        let _ = connection.trace_v2(
            TraceEventCodes::SQLITE_TRACE_STMT,
            Some(|event: TraceEvent<'_>| {
                if let TraceEvent::Stmt(statement, _) = event {
                    record_statement(statement.sql().as_ref());
                }
            }),
        );
    }
}

fn record_statement(sql: &str) {
    let Some((operation, table)) = classify_statement(sql) else {
        return;
    };
    let audit = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !audit.enabled || audit.path.as_os_str().is_empty() {
        return;
    }
    if let Some(parent) = audit.path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_secs());
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&audit.path)
    {
        let _ = writeln!(file, "{timestamp}\t{operation}\t{table}");
    }
}

fn classify_statement(sql: &str) -> Option<(String, String)> {
    let words = sql
        .split(|character: char| character.is_whitespace() || ",();`\"[]".contains(character))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let operation = words.first()?.to_ascii_uppercase();
    let table_index = match operation.as_str() {
        "INSERT" | "REPLACE" => words
            .iter()
            .position(|word| word.eq_ignore_ascii_case("into"))
            .map(|index| index + 1),
        "UPDATE" => Some(1),
        "DELETE" | "SELECT" => words
            .iter()
            .position(|word| word.eq_ignore_ascii_case("from"))
            .map(|index| index + 1),
        "CREATE" | "ALTER" | "DROP" => words
            .iter()
            .position(|word| word.eq_ignore_ascii_case("table"))
            .map(|index| index + 1),
        "PRAGMA" => Some(1),
        "BEGIN" | "COMMIT" | "ROLLBACK" => None,
        _ => return None,
    };
    let table = table_index
        .and_then(|index| words.get(index))
        .map(|value| {
            value.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '_'
            })
        })
        .filter(|value| !value.is_empty())
        .unwrap_or("transaction")
        .to_ascii_lowercase();
    Some((operation, table))
}

#[cfg(test)]
mod tests {
    use super::classify_statement;

    #[test]
    fn audit_classification_never_retains_values() {
        assert_eq!(
            classify_statement("INSERT INTO album(title) VALUES('secret')"),
            Some(("INSERT".to_string(), "album".to_string()))
        );
        assert_eq!(
            classify_statement("UPDATE track SET title='secret' WHERE id=2"),
            Some(("UPDATE".to_string(), "track".to_string()))
        );
    }
}
