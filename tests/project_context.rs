#![allow(missing_docs)]

use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

use assert_cmd::Command;
use serde_json::Value;

fn okf() -> Command {
    Command::cargo_bin("okf").expect("binary should build")
}

fn git(repository: &Path, args: &[&str]) {
    assert!(
        ProcessCommand::new("git")
            .current_dir(repository)
            .args(args)
            .status()
            .expect("git should execute")
            .success()
    );
}

fn status(profile: &Path) -> Value {
    let output = okf()
        .args(["--output", "json", "--project-context"])
        .arg(profile)
        .args(["project", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("status should be valid JSON")
}

#[test]
fn project_context_recovers_committed_and_working_tree_changes() {
    if ProcessCommand::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }

    let temporary = tempfile::tempdir().expect("temporary directory");
    let repository = temporary.path().join("repo");
    let runtime = temporary.path().join("runtime");
    let registry = runtime.join("libraries.json");
    let profile = runtime.join("project-context.json");
    fs::create_dir_all(repository.join("src")).unwrap();
    fs::write(
        repository.join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .unwrap();

    git(&repository, &["init"]);
    git(&repository, &["config", "user.email", "test@example.com"]);
    git(&repository, &["config", "user.name", "OKF Test"]);
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-m", "initial"]);

    okf()
        .arg("--registry")
        .arg(&registry)
        .arg("--project-context")
        .arg(&profile)
        .args(["project", "init", "--repository"])
        .arg(&repository)
        .args(["--project", "demo", "--id", "project-context"])
        .assert()
        .success();

    let initial = status(&profile);
    assert_eq!(initial["data"]["state"], "UNINITIALIZED");

    okf()
        .arg("--project-context")
        .arg(&profile)
        .args(["project", "checkpoint"])
        .assert()
        .success();
    assert_eq!(status(&profile)["data"]["state"], "VALID");

    fs::write(
        repository.join("src/lib.rs"),
        "pub fn value() -> u8 { 2 }\n",
    )
    .unwrap();
    let working = status(&profile);
    assert_eq!(working["data"]["state"], "DIRTY");
    assert!(
        working["data"]["changed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "src/lib.rs")
    );
    assert!(
        working["data"]["impacted_topics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "okf://project-context/current/architecture")
    );

    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-m", "change value"]);
    assert_eq!(status(&profile)["data"]["state"], "DIRTY");

    okf()
        .arg("--project-context")
        .arg(&profile)
        .args(["project", "checkpoint"])
        .assert()
        .success();
    assert_eq!(status(&profile)["data"]["state"], "VALID");

    let libraries = okf()
        .args(["--output", "json", "--registry"])
        .arg(&registry)
        .args(["library", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let libraries: Value = serde_json::from_slice(&libraries).unwrap();
    assert_eq!(libraries["data"][0]["manifest"]["id"], "project-context");
    assert_eq!(libraries["data"][0]["mounted"], true);
}
