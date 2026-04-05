//! CLI smoke-tests using `assert_cmd`.
//!
//! These tests verify that the compiled `vex` binary exits predictably for
//! flag-only invocations that require no runtime configuration.

use assert_cmd::Command;

#[test]
fn vex_help_exits_successfully() {
    Command::cargo_bin("vex")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn vex_completions_help_exits_successfully() {
    Command::cargo_bin("vex")
        .unwrap()
        .args(["completions", "--help"])
        .assert()
        .success();
}
