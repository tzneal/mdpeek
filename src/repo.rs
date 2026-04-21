use anyhow::{Context, Result};
use ignore::WalkBuilder;
use ignore::gitignore::GitignoreBuilder;
use std::path::{Path, PathBuf};
/// Walk up from `start` to the nearest directory containing `.git`.
/// Returns `start` (canonicalized) if no `.git` is found.
pub fn find_root(start: &Path) -> Result<PathBuf> {
    let start = start
        .canonicalize()
        .with_context(|| format!("path does not exist: {}", start.display()))?;
    for dir in start.ancestors() {
        if dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
    }
    Ok(start)
}

/// Walk `root` for `.md`, `.mdx`, and `.txt` files, respecting `.gitignore`
/// and skipping common build/vendor directories. Symlinks are not followed.
///
/// `user_ignores` is a list of additional gitignore-format patterns,
/// typically pulled from the per-repo sqlite `user_ignores` table.
///
/// Returns absolute paths.
pub fn walk_docs(root: &Path, user_ignores: &[String]) -> Result<Vec<PathBuf>> {
    const EXTS: &[&str] = &["md", "mdx", "txt"];
    const EXTRA_IGNORES: &[&str] = &["node_modules", "vendor", "target", "dist", "build"];

    let mut builder = WalkBuilder::new(root);
    builder.follow_links(false).hidden(true).git_ignore(true);
    builder.filter_entry(|e| {
        !EXTRA_IGNORES
            .iter()
            .any(|ig| e.file_name() == std::ffi::OsStr::new(ig))
    });

    // User-ignore patterns: built in-memory, rooted at the repo, so
    // paths like `drafts/**` match repo-relative paths.
    let user_matcher = if user_ignores.is_empty() {
        None
    } else {
        let mut b = GitignoreBuilder::new(root);
        for p in user_ignores {
            b.add_line(None, p)
                .with_context(|| format!("invalid user-ignore pattern: {p}"))?;
        }
        Some(b.build().context("build user-ignore matcher")?)
    };

    let mut out = Vec::new();
    for entry in builder.build() {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if let Some(m) = user_matcher.as_ref()
            && m.matched(path, false).is_ignore()
        {
            continue;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
            .unwrap_or(false);
        if ext_ok {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn find_root_walks_up_to_git_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".git")).unwrap();
        let deep = root.join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        let found = find_root(&deep).unwrap();
        assert_eq!(found.canonicalize().unwrap(), root.canonicalize().unwrap());
    }

    #[test]
    fn find_root_falls_back_to_start() {
        let tmp = TempDir::new().unwrap();
        let start = tmp.path();
        let found = find_root(start).unwrap();
        assert_eq!(found.canonicalize().unwrap(), start.canonicalize().unwrap());
    }

    #[test]
    fn walk_docs_filters_by_extension() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        touch(&root.join("a.md"));
        touch(&root.join("b.mdx"));
        touch(&root.join("c.txt"));
        touch(&root.join("d.rs"));
        touch(&root.join("sub/e.MD"));
        let docs = walk_docs(root, &[]).unwrap();
        let names: Vec<_> = docs
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["a.md", "b.mdx", "c.txt", "e.MD"]);
    }

    #[test]
    fn walk_docs_respects_gitignore() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // ignore crate requires .git or explicit .gitignore at root for
        // .gitignore to apply; simulate a repo.
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.md\n").unwrap();
        touch(&root.join("kept.md"));
        touch(&root.join("ignored.md"));
        let docs = walk_docs(root, &[]).unwrap();
        let names: Vec<_> = docs
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["kept.md"]);
    }

    #[test]
    fn walk_docs_skips_node_modules() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        touch(&root.join("README.md"));
        touch(&root.join("node_modules/pkg/readme.md"));
        let docs = walk_docs(root, &[]).unwrap();
        let names: Vec<_> = docs
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["README.md"]);
    }

    #[test]
    fn walk_docs_applies_user_ignores() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".git")).unwrap();
        touch(&root.join("keep.md"));
        touch(&root.join("drafts/a.md"));
        touch(&root.join("drafts/b.md"));
        let docs = walk_docs(root, &["drafts/**".to_string()]).unwrap();
        let names: Vec<_> = docs
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["keep.md"]);
    }
}
