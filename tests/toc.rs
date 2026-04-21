mod common;

use common::Env;
use predicates::prelude::*;

#[test]
fn toc_lists_all_docs_and_headings() {
    let env = Env::new();
    env.write("a.md", "# A\n\n## A1\nbody\n");
    env.write("docs/b.md", "# B\n\n## B1\nbody\n");

    env.cmd()
        .arg("toc")
        .assert()
        .success()
        .stdout(predicate::str::contains("a.md"))
        .stdout(predicate::str::contains("docs/b.md"))
        .stdout(predicate::str::contains("# A"))
        .stdout(predicate::str::contains("## A1"))
        .stdout(predicate::str::contains("# B"))
        .stdout(predicate::str::contains("## B1"));
}

#[test]
fn toc_json_shape() {
    let env = Env::new();
    env.write("a.md", "# Top\n\n## Sub\nbody\n");

    let out = env.cmd().args(["--json", "toc"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let doc = &arr[0];
    assert_eq!(doc["path"], "a.md");
    assert!(doc["total_tokens"].as_i64().unwrap() > 0);
    let secs = doc["sections"].as_array().unwrap();
    assert_eq!(secs.len(), 2);
    assert_eq!(secs[0]["heading"], "Top");
    assert_eq!(secs[0]["level"], 1);
    assert_eq!(secs[1]["heading"], "Sub");
    assert_eq!(secs[1]["heading_path"], "Top");
    // Snippets deliberately omitted from toc output.
    assert!(secs[0].get("snippet").is_none());
}
