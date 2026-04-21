use crate::parse::Section;
use anyhow::{Context, Result};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    FAST, Field, IndexRecordOption, NumericOptions, STORED, STRING, Schema, SchemaBuilder,
    TextFieldIndexing, TextOptions,
};
use tantivy::tokenizer::{
    Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer, StopWordFilter, TextAnalyzer,
};
use tantivy::{Index, IndexWriter, TantivyDocument, Term};

/// Analyzer name registered on every Index::open.
const ANALYZER: &str = "mdpeek_en";
/// Heading field boost at query time.
const HEADING_BOOST: f32 = 3.0;
/// Writer memory budget (min allowed by tantivy is 15MB).
const WRITER_MEM: usize = 50_000_000;

/// Handle to the per-repo tantivy index.
pub(crate) struct SearchIndex {
    index: Index,
    writer: IndexWriter,
    f_doc_id: Field,
    f_section_id: Field,
    f_path: Field,
    f_dir: Field,
    f_heading: Field,
    f_heading_path: Field,
    f_body: Field,
    f_tokens: Field,
    f_mtime: Field,
}

/// One hit from a query.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hit {
    pub doc_id: String,
    pub section_id: String,
    pub score: f32,
    pub snippet: Option<String>,
}

pub fn run(
    query: &str,
    limit: usize,
    snippet: bool,
    include_content: bool,
    max_tokens: Option<usize>,
    json: bool,
    no_auto_index: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = crate::repo::find_root(&cwd)?;
    let paths = crate::cache::paths_for(&root)?;
    let mut conn = crate::db::open(&paths.db)?;
    let mut search = SearchIndex::open(&paths.tantivy)?;
    if !no_auto_index {
        crate::sync::run_if_stale(
            &mut conn,
            &mut search,
            &root,
            crate::sync::AUTO_INDEX_TTL,
            false,
        )?;
    }

    let hits = search.query(query, limit, snippet)?;
    if json {
        let arr: Vec<serde_json::Value> = hits
            .iter()
            .filter_map(|h| hit_json(&conn, h, include_content, max_tokens).ok())
            .collect();
        println!("{}", serde_json::Value::Array(arr));
    } else {
        if hits.is_empty() {
            println!("No results.");
            return Ok(());
        }
        println!(
            "| {:<8} | {:<6} | {:<30} | {:<30} | {:>6} | {:>5} |",
            "Doc", "Sec", "Path", "Heading", "Tokens", "Score"
        );
        println!(
            "|----------|--------|--------------------------------|--------------------------------|--------|-------|"
        );
        for h in &hits {
            let (path, heading, tokens) = hit_meta(&conn, h).unwrap_or_default();
            println!(
                "| {:<8} | {:<6} | {:<30} | {:<30} | {:>6} | {:>5.2} |",
                h.doc_id,
                h.section_id,
                truncate(&path, 30),
                truncate(&heading, 30),
                tokens,
                h.score
            );
            if let Some(snip) = &h.snippet
                && !snip.is_empty()
            {
                println!("  {snip}");
            }
            if include_content && let Ok(content) = hit_content(&conn, h) {
                println!("{}", content.trim());
            }
        }
    }
    Ok(())
}

fn hit_meta(conn: &rusqlite::Connection, h: &Hit) -> Result<(String, String, i64)> {
    use rusqlite::params;
    conn.query_row(
        "SELECT d.rel_path, s.heading, s.tokens FROM sections s \
         JOIN docs d ON d.doc_id = s.doc_id \
         WHERE s.doc_id = ?1 AND s.section_id = ?2",
        params![h.doc_id, h.section_id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        },
    )
    .map_err(Into::into)
}

