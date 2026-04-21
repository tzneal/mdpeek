//! Shared test harness for mdpeek end-to-end tests.
//!
//! Each test creates an isolated fake repo + XDG cache so state
//! doesn't leak between runs.

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub struct Env {
    pub repo: TempDir,
    pub xdg: TempDir,
}

impl Env {
    /// Create a fresh fake repo (with `.git/`) and isolated XDG cache.
    pub fn new() -> Self {
        let repo = TempDir::new().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let xdg = TempDir::new().unwrap();
        Self { repo, xdg }
    }

    /// Write `content` to `rel_path` under the fake repo, creating parents.
    pub(crate) fn write(&self, rel_path: &str, content: &str) -> PathBuf {
        let path = self.repo.path().join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Build a `mdpeek` command rooted at the fake repo with isolated XDG.
    pub fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("mdpeek").unwrap();
        c.current_dir(self.repo.path())
            .env("XDG_CACHE_HOME", self.xdg.path())
            // Defend against the user's real config leaking in.
            .env_remove("HOME");
        c
    }

    pub(crate) fn repo_path(&self) -> &Path {
        self.repo.path()
    }
}
