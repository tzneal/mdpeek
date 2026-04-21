mod common;

use common::Env;
use predicates::prelude::*;

#[test]
fn outline_shows_sections_with_ids() {
    let env = Env::new();
    env.write(
        "docs/guide.md",
        "# Guide\n\nIntro text.\n\n## Install\n\nRun cargo install.\n\n## Usage\n\nDo stuff.\n",
    );

    // First index so we can get the doc id.
    let out = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    env.cmd()
        .arg("outline")
        .arg(&doc_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("Guide"))
        .stdout(predicate::str::contains("Install"))
        .stdout(predicate::str::contains("Usage"))
        .stdout(predicate::str::contains("[sec:"));
}

#[test]
fn outline_prefix_match_works() {
    let env = Env::new();
    env.write("a.md", "# Title\n\nBody.\n");

    let out = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let full_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let prefix = &full_id[..4];

    let out = env
        .cmd()
        .arg("--json")
        .arg("outline")
        .arg(prefix)
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["doc_id"].as_str().unwrap(), full_id);
}

#[test]
fn outline_bad_id_errors() {
    let env = Env::new();
    env.write("a.md", "# A\n");
    env.cmd().arg("index").assert().success();

    env.cmd()
        .arg("outline")
        .arg("zzzzzzzz")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no doc"));
}

#[test]
fn outline_json_has_sections_array() {
    let env = Env::new();
    env.write("a.md", "# Top\n\nHello.\n\n## Sub\n\nWorld.\n");

    let out = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = env
        .cmd()
        .arg("--json")
        .arg("outline")
        .arg(&doc_id)
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let secs = v["sections"].as_array().unwrap();
    assert_eq!(secs.len(), 2);
    assert_eq!(secs[0]["heading"], "Top");
    assert_eq!(secs[1]["heading"], "Sub");
    assert!(secs[0]["tokens"].as_i64().unwrap() > 0);
}

#[test]
fn outline_by_path() {
    let env = Env::new();
    env.write("docs/guide.md", "# Guide\n\n## Setup\nDo stuff.\n");
    env.cmd().arg("index").assert().success();

    env.cmd()
        .arg("outline")
        .arg("docs/guide.md")
        .assert()
        .success()
        .stdout(predicate::str::contains("Guide"))
        .stdout(predicate::str::contains("Setup"));
}

#[test]
fn outline_reports_code_tokens_for_fenced_blocks() {
    let env = Env::new();
    env.write(
        "a.md",
        "# Top\n\nProse paragraph.\n\n```rust\nfn main() { println!(\"hello\"); }\nlet x = 42;\n```\n",
    );
    let out = env.cmd().args(["--json", "index"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = env
        .cmd()
        .args(["--json", "outline", &doc_id])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let sec = &v["sections"][0];
    let code = sec["code_tokens"].as_i64().unwrap();
    let total = sec["tokens"].as_i64().unwrap();
    assert!(code > 0, "expected code_tokens > 0, got {code}");
    assert!(
        code < total,
        "code ({code}) should be subset of total ({total})"
    );
}

#[test]
fn outline_code_tokens_zero_for_prose_only() {
    let env = Env::new();
    env.write("a.md", "# Top\n\nJust prose, no fences.\n");
    let out = env.cmd().args(["--json", "index"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = env
        .cmd()
        .args(["--json", "outline", &doc_id])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["sections"][0]["code_tokens"].as_i64().unwrap(), 0);
}
