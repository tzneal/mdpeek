mod common;

use common::Env;
use predicates::prelude::*;

#[test]
fn show_full_doc_returns_content() {
    let env = Env::new();
    env.write("a.md", "# Hello\n\nWorld.\n");

    let idx = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&idx.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    env.cmd()
        .arg("show")
        .arg(&doc_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("# Hello"))
        .stdout(predicate::str::contains("World."));
}

#[test]
fn show_section_by_id() {
    let env = Env::new();
    env.write("a.md", "# Top\n\nIntro.\n\n## Sub\n\nDetails.\n");

    let idx = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&idx.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let outline = env
        .cmd()
        .arg("--json")
        .arg("outline")
        .arg(&doc_id)
        .output()
        .unwrap();
    let o: serde_json::Value = serde_json::from_slice(&outline.stdout).unwrap();
    let sec_id = o["sections"][1]["id"].as_str().unwrap().to_string();

    let out = env
        .cmd()
        .arg("show")
        .arg(format!("{doc_id}:{sec_id}"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("## Sub"));
    assert!(text.contains("Details."));
    assert!(!text.contains("# Top"));
}

#[test]
fn show_json_has_content_and_tokens() {
    let env = Env::new();
    env.write("a.md", "# A\n\nBody.\n");

    let idx = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&idx.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = env
        .cmd()
        .arg("--json")
        .arg("show")
        .arg(&doc_id)
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["content"].as_str().unwrap().contains("Body."));
    assert!(v["tokens"].as_i64().unwrap() > 0);
    assert_eq!(v["doc_id"].as_str().unwrap(), doc_id);
}

#[test]
fn show_bad_section_id_errors() {
    let env = Env::new();
    env.write("a.md", "# A\n");

    let idx = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&idx.stdout).unwrap();
    let doc_id = v["groups"][0]["docs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    env.cmd()
        .arg("show")
        .arg(format!("{doc_id}:zzzzzz"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("no section"));
}

#[test]
fn show_by_path() {
    let env = Env::new();
    env.write("docs/guide.md", "# Guide\n\nHello world.\n");
    env.cmd().arg("index").assert().success();

    env.cmd()
        .arg("show")
        .arg("docs/guide.md")
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello world"));
}

#[test]
fn show_json_max_tokens_truncates_and_adds_fields() {
    let env = Env::new();
    // Create a doc with enough content to exceed a small token budget.
    let body = "word ".repeat(200);
    env.write("big.md", &format!("# Big Doc\n\n{body}\n"));
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .args(["--json", "show", "big.md", "--max-tokens", "10"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["truncated"], true);
    assert_eq!(v["start_token"], 0);
    assert_eq!(v["end_token"], 10);
    // The original token count should be much larger than 10.
    assert!(v["tokens"].as_i64().unwrap() > 10);
}

#[test]
fn show_json_max_tokens_with_start_token_pages() {
    let env = Env::new();
    let body = "word ".repeat(200);
    env.write("big.md", &format!("# Big Doc\n\n{body}\n"));
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .args([
            "--json",
            "show",
            "big.md",
            "--max-tokens",
            "10",
            "--start-token",
            "5",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["start_token"], 5);
    assert_eq!(v["end_token"], 15);
    assert_eq!(v["truncated"], true);
}

#[test]
fn show_json_without_max_tokens_omits_budget_fields() {
    let env = Env::new();
    env.write("a.md", "# A\n\nHello.\n");
    env.cmd().arg("index").assert().success();

    let out = env.cmd().args(["--json", "show", "a.md"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.get("truncated").is_none());
    assert!(v.get("start_token").is_none());
    assert!(v.get("end_token").is_none());
}

#[test]
fn show_text_max_tokens_truncates_output() {
    let env = Env::new();
    let body = "word ".repeat(200);
    env.write("big.md", &format!("# Big Doc\n\n{body}\n"));
    env.cmd().arg("index").assert().success();

    let full = env.cmd().args(["show", "big.md"]).output().unwrap();
    let truncated = env
        .cmd()
        .args(["show", "big.md", "--max-tokens", "5"])
        .output()
        .unwrap();
    assert!(truncated.stdout.len() < full.stdout.len());
}

#[test]
fn show_no_code_replaces_fenced_blocks() {
    let env = Env::new();
    env.write(
        "a.md",
        "# Top\n\nProse line.\n\n```rust\nfn main() {}\nlet x = 1;\n```\n\nAfter.\n",
    );
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .args(["show", "a.md", "--no-code"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !text.contains("fn main"),
        "code body should be stripped: {text}"
    );
    assert!(
        !text.contains("```"),
        "fence markers should be stripped: {text}"
    );
    assert!(
        text.contains("[code: 2 lines]"),
        "placeholder missing: {text}"
    );
    assert!(text.contains("Prose line."));
    assert!(text.contains("After."));
}

#[test]
fn show_no_code_json_strips_code_in_content() {
    let env = Env::new();
    env.write("a.md", "# T\n\n```\nsecret code\n```\n\nprose.\n");
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .args(["--json", "show", "a.md", "--no-code"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let content = v["content"].as_str().unwrap();
    assert!(!content.contains("secret code"));
    assert!(content.contains("[code: 1 lines]"));
}
