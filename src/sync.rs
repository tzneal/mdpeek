use crate::db::{self, DocRow, SectionRow};
use crate::id::doc_id;
use crate::parse::{self, Section};
use crate::repo;
use crate::search::SearchIndex;
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Summary of one sync pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    /// Files skipped due to non-UTF-8 content.
    pub skipped_binary: Vec<String>,
    /// True when TTL short-circuited the walk.
    pub skipped: bool,
}

/// Run the staleness pass. Short-circuits if `now - last_scan_unix < ttl_secs`
/// unless `force` is true.
pub fn run_if_stale(
    conn: &mut Connection,
    search: &mut SearchIndex,
    repo_root: &Path,
    ttl_secs: u64,
    force: bool,
) -> Result<SyncReport> {
    let now = unix_now();
    if !force && recent_enough(conn, now, ttl_secs)? {
        return Ok(SyncReport {
            skipped: true,
            ..Default::default()
        });
    }

    let patterns = db::list_user_ignores(conn)?;
    let paths = repo::walk_docs(repo_root, &patterns)?;

    // Existing docs, keyed by rel_path.
    let existing: HashMap<String, db::DocStat> = db::all_doc_stats(conn)?
        .into_iter()
        .map(|s| (s.rel_path.clone(), s))
        .collect();

    let mut seen = std::collections::HashSet::with_capacity(paths.len());
    let mut report = SyncReport::default();

    let tx = conn.transaction()?;
    for abs in &paths {
        let rel = rel_path(repo_root, abs);
        seen.insert(rel.clone());
        let meta = std::fs::metadata(abs).with_context(|| format!("stat {}", abs.display()))?;
        let mtime = mtime_unix(&meta);
        let size = meta.len() as i64;

        // Skip binary (non-UTF-8) files early.
        let bytes = std::fs::read(abs).with_context(|| format!("read {}", abs.display()))?;
        if std::str::from_utf8(&bytes).is_err() {
            report.skipped_binary.push(rel);
            continue;
        }

        match existing.get(&rel) {
            Some(s) if s.mtime_unix == mtime && s.size_bytes == size => continue,
            Some(_) => report.changed += 1,
            None => report.added += 1,
        }
        upsert_one(&tx, search, abs, &rel, &bytes, mtime, size)?;
    }

    // Removed: in db but not in walk.
    for (rel, s) in &existing {
        if !seen.contains(rel) {
            db::delete_doc(&tx, &s.doc_id)?;
            search.delete_doc(&s.doc_id);
            report.removed += 1;
        }
    }

    db::set_meta(&tx, "last_scan_unix", &now.to_string())?;
    tx.commit()?;
    search.commit()?;
    Ok(report)
}

fn upsert_one(
    conn: &Connection,
    search: &SearchIndex,
    abs: &Path,
    rel: &str,
    bytes: &[u8],
    mtime: i64,
    size: i64,
) -> Result<()> {
    let is_markdown = matches!(
        abs.extension().and_then(|e| e.to_str()),
        Some(e) if e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("mdx")
    );
    let stem = abs
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let sections = parse::parse(bytes, &stem, is_markdown);

    let id = doc_id(rel);
    // Replace existing rows/sections.
    db::delete_doc(conn, &id)?;

    let dir = top_dir(rel).to_string();
    let total_tokens: i64 = sections.iter().map(|s| s.tokens as i64).sum();
    let title = derive_title(&sections, bytes, &stem);

    db::upsert_doc(
        conn,
        &DocRow {
            doc_id: id.clone(),
            rel_path: rel.to_string(),
            dir: dir.clone(),
            title,
            tokens: total_tokens,
            mtime_unix: mtime,
            size_bytes: size,
            is_markdown,
        },
    )?;
    // Deduplicate section IDs: when two sections in the same doc have
    // identical content they produce the same hash.  Append a counter
    // suffix to make each ID unique within the doc.
    let mut id_counts: HashMap<String, usize> = HashMap::new();
    let mut final_ids: Vec<String> = Vec::with_capacity(sections.len());
    for sec in &sections {
        let n = id_counts.entry(sec.id.clone()).or_insert(0);
        final_ids.push(if *n == 0 {
            sec.id.clone()
        } else {
            format!("{}_{n}", &sec.id[..4])
        });
        *n += 1;
    }

    for (seq, (sec, sid)) in sections.iter().zip(final_ids.iter()).enumerate() {
        db::insert_section(
            conn,
            &SectionRow {
                doc_id: id.clone(),
                section_id: sid.clone(),
                seq: seq as i64,
                level: sec.level as i64,
                heading: sec.heading.clone(),
                heading_path: sec.heading_path.clone(),
                snippet: sec.snippet.clone(),
                tokens: sec.tokens as i64,
                code_tokens: sec.code_tokens as i64,
                content: sec.content.clone(),
            },
        )?;
    }
    search.upsert_sections(&id, rel, &dir, mtime as u64, &sections)?;
    Ok(())
}

