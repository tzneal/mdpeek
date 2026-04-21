use crate::{cache, db, repo, search::SearchIndex, sync, token};
use anyhow::Result;
use rusqlite::params;

pub fn run(
    targets: &[String],
    json: bool,
    no_auto_index: bool,
    max_tokens: Option<usize>,
    start_token: usize,
    no_code: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = repo::find_root(&cwd)?;
    let paths = cache::paths_for(&root)?;
    let mut conn = db::open(&paths.db)?;
    let mut search = SearchIndex::open(&paths.tantivy)?;
    if !no_auto_index {
        sync::run_if_stale(&mut conn, &mut search, &root, sync::AUTO_INDEX_TTL, false)?;
    }

    let parsed = parse_targets(targets);
    let budget = Budget {
        max_tokens,
        start_token,
        no_code,
    };

    if json && parsed.len() > 1 {
        let mut arr = Vec::new();
        for (doc_input, sec) in &parsed {
            arr.push(show_one_json(&conn, doc_input, sec.as_deref(), &budget)?);
        }
        println!("{}", serde_json::Value::Array(arr));
    } else {
        for (doc_input, sec) in &parsed {
            show_one(&conn, doc_input, sec.as_deref(), json, &budget)?;
        }
    }
    Ok(())
}

struct Budget {
    max_tokens: Option<usize>,
    start_token: usize,
    no_code: bool,
}

fn parse_targets(targets: &[String]) -> Vec<(String, Option<String>)> {
    // Two args where neither contains ':' and second looks like a section id
    // (or has sec: prefix) → backwards-compat single target with section.
    if targets.len() == 2
        && !targets[0].contains(':')
        && (targets[1].starts_with("sec:") || !targets[1].contains('/'))
    {
        // Could be "doc section" or "doc1 doc2". If second starts with sec: it's
        // definitely a section. Otherwise check if it looks like a hex id.
        let sec = strip_sec_prefix(&targets[1]);
        if targets[1].starts_with("sec:") || sec.chars().all(|c| c.is_ascii_hexdigit()) {
            return vec![(targets[0].clone(), Some(sec.to_string()))];
        }
    }

    targets
        .iter()
        .map(|t| {
            if let Some((d, s)) = t.split_once(':') {
                (d.to_string(), Some(strip_sec_prefix(s).to_string()))
            } else {
                (t.clone(), None)
            }
        })
        .collect()
}

fn show_one(
    conn: &rusqlite::Connection,
    doc_input: &str,
    sec_prefix: Option<&str>,
    json: bool,
    budget: &Budget,
) -> Result<()> {
    let doc_id = db::resolve_doc_id_or_path(conn, doc_input)?;
    let rel_path: String = conn.query_row(
        "SELECT rel_path FROM docs WHERE doc_id = ?1",
        params![doc_id],
        |r| r.get(0),
    )?;
    if let Some(sp) = sec_prefix {
        show_section(conn, &doc_id, sp, &rel_path, json, budget)
    } else {
        show_full(conn, &doc_id, &rel_path, json, budget)
    }
}

fn show_one_json(
    conn: &rusqlite::Connection,
    doc_input: &str,
    sec_prefix: Option<&str>,
    budget: &Budget,
) -> Result<serde_json::Value> {
    let doc_id = db::resolve_doc_id_or_path(conn, doc_input)?;
    let rel_path: String = conn.query_row(
        "SELECT rel_path FROM docs WHERE doc_id = ?1",
        params![doc_id],
        |r| r.get(0),
    )?;
    if let Some(sp) = sec_prefix {
        section_json(conn, &doc_id, sp, &rel_path, budget)
    } else {
        full_json(conn, &doc_id, &rel_path, budget)
    }
}

struct SectionHit {
    section_id: String,
    heading: String,
    tokens: i64,
    content: Vec<u8>,
}

