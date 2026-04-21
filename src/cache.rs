use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const REPO_HASH_HEX_LEN: usize = 16;

/// Paths for a single repo's on-disk state.
#[derive(Debug, Clone)]
pub struct RepoPaths {
    /// The per-repo root dir: `$XDG_CACHE_HOME/mdpeek/repos/<hash>/`
    #[allow(dead_code)]
    pub root: PathBuf,
    /// Sqlite file inside `root`.
    pub db: PathBuf,
    /// Tantivy index directory inside `root`.
    pub tantivy: PathBuf,
}

/// Resolve the per-repo cache paths for `repo_root`, creating the
/// directory tree if missing. Does not open the db or tantivy index.
pub fn paths_for(repo_root: &Path) -> Result<RepoPaths> {
    paths_for_in(repo_root, &cache_base()?)
}

/// Like `paths_for`, but rooted under an explicit cache base.
/// Primarily for tests.
pub fn paths_for_in(repo_root: &Path, cache_base: &Path) -> Result<RepoPaths> {
    let hash = repo_hash(repo_root);
    let root = cache_base.join("repos").join(&hash);
    std::fs::create_dir_all(&root)
        .with_context(|| format!("create cache dir {}", root.display()))?;
    let db = root.join("mdpeek.db");
    let tantivy = root.join("tantivy");
    std::fs::create_dir_all(&tantivy)
        .with_context(|| format!("create tantivy dir {}", tantivy.display()))?;
    Ok(RepoPaths { root, db, tantivy })
}

fn cache_base() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("no XDG cache dir (set XDG_CACHE_HOME or HOME)")?
        .join("mdpeek");
    Ok(dir)
}

fn repo_hash(repo_root: &Path) -> String {
    // Canonicalize if possible; otherwise hash the path as-given.
    let canonical = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut h = Sha256::new();
    h.update(canonical.to_string_lossy().as_bytes());
    let digest = h.finalize();
    use std::fmt::Write;
    let mut s = String::with_capacity(REPO_HASH_HEX_LEN);
    for b in &digest[..REPO_HASH_HEX_LEN / 2] {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn paths_for_in_creates_repo_dir_and_tantivy_dir() {
        let xdg = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let p = paths_for_in(repo.path(), xdg.path()).unwrap();
        assert!(p.root.exists());
        assert!(p.tantivy.exists());
        assert!(p.root.starts_with(xdg.path().join("repos")));
        assert_eq!(p.db.file_name().unwrap(), "mdpeek.db");
    }

    #[test]
    fn repo_hash_is_stable_per_path() {
        let tmp = TempDir::new().unwrap();
        let a = repo_hash(tmp.path());
        let b = repo_hash(tmp.path());
        assert_eq!(a, b);
        assert_eq!(a.len(), REPO_HASH_HEX_LEN);
    }

    #[test]
    fn repo_hash_differs_for_different_paths() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        assert_ne!(repo_hash(a.path()), repo_hash(b.path()));
    }

    #[test]
    fn paths_for_in_is_idempotent() {
        let xdg = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let p1 = paths_for_in(repo.path(), xdg.path()).unwrap();
        let p2 = paths_for_in(repo.path(), xdg.path()).unwrap();
        assert_eq!(p1.root, p2.root);
        assert_eq!(p1.db, p2.db);
        assert_eq!(p1.tantivy, p2.tantivy);
    }
}
