use tiktoken_rs::cl100k_base_singleton;

/// Count tokens for `text` using the `cl100k_base` encoding
/// (GPT-4 / GPT-3.5; close approximation for Claude budgeting).
///
/// Uses a global singleton, so repeated calls are cheap.
pub fn count(text: &str) -> usize {
    cl100k_base_singleton().encode_ordinary(text).len()
}

/// Result of truncating text to a token budget.
pub struct Truncated {
    pub text: String,
    pub total_tokens: usize,
    pub truncated: bool,
}

/// Skip `start` tokens, then take up to `max` tokens from `text`.
/// Returns the decoded text slice and whether it was truncated.
pub fn truncate(text: &str, start: usize, max: usize) -> Truncated {
    let bpe = cl100k_base_singleton();
    let tokens = bpe.encode_ordinary(text);
    let total_tokens = tokens.len();
    let begin = start.min(total_tokens);
    let end = (begin + max).min(total_tokens);
    let truncated = end < total_tokens;
    let slice = &tokens[begin..end];
    let decoded = if slice.is_empty() {
        String::new()
    } else {
        bpe.decode(slice).unwrap_or_default()
    };
    Truncated {
        text: decoded,
        total_tokens,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(count(""), 0);
    }

    #[test]
    fn ascii_hello_world() {
        // "hello world" encodes to 2 tokens in cl100k_base.
        assert_eq!(count("hello world"), 2);
    }

    #[test]
    fn longer_text_scales() {
        let short = count("the quick brown fox");
        let long = count(&"the quick brown fox ".repeat(10));
        assert!(
            long > short * 5,
            "expected ~10x tokens, got {short} vs {long}"
        );
    }

    #[test]
    fn truncate_within_budget_not_truncated() {
        let t = truncate("hello world", 0, 100);
        assert_eq!(t.text, "hello world");
        assert!(!t.truncated);
        assert_eq!(t.total_tokens, 2);
    }

    #[test]
    fn truncate_over_budget_is_truncated() {
        let long = "the quick brown fox jumps over the lazy dog";
        let total = count(long);
        let t = truncate(long, 0, 2);
        assert!(t.truncated);
        assert!(count(&t.text) <= 2);
        assert_eq!(t.total_tokens, total);
    }

    #[test]
    fn truncate_start_skips_tokens() {
        let text = "alpha bravo charlie delta echo";
        let full = truncate(text, 0, 100);
        let skipped = truncate(text, 2, 100);
        assert!(skipped.text.len() < full.text.len());
        assert!(!skipped.text.starts_with("alpha"));
    }

    #[test]
    fn truncate_start_past_end_returns_empty() {
        let t = truncate("hello world", 999, 100);
        assert_eq!(t.text, "");
        assert!(!t.truncated);
    }
}