fn resolve_section(
    conn: &rusqlite::Connection,
    doc_id: &str,
    sec_prefix: &str,
) -> Result<SectionHit> {
    let mut stmt = conn.prepare(
        "SELECT section_id, heading, tokens, content FROM sections \
         WHERE doc_id = ?1 AND section_id LIKE ?2 || '%' ORDER BY section_id LIMIT 10",
    )?;
    let rows: Vec<SectionHit> = stmt
        .query_map(params![doc_id, sec_prefix], |r| {
            Ok(SectionHit {
                section_id: r.get(0)?,
                heading: r.get(1)?,
                tokens: r.get(2)?,
                content: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    match rows.len() {
        0 => anyhow::bail!("no section matching '{sec_prefix}' in doc {doc_id}"),
        1 => Ok(rows.into_iter().next().unwrap()),
        _ => {
            let ids: Vec<_> = rows.iter().map(|r| r.section_id.as_str()).collect();
            anyhow::bail!(
                "section id '{sec_prefix}' matches multiple: {}",
                ids.join(", ")
            );
        }
    }
}

fn show_section(
    conn: &rusqlite::Connection,
    doc_id: &str,
    sec_prefix: &str,
    rel_path: &str,
    json: bool,
    budget: &Budget,
) -> Result<()> {
    let hit = resolve_section(conn, doc_id, sec_prefix)?;
    if json {
        println!("{}", section_json_value(doc_id, rel_path, &hit, budget));
    } else {
        let content = String::from_utf8_lossy(&hit.content);
        let prepared = prepare(&content, budget);
        print!("{}", maybe_truncate(&prepared, budget));
    }
    Ok(())
}

fn section_json(
    conn: &rusqlite::Connection,
    doc_id: &str,
    sec_prefix: &str,
    rel_path: &str,
    budget: &Budget,
) -> Result<serde_json::Value> {
    let hit = resolve_section(conn, doc_id, sec_prefix)?;
    Ok(section_json_value(doc_id, rel_path, &hit, budget))
}

fn section_json_value(
    doc_id: &str,
    rel_path: &str,
    hit: &SectionHit,
    budget: &Budget,
) -> serde_json::Value {
    let raw = String::from_utf8_lossy(&hit.content);
    let prepared = prepare(&raw, budget);
    content_json(
        doc_id,
        rel_path,
        None,
        Some(&hit.section_id),
        Some(&hit.heading),
        hit.tokens,
        &prepared,
        budget,
    )
}

fn fetch_full(conn: &rusqlite::Connection, doc_id: &str) -> Result<(i64, String, String)> {
    let mut stmt = conn.prepare("SELECT content FROM sections WHERE doc_id = ?1 ORDER BY seq")?;
    let blobs: Vec<Vec<u8>> = stmt
        .query_map(params![doc_id], |r| r.get::<_, Vec<u8>>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut full = Vec::new();
    for b in &blobs {
        full.extend_from_slice(b);
    }
    let (tokens, title): (i64, String) = conn.query_row(
        "SELECT tokens, title FROM docs WHERE doc_id = ?1",
        params![doc_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((tokens, title, String::from_utf8_lossy(&full).into_owned()))
}

fn show_full(
    conn: &rusqlite::Connection,
    doc_id: &str,
    rel_path: &str,
    json: bool,
    budget: &Budget,
) -> Result<()> {
    let (tokens, title, text) = fetch_full(conn, doc_id)?;
    let prepared = prepare(&text, budget);
    if json {
        println!(
            "{}",
            content_json(
                doc_id,
                rel_path,
                Some(&title),
                None,
                None,
                tokens,
                &prepared,
                budget
            ),
        );
    } else {
        print!("{}", maybe_truncate(&prepared, budget));
    }
    Ok(())
}

fn full_json(
    conn: &rusqlite::Connection,
    doc_id: &str,
    rel_path: &str,
    budget: &Budget,
) -> Result<serde_json::Value> {
    let (tokens, title, text) = fetch_full(conn, doc_id)?;
    let prepared = prepare(&text, budget);
    Ok(content_json(
        doc_id,
        rel_path,
        Some(&title),
        None,
        None,
        tokens,
        &prepared,
        budget,
    ))
}

/// Build JSON for content, applying token budget if set.
#[allow(clippy::too_many_arguments)]
fn content_json(
    doc_id: &str,
    path: &str,
    title: Option<&str>,
    section_id: Option<&str>,
    heading: Option<&str>,
    tokens: i64,
    raw: &str,
    budget: &Budget,
) -> serde_json::Value {
    let mut v = serde_json::json!({
        "doc_id": doc_id,
        "path": path,
        "tokens": tokens,
    });
    if let Some(t) = title {
        v["title"] = serde_json::Value::String(t.to_string());
    }
    if let Some(sid) = section_id {
        v["section_id"] = serde_json::Value::String(sid.to_string());
    }
    if let Some(h) = heading {
        v["heading"] = serde_json::Value::String(h.to_string());
    }
    if let Some(max) = budget.max_tokens {
        let t = token::truncate(raw, budget.start_token, max);
        v["content"] = serde_json::Value::String(t.text);
        v["start_token"] = serde_json::json!(budget.start_token);
        v["end_token"] = serde_json::json!(
            budget.start_token + max.min(t.total_tokens.saturating_sub(budget.start_token))
        );
        v["truncated"] = serde_json::json!(t.truncated);
    } else {
        v["content"] = serde_json::Value::String(raw.to_string());
    }
    v
}

/// Apply token budget for plain-text output. Returns the (possibly truncated) text.
fn maybe_truncate(text: &str, budget: &Budget) -> String {
    match budget.max_tokens {
        Some(max) => {
            let t = token::truncate(text, budget.start_token, max);
            t.text
        }
        None => text.to_string(),
    }
}

/// Strip optional "sec:" prefix so users can paste from outline output.
fn strip_sec_prefix(s: &str) -> &str {
    s.strip_prefix("sec:").unwrap_or(s)
}

/// If `budget.no_code`, replace each fenced code block with
/// `[code: N lines]` placeholder. Otherwise return input unchanged.
fn prepare(text: &str, budget: &Budget) -> String {
    if !budget.no_code {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    let mut count = 0usize;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            if in_fence {
                out.push_str(&format!("[code: {count} lines]\n"));
                count = 0;
                in_fence = false;
            } else {
                in_fence = true;
            }
            continue;
        }
        if in_fence {
            count += 1;
        } else {
            out.push_str(line);
        }
    }
    // Unclosed fence: emit what we have.
    if in_fence && count > 0 {
        out.push_str(&format!("[code: {count} lines]\n"));
    }
    out
}
