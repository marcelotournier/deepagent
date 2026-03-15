use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_no_args_shows_error() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("GEMINI_API_KEY", "test-key")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No prompt provided"));
}

#[test]
fn test_missing_api_key() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env_remove("GEMINI_API_KEY")
        .arg("-p")
        .arg("hello")
        .assert()
        .failure()
        .stderr(predicate::str::contains("GEMINI_API_KEY"));
}

#[test]
fn test_help_flag() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("coding agent powered by Gemini"));
}

#[test]
fn test_version_flag() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("deepagent"));
}
