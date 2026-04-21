use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 1;

/// Row returned for staleness checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocStat {
    pub doc_id: String,
    pub rel_path: String,
    pub mtime_unix: i64,
    pub size_bytes: i64,
}

/// Full row written during sync.
#[derive(Debug, Clone)]
pub struct DocRow {
    pub doc_id: String,
    pub rel_path: String,
    pub dir: String,
    pub title: String,
    pub tokens: i64,
    pub mtime_unix: i64,
    pub size_bytes: i64,
    pub is_markdown: bool,
}

#[derive(Debug, Clone)]
pub struct SectionRow {
    pub doc_id: String,
    pub section_id: String,
    pub seq: i64,
    pub level: i64,
    pub heading: String,
    pub heading_path: String,
    pub snippet: String,
    pub tokens: i64,
    pub code_tokens: i64,
    pub content: Vec<u8>,
}

/// Open (or create) the per-repo sqlite DB at `path`, apply migrations,
/// and return the connection.
pub fn open(path: &Path) -> Result<Connection> {
    let conn =
        Connection::open(path).with_context(|| format!("open sqlite at {}", path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS docs (
            doc_id      TEXT PRIMARY KEY,
            rel_path    TEXT NOT NULL UNIQUE,
            dir         TEXT NOT NULL,
            title       TEXT NOT NULL,
            tokens      INTEGER NOT NULL,
            mtime_unix  INTEGER NOT NULL,
            size_bytes  INTEGER NOT NULL,
            is_markdown INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS docs_dir ON docs(dir);
        CREATE TABLE IF NOT EXISTS sections (
            doc_id       TEXT NOT NULL REFERENCES docs(doc_id) ON DELETE CASCADE,
            section_id   TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            level        INTEGER NOT NULL,
            heading      TEXT NOT NULL,
            heading_path TEXT NOT NULL,
            snippet      TEXT NOT NULL,
            tokens       INTEGER NOT NULL,
            code_tokens  INTEGER NOT NULL,
            content      BLOB NOT NULL,
            PRIMARY KEY (doc_id, section_id)
        );
        CREATE INDEX IF NOT EXISTS sections_doc_seq ON sections(doc_id, seq);
        CREATE TABLE IF NOT EXISTS user_ignores (
            pattern   TEXT PRIMARY KEY,
            added_at  INTEGER NOT NULL
        );
        "#,
    )?;
    // Stamp the schema version if absent.
    set_meta_if_absent(conn, "schema_version", &SCHEMA_VERSION.to_string())?;
    Ok(())
}

fn set_meta_if_absent(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get::<_, String>(0)
        })
        .optional()?)
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES(?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn all_doc_stats(conn: &Connection) -> Result<Vec<DocStat>> {
    let mut stmt = conn.prepare("SELECT doc_id, rel_path, mtime_unix, size_bytes FROM docs")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DocStat {
                doc_id: r.get(0)?,
                rel_path: r.get(1)?,
                mtime_unix: r.get(2)?,
                size_bytes: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_doc(conn: &Connection, d: &DocRow) -> Result<()> {
    conn.execute(
        "INSERT INTO docs(doc_id, rel_path, dir, title, tokens, mtime_unix, size_bytes, is_markdown) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(doc_id) DO UPDATE SET \
            rel_path=excluded.rel_path, dir=excluded.dir, title=excluded.title, \
            tokens=excluded.tokens, mtime_unix=excluded.mtime_unix, \
            size_bytes=excluded.size_bytes, is_markdown=excluded.is_markdown",
        params![
            d.doc_id, d.rel_path, d.dir, d.title, d.tokens,
            d.mtime_unix, d.size_bytes, d.is_markdown as i64,
        ],
    )?;
    Ok(())
}

pub fn insert_section(conn: &Connection, s: &SectionRow) -> Result<()> {
    conn.execute(
        "INSERT INTO sections(doc_id, section_id, seq, level, heading, heading_path, snippet, tokens, code_tokens, content) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            s.doc_id, s.section_id, s.seq, s.level, s.heading,
            s.heading_path, s.snippet, s.tokens, s.code_tokens, s.content,
        ],
    )?;
    Ok(())
}

/// Cascades to sections via FK.
pub fn delete_doc(conn: &Connection, doc_id: &str) -> Result<()> {
    conn.execute("DELETE FROM docs WHERE doc_id = ?1", params![doc_id])?;
    Ok(())
}

pub fn list_user_ignores(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT pattern FROM user_ignores ORDER BY pattern")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn add_user_ignore(conn: &Connection, pattern: &str, now_unix: i64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO user_ignores(pattern, added_at) VALUES(?1, ?2)",
        params![pattern, now_unix],
    )?;
    Ok(())
}

pub fn remove_user_ignore(conn: &Connection, pattern: &str) -> Result<bool> {
    let n = conn.execute(
        "DELETE FROM user_ignores WHERE pattern = ?1",
        params![pattern],
    )?;
    Ok(n > 0)
}

pub fn clear_user_ignores(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM user_ignores", [])?;
    Ok(())
}

/// Resolve a doc-id prefix to a full doc_id.
/// Returns Err if no match or ambiguous (message lists candidates).
pub fn resolve_doc_id(conn: &Connection, prefix: &str) -> Result<String> {
    let mut stmt = conn
        .prepare("SELECT doc_id FROM docs WHERE doc_id LIKE ?1 || '%' ORDER BY doc_id LIMIT 10")?;
    let rows: Vec<String> = stmt
        .query_map(params![prefix], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    match rows.len() {
        0 => anyhow::bail!("no doc matching '{prefix}'"),
        1 => Ok(rows.into_iter().next().unwrap()),
        _ => anyhow::bail!("doc id '{prefix}' matches multiple: {}", rows.join(", ")),
    }
}

/// Try resolving as a doc-id prefix first; if that fails, try as a path.
pub fn resolve_doc_id_or_path(conn: &Connection, input: &str) -> Result<String> {
    match resolve_doc_id(conn, input) {
        Ok(id) => Ok(id),
        Err(_) => {
            let id: String = conn
                .query_row(
                    "SELECT doc_id FROM docs WHERE rel_path = ?1",
                    params![input],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| anyhow::anyhow!("no doc matching '{input}'"))?;
            Ok(id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, Connection) {
        let tmp = TempDir::new().unwrap();
        let db = open(&tmp.path().join("m.db")).unwrap();
        (tmp, db)
    }

    #[test]
    fn opens_and_stamps_schema_version() {
        let (_tmp, conn) = fresh();
        let v = get_meta(&conn, "schema_version").unwrap();
        assert_eq!(v.as_deref(), Some("1"));
    }

    #[test]
    fn meta_upsert_roundtrip() {
        let (_tmp, conn) = fresh();
        set_meta(&conn, "last_scan_unix", "100").unwrap();
        set_meta(&conn, "last_scan_unix", "200").unwrap();
        assert_eq!(
            get_meta(&conn, "last_scan_unix").unwrap().as_deref(),
            Some("200")
        );
    }

    fn sample_doc(id: &str, path: &str, mtime: i64, size: i64) -> DocRow {
        DocRow {
            doc_id: id.into(),
            rel_path: path.into(),
            dir: "docs".into(),
            title: "T".into(),
            tokens: 10,
            mtime_unix: mtime,
            size_bytes: size,
            is_markdown: true,
        }
    }

    fn sample_section(doc_id: &str, sid: &str, seq: i64) -> SectionRow {
        SectionRow {
            doc_id: doc_id.into(),
            section_id: sid.into(),
            seq,
            level: 1,
            heading: "H".into(),
            heading_path: "".into(),
            snippet: "snip".into(),
            tokens: 5,
            code_tokens: 0,
            content: b"# H\nbody\n".to_vec(),
        }
    }

    #[test]
    fn docs_upsert_and_stats() {
        let (_tmp, conn) = fresh();
        upsert_doc(&conn, &sample_doc("aaaaaaaa", "a.md", 10, 100)).unwrap();
        upsert_doc(&conn, &sample_doc("aaaaaaaa", "a.md", 20, 150)).unwrap();
        let stats = all_doc_stats(&conn).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].mtime_unix, 20);
        assert_eq!(stats[0].size_bytes, 150);
    }

    #[test]
    fn delete_doc_cascades_sections() {
        let (_tmp, conn) = fresh();
        upsert_doc(&conn, &sample_doc("aaaaaaaa", "a.md", 1, 1)).unwrap();
        insert_section(&conn, &sample_section("aaaaaaaa", "s1s1s1", 0)).unwrap();
        insert_section(&conn, &sample_section("aaaaaaaa", "s2s2s2", 1)).unwrap();
        delete_doc(&conn, "aaaaaaaa").unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM sections", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn user_ignores_crud() {
        let (_tmp, conn) = fresh();
        add_user_ignore(&conn, "drafts/**", 100).unwrap();
        add_user_ignore(&conn, "CHANGELOG*", 100).unwrap();
        add_user_ignore(&conn, "drafts/**", 100).unwrap(); // dup no-op
        assert_eq!(
            list_user_ignores(&conn).unwrap(),
            vec!["CHANGELOG*", "drafts/**"]
        );
        assert!(remove_user_ignore(&conn, "CHANGELOG*").unwrap());
        assert!(!remove_user_ignore(&conn, "CHANGELOG*").unwrap());
        assert_eq!(list_user_ignores(&conn).unwrap(), vec!["drafts/**"]);
        clear_user_ignores(&conn).unwrap();
        assert!(list_user_ignores(&conn).unwrap().is_empty());
    }

    #[test]
    fn resolve_doc_id_prefix_match() {
        let (_tmp, conn) = fresh();
        upsert_doc(&conn, &sample_doc("abc12345", "a.md", 1, 1)).unwrap();
        upsert_doc(&conn, &sample_doc("abd67890", "b.md", 1, 1)).unwrap();
        assert_eq!(resolve_doc_id(&conn, "abc12345").unwrap(), "abc12345");
        assert_eq!(resolve_doc_id(&conn, "abc").unwrap(), "abc12345");
        assert!(
            resolve_doc_id(&conn, "ab")
                .unwrap_err()
                .to_string()
                .contains("multiple")
        );
        assert!(
            resolve_doc_id(&conn, "zz")
                .unwrap_err()
                .to_string()
                .contains("no doc")
        );
    }
}
