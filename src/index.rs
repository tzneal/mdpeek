use crate::db::DocRow;
use crate::{cache, db, repo, search::SearchIndex, sync};
use anyhow::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

const AUTO_INDEX_TTL: u64 = 5;

pub fn run(path: Option<PathBuf>, json: bool, _no_auto_index: bool) -> Result<()> {
    let cwd = path.unwrap_or(std::env::current_dir()?);
    let root = repo::find_root(&cwd)?;
    let paths = cache::paths_for(&root)?;
    let mut conn = db::open(&paths.db)?;
    let mut search = SearchIndex::open(&paths.tantivy)?;
    // `index` always forces a reindex regardless of TTL.
    let report = sync::run_if_stale(&mut conn, &mut search, &root, AUTO_INDEX_TTL, true)?;
    let rows = fetch_rows(&conn)?;
    if json {
        let mut v = render_json_value(&root, &rows);
        if !report.skipped_binary.is_empty() {
            v["warnings"] = serde_json::json!(
                report
                    .skipped_binary
                    .iter()
                    .map(|f| format!("skipping non-UTF-8 file: {f}"))
                    .collect::<Vec<_>>()
            );
        }
        println!("{v}");
    } else {
        for f in &report.skipped_binary {
            eprintln!("warning: skipping non-UTF-8 file: {f}");
        }
        print!("{}", render_text(&rows));
    }
    Ok(())
}