fn recent_enough(conn: &Connection, now: i64, ttl_secs: u64) -> Result<bool> {
    let Some(v) = db::get_meta(conn, "last_scan_unix")? else {
        return Ok(false);
    };
    let last: i64 = v.parse().unwrap_or(0);
    Ok(now >= last && (now - last) < ttl_secs as i64)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mtime_unix(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn rel_path(root: &Path, abs: &Path) -> String {
    let p: PathBuf = abs.strip_prefix(root).unwrap_or(abs).to_path_buf();
    p.to_string_lossy().replace('\\', "/")
}

fn top_dir(rel: &str) -> &str {
    rel.split_once('/').map(|(d, _)| d).unwrap_or("")
}

/// First H1; else first non-empty line with ≥2 alphanumeric words; else stem.
fn derive_title(sections: &[Section], bytes: &[u8], stem: &str) -> String {
    if let Some(h) = sections.iter().find(|s| s.level == 1)
        && has_enough_words(&h.heading)
    {
        return h.heading.clone();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        for line in text.lines() {
            let t = line.trim().trim_start_matches('#').trim();
            if has_enough_words(t) {
                return t.to_string();
            }
        }
    }
    stem.to_string()
}

/// At least 1 word containing an alphanumeric character.
fn has_enough_words(s: &str) -> bool {
    s.split_whitespace()
        .any(|w| w.chars().any(|c| c.is_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct Fx {
        _xdg: TempDir,
        repo: TempDir,
        conn: Connection,
        search: SearchIndex,
    }

    fn setup() -> Fx {
        let xdg = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let paths = crate::cache::paths_for_in(repo.path(), xdg.path()).unwrap();
        let conn = db::open(&paths.db).unwrap();
        let search = SearchIndex::open(&paths.tantivy).unwrap();
        Fx {
            _xdg: xdg,
            repo,
            conn,
            search,
        }
    }

    fn write(root: &Path, rel: &str, body: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn skips_binary_files() {
        let mut fx = setup();
        write(fx.repo.path(), "good.md", b"# Good\nbody\n");
        let bin = fx.repo.path().join("bad.txt");
        fs::write(&bin, b"\x00\x80\xff binary junk").unwrap();
        let r = run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        assert_eq!(r.added, 1); // only good.md
        assert_eq!(r.skipped_binary.len(), 1);
        assert!(r.skipped_binary[0].contains("bad.txt"));
    }

    #[test]
    fn adds_new_files() {
        let mut fx = setup();
        write(fx.repo.path(), "docs/a.md", b"# A\nbody\n");
        write(fx.repo.path(), "b.txt", b"plain");
        let r = run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        assert_eq!(r.added, 2);
        assert_eq!(r.changed, 0);
        assert_eq!(r.removed, 0);
        assert_eq!(db::all_doc_stats(&fx.conn).unwrap().len(), 2);
    }

    #[test]
    fn ttl_short_circuits() {
        let mut fx = setup();
        write(fx.repo.path(), "a.md", b"# A\n");
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        write(fx.repo.path(), "b.md", b"# B\n");
        let r = run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 60, false).unwrap();
        assert!(r.skipped);
        assert_eq!(db::all_doc_stats(&fx.conn).unwrap().len(), 1);
    }

    #[test]
    fn force_bypasses_ttl() {
        let mut fx = setup();
        write(fx.repo.path(), "a.md", b"# A\n");
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        write(fx.repo.path(), "b.md", b"# B\n");
        let r = run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 60, true).unwrap();
        assert!(!r.skipped);
        assert_eq!(r.added, 1);
    }

    #[test]
    fn detects_changed_files() {
        let mut fx = setup();
        write(fx.repo.path(), "a.md", b"# A\nv1\n");
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        // Rewrite with different size; mtime on most fs bumps too.
        write(fx.repo.path(), "a.md", b"# A\nv2 larger body\n");
        let r = run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 0, true).unwrap();
        assert_eq!(r.changed, 1);
        assert_eq!(r.added, 0);
    }

    #[test]
    fn detects_removed_files() {
        let mut fx = setup();
        write(fx.repo.path(), "a.md", b"# A\n");
        write(fx.repo.path(), "b.md", b"# B\n");
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        fs::remove_file(fx.repo.path().join("b.md")).unwrap();
        let r = run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 0, true).unwrap();
        assert_eq!(r.removed, 1);
        assert_eq!(db::all_doc_stats(&fx.conn).unwrap().len(), 1);
    }

    #[test]
    fn derives_title_from_h1_then_first_line_then_stem() {
        let mut fx = setup();
        write(fx.repo.path(), "h1.md", b"# The Title\nbody\n");
        write(fx.repo.path(), "pre.md", b"leading paragraph\n\n## Sub\n");
        write(fx.repo.path(), "bare.txt", b"");
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        let titles: HashMap<String, String> = fx
            .conn
            .prepare("SELECT rel_path, title FROM docs")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(titles["h1.md"], "The Title");
        assert_eq!(titles["pre.md"], "leading paragraph");
        assert_eq!(titles["bare.txt"], "bare");
    }

    #[test]
    fn search_index_sees_new_and_removed_docs() {
        let mut fx = setup();
        write(
            fx.repo.path(),
            "a.md",
            b"# Install\nrun cargo install mdpeek\n",
        );
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        assert_eq!(fx.search.query("cargo", 10, false).unwrap().len(), 1);

        fs::remove_file(fx.repo.path().join("a.md")).unwrap();
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 0, true).unwrap();
        assert_eq!(fx.search.query("cargo", 10, false).unwrap().len(), 0);
    }

    #[test]
    fn stamps_last_scan_unix() {
        let mut fx = setup();
        write(fx.repo.path(), "a.md", b"# A\n");
        run_if_stale(&mut fx.conn, &mut fx.search, fx.repo.path(), 5, true).unwrap();
        let v = db::get_meta(&fx.conn, "last_scan_unix").unwrap().unwrap();
        let parsed: i64 = v.parse().unwrap();
        assert!(parsed > 0);
    }
}
