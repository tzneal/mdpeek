mod common;

use common::Env;
use predicates::prelude::*;

#[test]
fn index_lists_docs_grouped_by_dir() {
    let env = Env::new();
    env.write("README.md", "# Project\n\nRoot readme.\n");
    env.write("docs/intro.md", "# Intro\n\nHello.\n");
    env.write("docs/deep.md", "# Deep Dive\n\nStuff.\n");

    env.cmd()
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("./"))
        .stdout(predicate::str::contains("docs/"))
        .stdout(predicate::str::contains("Project"))
        .stdout(predicate::str::contains("Intro"))
        .stdout(predicate::str::contains("Deep Dive"));
}

#[test]
fn index_json_has_groups_and_totals() {
    let env = Env::new();
    env.write("docs/a.md", "# A\n\nbody.\n");
    env.write("docs/b.md", "# B\n\nbody.\n");

    let out = env.cmd().arg("--json").arg("index").output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["total_docs"], 2);
    assert!(v["total_tokens"].as_i64().unwrap() > 0);
    assert_eq!(v["groups"][0]["dir"], "docs");
    assert_eq!(v["groups"][0]["docs"].as_array().unwrap().len(), 2);
}

#[test]
fn index_picks_up_changes_on_rerun() {
    let env = Env::new();
    env.write("a.md", "# A\n");
    let v1: serde_json::Value = serde_json::from_slice(
        &env.cmd()
            .arg("--json")
            .arg("index")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(v1["total_docs"], 1);

    env.write("b.md", "# B\n");
    let v2: serde_json::Value = serde_json::from_slice(
        &env.cmd()
            .arg("--json")
            .arg("index")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(v2["total_docs"], 2);
}

#[test]
fn index_respects_user_ignores() {
    let env = Env::new();
    env.write("keep.md", "# Keep\n");
    env.write("drafts/skip.md", "# Skip\n");

    env.cmd().arg("ignore").arg("drafts/**").assert().success();

    let out = env.cmd().arg("--json").arg("index").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let titles: Vec<String> = v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["docs"].as_array().unwrap().iter())
        .map(|d| d["title"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(titles, vec!["Keep"]);
}
