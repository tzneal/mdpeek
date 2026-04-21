use crate::db::SectionRow;
use crate::{cache, db, repo, search::SearchIndex, sync};
use anyhow::Result;
use rusqlite::params;

pub fn run(doc_ids: &[String], json: bool, no_auto_index: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = repo::find_root(&cwd)?;
    let paths = cache::paths_for(&root)?;
    let mut conn = db::open(&paths.db)?;
    let mut search = SearchIndex::open(&paths.tantivy)?;
    if !no_auto_index {
        sync::run_if_stale(&mut conn, &mut search, &root, sync::AUTO_INDEX_TTL, false)?;
    }

    if json && doc_ids.len() > 1 {
        let mut arr = Vec::new();
        for input in doc_ids {
            arr.push(outline_one_json(&conn, input)?);
        }
        println!("{}", serde_json::Value::Array(arr));
    } else {
        for input in doc_ids {
            outline_one(&conn, input, json)?;
        }
    }
    Ok(())
}

fn outline_one(conn: &rusqlite::Connection, input: &str, json: bool) -> Result<()> {
    let doc_id = db::resolve_doc_id_or_path(conn, input)?;
    let (rel_path, total_tokens) = conn.query_row(
        "SELECT rel_path, tokens FROM docs WHERE doc_id = ?1",
        params![doc_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
    )?;
    let sections = db::section_outlines(conn, &doc_id)?;
    if json {
        println!(
            "{}",
            render_json(&doc_id, &rel_path, total_tokens, &sections)
        );
    } else {
        print!(
            "{}",
            render_text(&doc_id, &rel_path, total_tokens, &sections)
        );
    }
    Ok(())
}

fn outline_one_json(conn: &rusqlite::Connection, input: &str) -> Result<serde_json::Value> {
    let doc_id = db::resolve_doc_id_or_path(conn, input)?;
    let (rel_path, total_tokens) = conn.query_row(
        "SELECT rel_path, tokens FROM docs WHERE doc_id = ?1",
        params![doc_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
    )?;
    let sections = db::section_outlines(conn, &doc_id)?;
    Ok(render_json_value(
        &doc_id,
        &rel_path,
        total_tokens,
        &sections,
    ))
}

fn render_text(doc_id: &str, rel_path: &str, total_tokens: i64, sections: &[SectionRow]) -> String {
    let mut out = format!("{rel_path}  [{doc_id}]  {total_tokens} tokens\n\n");
    for s in sections {
        let prefix = "#".repeat(s.level.max(1) as usize);
        let code_suffix = if s.code_tokens > 0 {
            format!(", {} code", s.code_tokens)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "{prefix} {}  [sec:{}]  ({} tokens{})\n",
            s.heading, s.section_id, s.tokens, code_suffix
        ));
        if !s.snippet.is_empty() {
            out.push_str(&format!("{}\n", s.snippet));
        }
        out.push('\n');
    }
    out
}

fn render_json(doc_id: &str, rel_path: &str, total_tokens: i64, sections: &[SectionRow]) -> String {
    render_json_value(doc_id, rel_path, total_tokens, sections).to_string()
}

fn render_json_value(
    doc_id: &str,
    rel_path: &str,
    total_tokens: i64,
    sections: &[SectionRow],
) -> serde_json::Value {
    use serde_json::json;
    let secs: Vec<_> = sections
        .iter()
        .map(|s| {
            json!({
                "id": s.section_id,
                "level": s.level,
                "heading": s.heading,
                "heading_path": s.heading_path,
                "snippet": s.snippet,
                "tokens": s.tokens,
                "code_tokens": s.code_tokens,
            })
        })
        .collect();
    json!({
        "doc_id": doc_id,
        "path": rel_path,
        "total_tokens": total_tokens,
        "sections": secs,
    })
}
