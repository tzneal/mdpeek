use crate::id::section_id;
use crate::token::count as count_tokens;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};

/// One indexable slice of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    /// 0 = synthetic root (pre-first-heading or `.txt` whole-file),
    /// 1..=6 = H1..=H6.
    pub level: u8,
    /// Heading text, or filename stem for the root section.
    pub heading: String,
    /// Breadcrumb of parent headings, e.g. "Install > macOS".
    /// Empty for top-level or root sections.
    pub heading_path: String,
    /// Raw bytes of the section (heading line through end of section).
    pub content: Vec<u8>,
    /// First sentence, ≤25 words, code/HTML stripped.
    pub snippet: String,
    /// Token count of `content`.
    pub tokens: usize,
    /// Token count of fenced code-block content within `content`
    /// (fence markers excluded). Zero when the section has no fences.
    pub code_tokens: usize,
}

/// Parse a document's bytes into a flat list of sections, in source order.
///
/// `filename_stem` is used as the heading for synthetic root sections
/// (pre-first-heading markdown, or entire `.txt` files).
/// `is_markdown` selects the parser; `false` treats the entire input as
/// one root section.
pub fn parse(bytes: &[u8], filename_stem: &str, is_markdown: bool) -> Vec<Section> {
    if !is_markdown {
        return vec![make_section(0, filename_stem, "", bytes)];
    }

    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return Vec::new(), // skip non-utf8 silently
    };

    // Collect heading boundaries: (byte_offset, level, heading_text).
    let mut boundaries: Vec<(usize, u8, String)> = Vec::new();
    let parser = Parser::new(text).into_offset_iter();
    let mut cur: Option<(usize, u8, String)> = None;
    for (event, range) in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                cur = Some((range.start, heading_level_to_u8(level), String::new()));
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, _, s)) = cur.as_mut() {
                    s.push_str(&t);
                }
            }
            Event::End(_) if cur.is_some() => {
                let (start, level, heading) = cur.take().unwrap();
                if !heading.is_empty() {
                    boundaries.push((start, level, heading));
                }
            }
            _ => {}
        }
    }

    // Walk boundaries to build sections + heading_path breadcrumbs.
    let mut sections = Vec::new();
    let mut stack: Vec<(u8, String)> = Vec::new();

    // Content before the first heading -> synthetic root section.
    let first = boundaries.first().map(|b| b.0).unwrap_or(bytes.len());
    if first > 0 {
        sections.push(make_section(0, filename_stem, "", &bytes[..first]));
    }

    for i in 0..boundaries.len() {
        let (start, level, ref heading) = boundaries[i];
        let end = boundaries.get(i + 1).map(|b| b.0).unwrap_or(bytes.len());

        while let Some(&(l, _)) = stack.last() {
            if l >= level {
                stack.pop();
            } else {
                break;
            }
        }
        let path = stack
            .iter()
            .map(|(_, h)| h.as_str())
            .collect::<Vec<_>>()
            .join(" > ");

        sections.push(make_section(level, heading, &path, &bytes[start..end]));
        stack.push((level, heading.clone()));
    }

    sections
}

fn heading_level_to_u8(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn make_section(level: u8, heading: &str, heading_path: &str, content: &[u8]) -> Section {
    let text = std::str::from_utf8(content).unwrap_or("");
    Section {
        id: section_id(content),
        level,
        heading: heading.trim().to_string(),
        heading_path: heading_path.trim().to_string(),
        content: content.to_vec(),
        snippet: extract_snippet(text),
        tokens: count_tokens(text),
        code_tokens: count_code_tokens(text),
    }
}

/// Sum of tokens inside fenced code blocks (fence markers excluded).
fn count_code_tokens(text: &str) -> usize {
    let mut buf = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if buf.is_empty() {
        0
    } else {
        count_tokens(&buf)
    }
}

/// First sentence of a section body, ≤25 words.
/// - Skips the heading line.
/// - Strips fenced code blocks and HTML tags.
/// - Terminates at the first `.`, `!`, `?`, or blank line.
fn extract_snippet(text: &str) -> String {
    // Drop the heading line if present.
    let body = if text.starts_with('#') {
        text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        text
    };

    let mut out = String::new();
    let mut in_fence = false;
    let mut saw_blank = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if line.trim().is_empty() {
            if !out.is_empty() {
                saw_blank = true;
                break;
            }
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line.trim());
        if find_sentence_end(&out).is_some() {
            break;
        }
    }
    let _ = saw_blank;

    // Fallback: if body was all code fences, use first line from the fence.
    if out.is_empty() {
        let mut in_f = false;
        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_f = !in_f;
                continue;
            }
            if in_f && !line.trim().is_empty() {
                out = line.trim().to_string();
                break;
            }
        }
    }

    // Strip inline HTML tags.
    let out = strip_html(&out);

    // Cut at first sentence terminator (keeping the terminator).
    // A terminator is `.`, `!`, or `?` followed by whitespace, end-of-string,
    // or a closing quote/paren — avoids false cuts on `1.`, code spans, etc.
    let cut = find_sentence_end(&out).unwrap_or(out.len());
    let sentence = &out[..cut];

    // Word cap at 25, append ellipsis if truncated.
    let words: Vec<&str> = sentence.split_whitespace().collect();
    if words.len() > 25 {
        format!("{}…", words[..25].join(" "))
    } else {
        words.join(" ")
    }
}

