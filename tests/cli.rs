#![allow(missing_docs)]

use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn okf() -> Command {
    Command::cargo_bin("okf").expect("binary should build")
}

#[test]
fn validates_valid_bundle_as_json() {
    let output = okf()
        .args([
            "--output",
            "json",
            "--bundle",
            "tests/fixtures/valid",
            "validate",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).expect("valid JSON output");
    assert_eq!(value["schema_version"], "1");
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["documents_checked"], 3);
}

#[test]
fn invalid_reference_returns_validation_exit_code() {
    okf()
        .args(["--bundle", "tests/fixtures/invalid", "validate"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("OKF203"));
}

#[test]
fn search_and_alias_resolution_use_sdk() {
    okf()
        .args([
            "--output",
            "json",
            "--bundle",
            "tests/fixtures/valid",
            "search",
            "workflow runtime",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("architecture/runtime"));

    okf()
        .args(["--bundle", "tests/fixtures/valid", "get", "runtime"])
        .assert()
        .success()
        .stdout(predicate::str::contains("executes configured workflows"));
}

#[test]
fn init_creates_a_portable_minimal_bundle() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let bundle = temporary.path().join("knowledge");

    okf()
        .args(["--bundle"])
        .arg(&bundle)
        .arg("init")
        .assert()
        .success();
    assert!(bundle.join("index.md").is_file());

    okf()
        .args(["--bundle"])
        .arg(&bundle)
        .arg("validate")
        .assert()
        .success();
    assert_eq!(Path::new("index.md"), Path::new("index.md"));
}

#[test]
fn invalid_usage_is_json_when_requested() {
    let output = okf()
        .args(["--output", "json", "not-a-command"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).expect("valid JSON output");
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["kind"], "usage");
}

#[test]
fn library_local_lifecycle_and_query_are_persistent() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let registry = temporary.path().join("libraries.json");
    let fixture = std::fs::canonicalize("tests/fixtures/valid").expect("fixture path");

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "add"])
        .arg(&fixture)
        .args(["--id", "docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("installed docs"));

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "mount", "docs"])
        .assert()
        .success();

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "catalog", "docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("architecture/runtime"));

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "read", "okf://docs/architecture/runtime"])
        .assert()
        .success()
        .stdout(predicate::str::contains("executes configured workflows"));

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "query", "workflow runtime", "--library", "docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("architecture/runtime"));

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "unmount", "docs"])
        .assert()
        .success();

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "remove", "docs"])
        .assert()
        .success();
}
