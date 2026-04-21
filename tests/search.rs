mod common;

use common::Env;
use predicates::prelude::*;

#[test]
fn search_finds_matching_section() {
    let env = Env::new();
    env.write(
        "docs/install.md",
        "# Installation\n\nRun cargo install mdpeek to get started.\n",
    );
    env.write("docs/usage.md", "# Usage\n\nPass a query to search.\n");
    env.cmd().arg("index").assert().success();

    env.cmd()
        .arg("search")
        .arg("cargo install")
        .assert()
        .success()
        .stdout(predicate::str::contains("install.md"));
}

#[test]
fn search_json_returns_array() {
    let env = Env::new();
    env.write("a.md", "# Alpha\n\nUnique xylophone content.\n");
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .arg("--json")
        .arg("search")
        .arg("xylophone")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert!(arr[0]["score"].as_f64().unwrap() > 0.0);
    assert!(arr[0]["path"].as_str().unwrap().contains("a.md"));
}

#[test]
fn search_no_results_prints_message() {
    let env = Env::new();
    env.write("a.md", "# A\n\nHello.\n");
    env.cmd().arg("index").assert().success();

    env.cmd()
        .arg("search")
        .arg("zzzznonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains("No results"));
}

#[test]
fn search_limit_caps_results() {
    let env = Env::new();
    for i in 0..5 {
        env.write(
            &format!("{i}.md"),
            &format!("# Doc {i}\n\nCommon keyword here.\n"),
        );
    }
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .arg("--json")
        .arg("search")
        .arg("common keyword")
        .arg("--limit")
        .arg("2")
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.as_array().unwrap().len() <= 2);
}

#[test]
fn search_snippet_json_includes_snippet_field() {
    let env = Env::new();
    env.write("a.md", "# Alpha\n\nUnique xylophone content here.\n");
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .arg("--json")
        .arg("search")
        .arg("xylophone")
        .arg("--snippet")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let snip = arr[0]["snippet"].as_str().unwrap();
    assert!(snip.contains("xylophone"), "snippet was: {snip}");
}

#[test]
fn search_without_snippet_flag_omits_snippet() {
    let env = Env::new();
    env.write("a.md", "# Alpha\n\nUnique xylophone content.\n");
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .arg("--json")
        .arg("search")
        .arg("xylophone")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v[0].get("snippet").is_none());
}

#[test]
fn search_snippet_text_prints_snippet_line() {
    let env = Env::new();
    env.write("a.md", "# Alpha\n\nUnique xylophone content here.\n");
    env.cmd().arg("index").assert().success();

    env.cmd()
        .arg("search")
        .arg("xylophone")
        .arg("--snippet")
        .assert()
        .success()
        .stdout(predicate::str::contains("xylophone"));
}

#[test]
fn search_include_content_embeds_section() {
    let env = Env::new();
    env.write("a.md", "# Alpha\n\nUnique xylophone content here.\n");
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .arg("--json")
        .arg("search")
        .arg("xylophone")
        .arg("--include-content")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let content = arr[0]["content"].as_str().unwrap();
    assert!(content.contains("# Alpha"), "content was: {content}");
    assert!(content.contains("xylophone"), "content was: {content}");
}

#[test]
fn search_without_include_content_omits_it() {
    let env = Env::new();
    env.write("a.md", "# Alpha\n\nUnique xylophone content.\n");
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .arg("--json")
        .arg("search")
        .arg("xylophone")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v[0].get("content").is_none());
}

#[test]
fn search_include_content_max_tokens_truncates() {
    let env = Env::new();
    let body = "unique_searchterm ".repeat(200);
    env.write("big.md", &format!("# Big\n\n{body}\n"));
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .args([
            "--json",
            "search",
            "unique_searchterm",
            "--include-content",
            "--max-tokens",
            "5",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    assert_eq!(arr[0]["truncated"], true);
    // Content should be much shorter than the full section.
    let content = arr[0]["content"].as_str().unwrap();
    assert!(content.len() < body.len());
}

#[test]
fn search_include_content_without_max_tokens_no_truncated_field() {
    let env = Env::new();
    env.write("a.md", "# Alpha\n\nUnique xylophone content.\n");
    env.cmd().arg("index").assert().success();

    let out = env
        .cmd()
        .args(["--json", "search", "xylophone", "--include-content"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v[0].get("truncated").is_none());
}