fn hit_content(conn: &rusqlite::Connection, h: &Hit) -> Result<String> {
    use rusqlite::params;
    let bytes: Vec<u8> = conn.query_row(
        "SELECT content FROM sections WHERE doc_id = ?1 AND section_id = ?2",
        params![h.doc_id, h.section_id],
        |r| r.get(0),
    )?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn hit_json(
    conn: &rusqlite::Connection,
    h: &Hit,
    include_content: bool,
    max_tokens: Option<usize>,
) -> Result<serde_json::Value> {
    use rusqlite::params;
    let (path, heading_path, tokens): (String, String, i64) = conn.query_row(
        "SELECT d.rel_path, s.heading_path, s.tokens FROM sections s \
         JOIN docs d ON d.doc_id = s.doc_id \
         WHERE s.doc_id = ?1 AND s.section_id = ?2",
        params![h.doc_id, h.section_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let heading: String = conn.query_row(
        "SELECT heading FROM sections WHERE doc_id = ?1 AND section_id = ?2",
        params![h.doc_id, h.section_id],
        |r| r.get(0),
    )?;
    let mut v = serde_json::json!({
        "doc_id": h.doc_id,
        "section_id": h.section_id,
        "path": path,
        "heading": heading,
        "heading_path": heading_path,
        "tokens": tokens,
        "score": h.score,
    });
    if let Some(snip) = &h.snippet {
        v["snippet"] = serde_json::Value::String(snip.clone());
    }
    if include_content {
        let raw = hit_content(conn, h)?;
        if let Some(max) = max_tokens {
            let t = crate::token::truncate(&raw, 0, max);
            v["content"] = serde_json::Value::String(t.text);
            v["truncated"] = serde_json::json!(t.truncated);
        } else {
            v["content"] = serde_json::Value::String(raw);
        }
    }
    Ok(v)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

impl SearchIndex {
    /// Open (or create) the index at `dir`. Re-registers `mdpeek_en`.
    pub(crate) fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create tantivy dir {}", dir.display()))?;
        let schema = build_schema();
        let index = if dir.read_dir()?.next().is_some() {
            Index::open_in_dir(dir).with_context(|| format!("open {}", dir.display()))?
        } else {
            Index::create_in_dir(dir, schema.clone())
                .with_context(|| format!("create {}", dir.display()))?
        };
        register_analyzer(&index);
        let writer: IndexWriter = open_writer(&index)?;
        let s = index.schema();
        Ok(Self {
            f_doc_id: s.get_field("doc_id")?,
            f_section_id: s.get_field("section_id")?,
            f_path: s.get_field("path")?,
            f_dir: s.get_field("dir")?,
            f_heading: s.get_field("heading")?,
            f_heading_path: s.get_field("heading_path")?,
            f_body: s.get_field("body")?,
            f_tokens: s.get_field("tokens")?,
            f_mtime: s.get_field("mtime")?,
            index,
            writer,
        })
    }

    /// Replace all sections for `doc_id` with `sections`. Caller must
    /// invoke `commit` afterwards (typically once per sync batch).
    pub(crate) fn upsert_sections(
        &self,
        doc_id: &str,
        path: &str,
        dir: &str,
        mtime: u64,
        sections: &[Section],
    ) -> Result<()> {
        self.writer
            .delete_term(Term::from_field_text(self.f_doc_id, doc_id));
        for sec in sections {
            let body = std::str::from_utf8(&sec.content).unwrap_or("");
            let mut d = TantivyDocument::default();
            d.add_text(self.f_doc_id, doc_id);
            d.add_text(self.f_section_id, &sec.id);
            d.add_text(self.f_path, path);
            d.add_text(self.f_dir, dir);
            d.add_text(self.f_heading, &sec.heading);
            d.add_text(self.f_heading_path, &sec.heading_path);
            d.add_text(self.f_body, body);
            d.add_u64(self.f_tokens, sec.tokens as u64);
            d.add_u64(self.f_mtime, mtime);
            self.writer.add_document(d)?;
        }
        Ok(())
    }

    /// Remove all sections for `doc_id`. Caller must commit.
    pub(crate) fn delete_doc(&self, doc_id: &str) {
        self.writer
            .delete_term(Term::from_field_text(self.f_doc_id, doc_id));
    }

    /// Flush pending writes to disk.
    pub(crate) fn commit(&mut self) -> Result<()> {
        self.writer.commit()?;
        Ok(())
    }

    /// Query the index. Heading field boosted 3×.
    pub(crate) fn query(&self, query: &str, limit: usize, snippet: bool) -> Result<Vec<Hit>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let mut qp = QueryParser::for_index(&self.index, vec![self.f_heading, self.f_body]);
        qp.set_field_boost(self.f_heading, HEADING_BOOST);
        let q = qp.parse_query(query).context("parse query")?;
        let top = searcher.search(&q, &TopDocs::with_limit(limit).order_by_score())?;

        let snippet_gen = if snippet {
            let mut sg = tantivy::snippet::SnippetGenerator::create(&searcher, &*q, self.f_body)?;
            sg.set_max_num_chars(200);
            Some(sg)
        } else {
            None
        };

        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let d: TantivyDocument = searcher.doc(addr)?;
            let snip = snippet_gen.as_ref().map(|sg| {
                let s = sg.snippet_from_doc(&d);
                s.fragment().to_string()
            });
            hits.push(Hit {
                doc_id: first_text(&d, self.f_doc_id).unwrap_or_default(),
                section_id: first_text(&d, self.f_section_id).unwrap_or_default(),
                score,
                snippet: snip,
            });
        }
        Ok(hits)
    }
}

fn first_text(d: &TantivyDocument, f: Field) -> Option<String> {
    use tantivy::schema::Value;
    d.get_first(f).and_then(|v| v.as_str().map(String::from))
}

/// Acquire the tantivy IndexWriter, retrying on LockBusy for up to ~3s.
fn open_writer(index: &Index) -> Result<IndexWriter> {
    use backon::{BlockingRetryable, ExponentialBuilder};
    use tantivy::TantivyError;
    use tantivy::directory::error::LockError;

    let backoff = ExponentialBuilder::new()
        .with_min_delay(std::time::Duration::from_millis(100))
        .with_max_delay(std::time::Duration::from_secs(1))
        .with_total_delay(Some(std::time::Duration::from_secs(3)));

    (|| index.writer(WRITER_MEM))
        .retry(backoff)
        .when(|e| matches!(e, TantivyError::LockFailure(LockError::LockBusy, _)))
        .call()
        .context("acquire tantivy index writer")
}

fn build_schema() -> Schema {
    let mut b: SchemaBuilder = Schema::builder();
    let tokenized: TextOptions = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(ANALYZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    // body: tokenized and stored (stored needed for snippet generation)
    let body_opts: TextOptions = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(ANALYZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    let u64_fast_stored: NumericOptions = NumericOptions::default().set_stored().set_fast();
    b.add_text_field("doc_id", STRING | STORED);
    b.add_text_field("section_id", STRING | STORED);
    b.add_text_field("path", STRING | STORED);
    b.add_text_field("dir", STRING | STORED | FAST);
    b.add_text_field("heading", tokenized.clone());
    b.add_text_field("heading_path", STRING | STORED);
    b.add_text_field("body", body_opts);
    b.add_u64_field("tokens", u64_fast_stored.clone());
    b.add_u64_field("mtime", u64_fast_stored);
    b.build()
}

fn register_analyzer(index: &Index) {
    let analyzer: TextAnalyzer = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .filter(StopWordFilter::new(Language::English).expect("english stopwords"))
        .filter(Stemmer::new(Language::English))
        .build();
    index.tokenizers().register(ANALYZER, analyzer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::Section;
    use tempfile::TempDir;

    fn sec(id: &str, heading: &str, body: &str, tokens: usize) -> Section {
        let mut content = format!("# {heading}\n\n{body}\n").into_bytes();
        // content hash isn't meaningful to these tests; we set id explicitly.
        content.shrink_to_fit();
        Section {
            id: id.into(),
            level: 1,
            heading: heading.into(),
            heading_path: String::new(),
            content,
            snippet: body.into(),
            tokens,
            code_tokens: 0,
        }
    }

    #[test]
    fn open_create_and_reopen() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("tantivy");
        {
            let _ = SearchIndex::open(&dir).unwrap();
        }
        // Reopen the existing index without error; analyzer re-registers.
        let _ = SearchIndex::open(&dir).unwrap();
    }

    #[test]
    fn upsert_then_query_returns_hit() {
        let tmp = TempDir::new().unwrap();
        let mut idx = SearchIndex::open(&tmp.path().join("t")).unwrap();
        let secs = vec![sec("s1abcd", "Install", "run cargo install mdpeek", 5)];
        idx.upsert_sections("deadbeef", "docs/install.md", "docs", 100, &secs)
            .unwrap();
        idx.commit().unwrap();
        let hits = idx.query("cargo", 10, false).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, "deadbeef");
        assert_eq!(hits[0].section_id, "s1abcd");
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn heading_hits_outrank_body_hits() {
        let tmp = TempDir::new().unwrap();
        let mut idx = SearchIndex::open(&tmp.path().join("t")).unwrap();
        idx.upsert_sections(
            "aaaaaaaa",
            "docs/a.md",
            "docs",
            1,
            &[sec(
                "aaaaaa",
                "plain heading",
                "the rocket word shows up in body",
                10,
            )],
        )
        .unwrap();
        idx.upsert_sections(
            "bbbbbbbb",
            "docs/b.md",
            "docs",
            1,
            &[sec(
                "bbbbbb",
                "rocket launch",
                "nothing special in the body",
                10,
            )],
        )
        .unwrap();
        idx.commit().unwrap();
        let hits = idx.query("rocket", 10, false).unwrap();
        assert!(hits.len() >= 2);
        assert_eq!(hits[0].doc_id, "bbbbbbbb", "heading match should win");
    }

    #[test]
    fn delete_doc_removes_hits() {
        let tmp = TempDir::new().unwrap();
        let mut idx = SearchIndex::open(&tmp.path().join("t")).unwrap();
        idx.upsert_sections(
            "aaaaaaaa",
            "a.md",
            "",
            1,
            &[sec("aaaaaa", "unique-token-xyz", "body", 1)],
        )
        .unwrap();
        idx.commit().unwrap();
        assert_eq!(idx.query("unique-token-xyz", 10, false).unwrap().len(), 1);
        idx.delete_doc("aaaaaaaa");
        idx.commit().unwrap();
        assert_eq!(idx.query("unique-token-xyz", 10, false).unwrap().len(), 0);
    }

    #[test]
    fn upsert_replaces_prior_sections() {
        let tmp = TempDir::new().unwrap();
        let mut idx = SearchIndex::open(&tmp.path().join("t")).unwrap();
        idx.upsert_sections(
            "aaaaaaaa",
            "a.md",
            "",
            1,
            &[sec("v1v1v1", "H", "old-token-qqq", 1)],
        )
        .unwrap();
        idx.commit().unwrap();
        idx.upsert_sections(
            "aaaaaaaa",
            "a.md",
            "",
            1,
            &[sec("v2v2v2", "H", "new-token-www", 1)],
        )
        .unwrap();
        idx.commit().unwrap();
        assert_eq!(idx.query("old-token-qqq", 10, false).unwrap().len(), 0);
        let hits = idx.query("new-token-www", 10, false).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].section_id, "v2v2v2");
    }

    #[test]
    fn english_stemming_matches_variants() {
        let tmp = TempDir::new().unwrap();
        let mut idx = SearchIndex::open(&tmp.path().join("t")).unwrap();
        idx.upsert_sections(
            "aaaaaaaa",
            "a.md",
            "",
            1,
            &[sec("aaaaaa", "Running", "jumping over fences", 1)],
        )
        .unwrap();
        idx.commit().unwrap();
        // Stemmer should reduce "jumping" -> "jump", matching query "jump".
        assert!(!idx.query("jump", 10, false).unwrap().is_empty());
        // And heading "Running" should match "run".
        assert!(!idx.query("run", 10, false).unwrap().is_empty());
    }
}
