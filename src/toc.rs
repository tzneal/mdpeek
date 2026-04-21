use crate::db::SectionRow;
use crate::{cache, db, repo, search::SearchIndex, sync};
use anyhow::Result;
use rusqlite::Connection;

pub fn run(json: bool, no_auto_index: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = repo::find_root(&cwd)?;
    let paths = cache::paths_for(&root)?;
    let mut conn = db::open(&paths.db)?;
    let mut search = SearchIndex::open(&paths.tantivy)?;
    if !no_auto_index {
        sync::run_if_stale(&mut conn, &mut search, &root, sync::AUTO_INDEX_TTL, false)?;
    }

    let docs = fetch_docs(&conn)?;
    if json {
        let arr: Vec<serde_json::Value> = docs
            .iter()
            .map(|(doc_id, rel_path, total_tokens)| {
                let secs = db::section_outlines(&conn, doc_id).unwrap_or_default();
                render_json(doc_id, rel_path, *total_tokens, &secs)
            })
            .collect();
        println!("{}", serde_json::Value::Array(arr));
    } else {
        let mut out = String::new();
        for (doc_id, rel_path, total_tokens) in &docs {
            let secs = db::section_outlines(&conn, doc_id)?;
            out.push_str(&render_text(doc_id, rel_path, *total_tokens, &secs));
            out.push('\n');
        }
        print!("{out}");
    }
    Ok(())
}

fn fetch_docs(conn: &Connection) -> Result<Vec<(String, String, i64)>> {
    let mut stmt =
        conn.prepare("SELECT doc_id, rel_path, tokens FROM docs ORDER BY dir, rel_path")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn render_text(doc_id: &str, rel_path: &str, total_tokens: i64, secs: &[SectionRow]) -> String {
    let mut out = format!("{rel_path}  [{doc_id}]  {total_tokens} tokens\n");
    for s in secs {
        let prefix = "#".repeat(s.level.max(1) as usize);
        out.push_str(&format!(
            "{prefix} {}  [sec:{}]  ({} tokens)\n",
            s.heading, s.section_id, s.tokens
        ));
    }
    out
}

fn render_json(
    doc_id: &str,
    rel_path: &str,
    total_tokens: i64,
    secs: &[SectionRow],
) -> serde_json::Value {
    use serde_json::json;
    let sections: Vec<_> = secs
        .iter()
        .map(|s| {
            json!({
                "id": s.section_id,
                "level": s.level,
                "heading": s.heading,
                "heading_path": s.heading_path,
                "tokens": s.tokens,
            })
        })
        .collect();
    json!({
        "doc_id": doc_id,
        "path": rel_path,
        "total_tokens": total_tokens,
        "sections": sections,
    })
}
