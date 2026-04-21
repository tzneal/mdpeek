use sha2::{Digest, Sha256};

/// 8-hex-char doc ID derived from the repo-relative path.
/// Stable across content changes; changes on rename.
pub fn doc_id(rel_path: &str) -> String {
    hex_prefix(rel_path.as_bytes(), 8)
}

/// 6-hex-char section ID derived from the section's raw content bytes
/// (heading line through end of section).
/// Stable across moves; changes on content edit.
pub fn section_id(content: &[u8]) -> String {
    hex_prefix(content, 6)
}

fn hex_prefix(bytes: &[u8], hex_chars: usize) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut out = String::with_capacity(hex_chars);
    // Each byte -> 2 hex chars. Take ceil(hex_chars/2) bytes.
    let bytes_needed = hex_chars.div_ceil(2);
    for b in &digest[..bytes_needed] {
        use std::fmt::Write;
        write!(&mut out, "{b:02x}").unwrap();
    }
    out.truncate(hex_chars);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_id_is_8_hex() {
        let id = doc_id("docs/design.md");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn doc_id_is_stable() {
        assert_eq!(doc_id("a/b.md"), doc_id("a/b.md"));
    }

    #[test]
    fn doc_id_differs_by_path() {
        assert_ne!(doc_id("a/b.md"), doc_id("a/c.md"));
    }

    #[test]
    fn section_id_is_6_hex() {
        let id = section_id(b"# Heading\nbody text\n");
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn section_id_differs_by_content() {
        assert_ne!(section_id(b"# A\nbody\n"), section_id(b"# B\nbody\n"));
    }
}