fn fetch_rows(conn: &Connection) -> Result<Vec<(DocRow, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT d.doc_id, d.rel_path, d.dir, d.title, d.tokens, d.mtime_unix, d.size_bytes, d.is_markdown, \
         (SELECT COUNT(*) FROM sections s WHERE s.doc_id = d.doc_id) \
         FROM docs d ORDER BY d.dir, d.rel_path",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                DocRow {
                    doc_id: r.get(0)?,
                    rel_path: r.get(1)?,
                    dir: r.get(2)?,
                    title: r.get(3)?,
                    tokens: r.get(4)?,
                    mtime_unix: r.get(5)?,
                    size_bytes: r.get(6)?,
                    is_markdown: r.get::<_, i64>(7)? != 0,
                },
                r.get::<_, i64>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn render_text(rows: &[(DocRow, i64)]) -> String {
    let mut out = String::new();
    let mut cur_dir: Option<&str> = None;
    for (row, _) in rows {
        if cur_dir != Some(row.dir.as_str()) {
            if cur_dir.is_some() {
                out.push('\n');
            }
            let label = if row.dir.is_empty() {
                "./".to_string()
            } else {
                format!("{}/", row.dir)
            };
            out.push_str(&format!("{label}\n"));
            out.push_str("| ID       | File                 | Title                          | Tokens | Modified   |\n");
            out.push_str("|----------|----------------------|--------------------------------|--------|------------|\n");
            cur_dir = Some(row.dir.as_str());
        }
        let filename = Path::new(&row.rel_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        out.push_str(&format!(
            "| {:<8} | {:<20} | {:<30} | {:>6} | {} |\n",
            row.doc_id,
            truncate(&filename, 20),
            truncate(&row.title, 30),
            row.tokens,
            format_date(row.mtime_unix),
        ));
    }
    out
}

fn render_json_value(root: &Path, rows: &[(DocRow, i64)]) -> serde_json::Value {
    use serde_json::json;
    let total_docs = rows.len();
    let total_tokens: i64 = rows.iter().map(|(r, _)| r.tokens).sum();
    let mut groups: Vec<serde_json::Value> = Vec::new();
    let mut cur: Option<(String, Vec<serde_json::Value>)> = None;
    for (row, section_count) in rows {
        let filename = Path::new(&row.rel_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let doc = json!({
            "id": row.doc_id,
            "path": row.rel_path,
            "file": filename,
            "title": row.title,
            "tokens": row.tokens,
            "section_count": section_count,
            "modified": format_datetime(row.mtime_unix),
        });
        match cur.as_mut() {
            Some((dir, docs)) if dir == &row.dir => docs.push(doc),
            _ => {
                if let Some((dir, docs)) = cur.take() {
                    groups.push(json!({"dir": dir, "docs": docs}));
                }
                cur = Some((row.dir.clone(), vec![doc]));
            }
        }
    }
    if let Some((dir, docs)) = cur {
        groups.push(json!({"dir": dir, "docs": docs}));
    }
    json!({
        "repo_root": root.display().to_string(),
        "total_docs": total_docs,
        "total_tokens": total_tokens,
        "groups": groups,
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// YYYY-MM-DD from unix seconds (UTC). No external deps.
fn format_date(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Full ISO-8601 UTC `YYYY-MM-DDTHH:MM:SSZ`.
fn format_datetime(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs_of_day = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let h = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

/// Howard Hinnant's civil_from_days algorithm.
/// `days` is days since 1970-01-01. Returns (year, month[1-12], day[1-31]).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(dir: &str, id: &str, title: &str, tokens: i64, mtime: i64) -> DocRow {
        DocRow {
            doc_id: id.into(),
            rel_path: format!("{dir}/{id}.md").trim_start_matches('/').into(),
            dir: dir.into(),
            title: title.into(),
            tokens,
            mtime_unix: mtime,
            size_bytes: 0,
            is_markdown: true,
        }
    }

    #[test]
    fn text_groups_by_dir_with_headers() {
        let rows = vec![
            (row("", "aaaaaaaa", "Root doc", 10, 0), 1),
            (row("docs", "bbbbbbbb", "Intro", 20, 0), 2),
            (row("docs", "cccccccc", "Deep Dive", 30, 0), 3),
        ];
        let out = render_text(&rows);
        assert!(out.contains("./\n"));
        assert!(out.contains("docs/\n"));
        assert!(out.contains("aaaaaaaa"));
        assert!(out.contains("Intro"));
        // One header per group, not per row.
        assert_eq!(out.matches("| ID       |").count(), 2);
        // Filename column present.
        assert!(out.contains("| File"));
        assert!(out.contains("aaaaaaaa.md"));
    }

    #[test]
    fn json_shape_matches_design() {
        let rows = vec![(row("docs", "a3f1b208", "Getting Started", 1243, 0), 3)];
        let v = render_json_value(Path::new("/r"), &rows);
        assert_eq!(v["repo_root"], "/r");
        assert_eq!(v["total_docs"], 1);
        assert_eq!(v["total_tokens"], 1243);
        assert_eq!(v["groups"][0]["dir"], "docs");
        assert_eq!(v["groups"][0]["docs"][0]["id"], "a3f1b208");
        assert_eq!(v["groups"][0]["docs"][0]["file"], "a3f1b208.md");
        assert_eq!(v["groups"][0]["docs"][0]["title"], "Getting Started");
        assert_eq!(v["groups"][0]["docs"][0]["section_count"], 3);
        assert_eq!(
            v["groups"][0]["docs"][0]["modified"],
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn format_date_known_values() {
        // 2023-01-01 00:00:00 UTC = 1_672_531_200
        assert_eq!(format_date(1_672_531_200), "2023-01-01");
        // 2024-02-29 (leap day)
        // 2024-02-29 00:00:00 UTC = 1_709_164_800
        assert_eq!(format_date(1_709_164_800), "2024-02-29");
        assert_eq!(format_date(0), "1970-01-01");
    }

    #[test]
    fn format_datetime_roundtrip() {
        assert_eq!(format_datetime(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_datetime(3661), "1970-01-01T01:01:01Z");
        assert_eq!(format_datetime(1_672_531_200), "2023-01-01T00:00:00Z");
    }

    #[test]
    fn truncate_ellipsizes() {
        assert_eq!(truncate("hello", 10), "hello");
        let t = truncate("abcdefghij", 5);
        assert_eq!(t.chars().count(), 5);
        assert!(t.ends_with('…'));
    }
}
