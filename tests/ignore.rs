//! e2e tests for `mdpeek ignore`.

mod common;

use common::Env;
use predicates::prelude::*;

#[test]
fn ignore_add_and_list() {
    let env = Env::new();
    env.cmd()
        .args(["ignore", "drafts/**", "CHANGELOG*"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added 2 pattern(s)"))
        .stdout(predicate::str::contains("drafts/**"))
        .stdout(predicate::str::contains("CHANGELOG*"));

    env.cmd()
        .args(["ignore", "--list"])
        .assert()
        .success()
        .stdout("CHANGELOG*\ndrafts/**\n");
}

#[test]
fn ignore_remove_missing_reports_no_such_pattern() {
    let env = Env::new();
    env.cmd()
        .args(["ignore", "--remove", "nope"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no such pattern: nope"));
}

#[test]
fn ignore_remove_existing() {
    let env = Env::new();
    env.cmd().args(["ignore", "drafts/**"]).assert().success();
    env.cmd()
        .args(["ignore", "--remove", "drafts/**"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed: drafts/**"));
    env.cmd()
        .args(["ignore", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn ignore_clear() {
    let env = Env::new();
    env.cmd().args(["ignore", "a", "b", "c"]).assert().success();
    env.cmd()
        .args(["ignore", "--clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cleared all user-ignore patterns"));
    env.cmd()
        .args(["ignore", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn ignore_json_output() {
    let env = Env::new();
    env.cmd()
        .args(["--json", "ignore", "drafts/**"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""patterns""#))
        .stdout(predicate::str::contains(r#""drafts/**""#));
}

#[test]
fn ignore_state_is_isolated_per_repo() {
    let a = Env::new();
    let b = Env::new();
    a.cmd().args(["ignore", "only-in-a"]).assert().success();
    b.cmd()
        .args(["ignore", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}
