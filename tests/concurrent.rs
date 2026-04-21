mod common;

use common::Env;
use std::process::Command;

/// Run two `mdpeek index` processes concurrently against the same repo+cache.
/// Both should succeed — the second waits for the tantivy lock instead of failing.
#[test]
fn concurrent_index_both_succeed() {
    let env = Env::new();
    for i in 0..10 {
        env.write(&format!("doc{i}.md"), &format!("# Doc {i}\n\nbody {i}\n"));
    }

    let bin = assert_cmd::cargo::cargo_bin("mdpeek");
    let spawn = |args: &[&str]| {
        Command::new(&bin)
            .args(args)
            .current_dir(env.repo_path())
            .env("XDG_CACHE_HOME", env.xdg.path())
            .env_remove("HOME")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };

    let a = spawn(&["--json", "index"]);
    let b = spawn(&["--json", "index"]);

    let out_a = a.wait_with_output().unwrap();
    let out_b = b.wait_with_output().unwrap();

    assert!(
        out_a.status.success(),
        "process A failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out_a.stdout),
        String::from_utf8_lossy(&out_a.stderr)
    );
    assert!(
        out_b.status.success(),
        "process B failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out_b.stdout),
        String::from_utf8_lossy(&out_b.stderr)
    );
}