/// Find the byte position just past the first real sentence terminator.
/// A terminator is `.`, `!`, or `?` where the next char is whitespace,
/// end-of-string, `)`, `"`, `'`, or `` ` ``. This avoids cutting on
/// `1.`, inline code paths, or abbreviations like `e.g.`.
fn find_sentence_end(s: &str) -> Option<usize> {
    let terminators = ['.', '!', '?'];
    let mut in_backtick = false;
    for (i, c) in s.char_indices() {
        if c == '`' {
            in_backtick = !in_backtick;
            continue;
        }
        if in_backtick {
            continue;
        }
        if terminators.contains(&c) {
            let after = i + c.len_utf8();
            let next = s[after..].chars().next();
            // Skip list markers like "1." — digit immediately before dot.
            if c == '.' {
                let prev = s[..i].chars().next_back();
                if prev.map(|p| p.is_ascii_digit()).unwrap_or(false) {
                    continue;
                }
            }
            match next {
                None | Some(' ') | Some('\t') | Some(')') | Some('"') | Some('\'') | Some('`') => {
                    return Some(after);
                }
                _ => {}
            }
        }
    }
    None
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_file_is_single_root_section() {
        let secs = parse(b"just some text\nmore text", "notes", false);
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].level, 0);
        assert_eq!(secs[0].heading, "notes");
        assert_eq!(secs[0].heading_path, "");
    }

    #[test]
    fn markdown_with_headings_splits() {
        let md = b"# Intro\nhello world.\n\n## Install\nrun cargo install.\n\n## Usage\ndo stuff.";
        let secs = parse(md, "readme", true);
        let headings: Vec<_> = secs.iter().map(|s| (s.level, s.heading.as_str())).collect();
        assert_eq!(headings, vec![(1, "Intro"), (2, "Install"), (2, "Usage")]);
    }

    #[test]
    fn heading_path_tracks_nesting() {
        let md = b"# A\n\n## B\n\n### C\n\n## D\n";
        let secs = parse(md, "x", true);
        assert_eq!(secs[0].heading_path, "");
        assert_eq!(secs[1].heading_path, "A");
        assert_eq!(secs[2].heading_path, "A > B");
        assert_eq!(secs[3].heading_path, "A");
    }

    #[test]
    fn preamble_becomes_root_section() {
        let md = b"intro paragraph.\n\n# First\nbody\n";
        let secs = parse(md, "readme", true);
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].level, 0);
        assert_eq!(secs[0].heading, "readme");
        assert_eq!(secs[1].level, 1);
        assert_eq!(secs[1].heading, "First");
    }

    #[test]
    fn snippet_skips_heading_and_takes_first_sentence() {
        let md = b"# Title\n\nFirst sentence here. Second one.\n";
        let secs = parse(md, "x", true);
        assert_eq!(secs[0].snippet, "First sentence here.");
    }

    #[test]
    fn snippet_strips_code_fence() {
        let md = b"# T\n\n```\nlet x = 1;\n```\n\nReal sentence now.\n";
        let secs = parse(md, "x", true);
        assert_eq!(secs[0].snippet, "Real sentence now.");
    }

    #[test]
    fn snippet_word_caps_at_25() {
        let body: String = "word ".repeat(40);
        let md = format!("# T\n\n{body}\n");
        let secs = parse(md.as_bytes(), "x", true);
        let snippet = &secs[0].snippet;
        let count = snippet.split_whitespace().count();
        assert_eq!(count, 25, "got: {snippet}");
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn snippet_ignores_dots_in_code_spans_and_list_markers() {
        // Backtick code span with a dot should not terminate the snippet.
        let md = b"# T\n\nRun `mdpeek ignore .` to add patterns.\n";
        let secs = parse(md, "x", true);
        assert_eq!(secs[0].snippet, "Run `mdpeek ignore .` to add patterns.");

        // Numbered list marker should not terminate.
        let md2 = b"# T\n\n1. First item of the list here.\n";
        let secs2 = parse(md2, "x", true);
        assert!(
            secs2[0].snippet.contains("First item"),
            "got: {}",
            secs2[0].snippet
        );
    }

    #[test]
    fn section_ids_are_stable_and_unique() {
        let md = b"# A\nbody a\n\n# B\nbody b\n";
        let secs = parse(md, "x", true);
        assert_eq!(secs[0].id.len(), 6);
        assert_ne!(secs[0].id, secs[1].id);
        // Re-parse: same IDs.
        let again = parse(md, "x", true);
        assert_eq!(secs[0].id, again[0].id);
    }

    #[test]
    fn token_counts_populated() {
        let secs = parse(b"# T\n\nsome text", "x", true);
        assert!(secs[0].tokens > 0);
    }

    #[test]
    fn code_tokens_counts_only_fenced_body() {
        let md = b"# T\n\nprose here.\n\n```\nlet x = 1;\nlet y = 2;\n```\n\nafter.\n";
        let secs = parse(md, "x", true);
        assert!(secs[0].code_tokens > 0);
        assert!(secs[0].code_tokens < secs[0].tokens);
    }

    #[test]
    fn code_tokens_zero_without_fences() {
        let secs = parse(b"# T\n\nprose only.\n", "x", true);
        assert_eq!(secs[0].code_tokens, 0);
    }

    #[test]
    fn code_tokens_zero_for_txt_files() {
        let secs = parse(b"plain text only\n", "notes", false);
        assert_eq!(secs[0].code_tokens, 0);
    }
}
