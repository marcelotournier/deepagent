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

#[test]
fn test_verbose_flag_accepted() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("GEMINI_API_KEY", "test-key")
        .args(["-v", "-p", "test"])
        .assert()
        .failure(); // fails because API key is invalid, but flag is accepted
}

#[test]
fn test_json_flag_accepted() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("GEMINI_API_KEY", "test-key")
        .args(["--json", "-p", "test"])
        .assert()
        .failure(); // fails because API key is invalid, but flag is accepted
}

#[test]
fn test_sessions_list_empty() {
    // --sessions should work even with no sessions
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("HOME", "/tmp/deepagent_test_nonexistent")
        .arg("--sessions")
        .assert()
        .success()
        .stdout(predicate::str::contains("No saved sessions"));
}

#[test]
fn test_resume_nonexistent() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("GEMINI_API_KEY", "test-key")
        .env("HOME", "/tmp/deepagent_test_nonexistent")
        .args(["--resume", "nonexistent123"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_resume_last_no_sessions() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("GEMINI_API_KEY", "test-key")
        .env("HOME", "/tmp/deepagent_test_nonexistent")
        .args(["--resume", "last"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no session"));
}

#[test]
fn test_help_shows_all_flags() {
    let output = Command::cargo_bin("deepagent")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify all major flags are documented
    assert!(stdout.contains("--prompt"));
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--max-turns"));
    assert!(stdout.contains("--timeout"));
    assert!(stdout.contains("--verbose"));
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("--resume"));
    assert!(stdout.contains("--sessions"));
}

#[test]
fn test_custom_model_flag() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("GEMINI_API_KEY", "test-key")
        .args(["--model", "gemini-2.5-pro", "-p", "test"])
        .assert()
        .failure(); // fails at API call, but model flag accepted
}

#[test]
fn test_init_creates_config() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("deepagent")
        .unwrap()
        .current_dir(dir.path())
        .arg("--init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Created DEEPAGENT.md"));

    assert!(dir.path().join("DEEPAGENT.md").exists());
}

#[test]
fn test_init_refuses_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("DEEPAGENT.md"), "existing").unwrap();

    Command::cargo_bin("deepagent")
        .unwrap()
        .current_dir(dir.path())
        .arg("--init")
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_max_turns_flag() {
    Command::cargo_bin("deepagent")
        .unwrap()
        .env("GEMINI_API_KEY", "test-key")
        .args(["--max-turns", "5", "-p", "test"])
        .assert()
        .failure(); // fails at API call, but flag accepted
}
