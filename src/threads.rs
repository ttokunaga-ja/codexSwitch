//! Read-only lookups in a Codex home's thread database.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Thread {
    pub id: String,
    /// Explicit thread name; empty for untitled threads.
    pub name: String,
    pub preview: String,
    pub title: String,
    pub cwd: PathBuf,
    pub model: String,
    pub provider: String,
    pub rollout: PathBuf,
    pub updated_ms: i64,
}

impl Thread {
    /// One-line label for listings. Untitled threads fall back to the first
    /// line of their opening message.
    pub fn label(&self) -> String {
        [&self.name, &self.preview, &self.title]
            .into_iter()
            .find_map(|s| s.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(|l| truncate(l, 60))
            .unwrap_or_else(|| self.id.clone())
    }
}

/// Finds the newest `state_<N>.sqlite`, so a schema bump does not break lookups.
pub fn state_db(home: &Path) -> Result<PathBuf> {
    let mut best: Option<(u32, PathBuf)> = None;
    let entries = std::fs::read_dir(home)
        .with_context(|| format!("Codex のホームを読めません: {}", home.display()))?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let n = name
            .strip_prefix("state_")
            .and_then(|s| s.strip_suffix(".sqlite"))
            .and_then(|s| s.parse::<u32>().ok());
        if let Some(n) = n
            && best.as_ref().is_none_or(|(b, _)| n > *b)
        {
            best = Some((n, entry.path()));
        }
    }
    best.map(|(_, p)| p)
        .with_context(|| format!("state_*.sqlite が見つかりません: {}", home.display()))
}

pub fn open(home: &Path) -> Result<Connection> {
    let db = state_db(home)?;
    Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("DB を開けません: {}", db.display()))
}

const COLS: &str = "id, coalesce(name, ''), coalesce(preview, ''), coalesce(title, ''), cwd, \
                    coalesce(model, ''), coalesce(model_provider, ''), rollout_path, \
                    coalesce(updated_at_ms, updated_at * 1000, 0)";

fn row(r: &Row<'_>) -> rusqlite::Result<Thread> {
    Ok(Thread {
        id: r.get(0)?,
        name: r.get(1)?,
        preview: r.get(2)?,
        title: r.get(3)?,
        cwd: PathBuf::from(r.get::<_, String>(4)?),
        model: r.get(5)?,
        provider: r.get(6)?,
        rollout: PathBuf::from(r.get::<_, String>(7)?),
        updated_ms: r.get(8)?,
    })
}

pub fn by_id(conn: &Connection, id: &str) -> Result<Option<Thread>> {
    let sql = format!("select {COLS} from threads where id = ?1");
    Ok(conn.query_row(&sql, params![id], row).optional()?)
}

/// Resolves a thread id or a title fragment. Thread names are searched first;
/// only if none match does the search widen to the opening message. Sub-agent
/// threads (whose `source` is a JSON object) and archived threads are skipped.
pub fn search(conn: &Connection, query: &str) -> Result<Vec<Thread>> {
    if is_uuid(query) {
        return Ok(by_id(conn, query)?.into_iter().collect());
    }
    let pattern = format!("%{}%", escape_like(query));
    let by_name = matching(conn, "name like ?1 escape '\\'", &pattern)?;
    if !by_name.is_empty() {
        return Ok(by_name);
    }
    matching(
        conn,
        "preview like ?1 escape '\\' or title like ?1 escape '\\'",
        &pattern,
    )
}

fn matching(conn: &Connection, condition: &str, pattern: &str) -> Result<Vec<Thread>> {
    let sql = format!(
        "select {COLS} from threads \
         where archived = 0 and source not like '{{%' and ({condition}) \
         order by updated_at_ms desc limit 20"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![pattern], row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n)
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_uuids() {
        assert!(is_uuid("0199aaaa-bbbb-7ccc-8ddd-eeeeffff0000"));
        assert!(!is_uuid("ログイン画面"));
        assert!(!is_uuid("0199aaaa-bbbb-7ccc-8ddd"));
    }

    #[test]
    fn escapes_like_wildcards() {
        assert_eq!(escape_like("50%_off\\"), "50\\%\\_off\\\\");
    }
}
