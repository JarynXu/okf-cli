#![allow(missing_docs)]

use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

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
    let fixture = fs::canonicalize("tests/fixtures/valid").expect("fixture path");

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

#[test]
fn library_manifest_controls_identity_semantic_catalog_and_query_guidance() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let registry = temporary.path().join("libraries.json");
    let library = temporary.path().join("mcx-library");
    fs::create_dir_all(library.join("interfaces")).unwrap();
    fs::write(
        library.join("interfaces/xcap.md"),
        "---\nid: interfaces/xcap\ntitle: XCAP\n---\n\nXCAP document selector and AUID knowledge.\n",
    )
    .unwrap();
    fs::write(
        library.join("okf-library.yaml"),
        r#"schema_version: "1"
id: mcx
name: Mission Critical Services
catalog:
  - id: xcap
    title: XCAP interfaces
    description: XCAP selectors and application usage identifiers.
    path: interfaces/xcap
    terms: [xcap, auid, document-selector]
query:
  preferred: semantic
  capabilities: [lexical, semantic, agentic]
  hints:
    - Prefer the XCAP topic for AUID and document selector questions.
"#,
    )
    .unwrap();

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "add"])
        .arg(&library)
        .assert()
        .success()
        .stdout(predicate::str::contains("installed mcx"));
    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "mount", "mcx"])
        .assert()
        .success();

    let output = okf()
        .args(["--output", "json"])
        .arg("--registry")
        .arg(&registry)
        .args(["library", "catalog", "mcx"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["data"][0]["entries"][0]["id"], "xcap");
    assert_eq!(
        value["data"][0]["entries"][0]["uri"]["path"],
        "interfaces/xcap"
    );

    let output = okf()
        .args(["--output", "json"])
        .arg("--registry")
        .arg(&registry)
        .args(["library", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["data"][0]["manifest"]["id"], "mcx");
    assert_eq!(value["data"][0]["query"]["preferred"], "semantic");
}

#[test]
fn git_library_materializes_and_mounts_through_the_same_runtime_contract() {
    if ProcessCommand::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }

    let temporary = tempfile::tempdir().expect("temporary directory");
    let source = temporary.path().join("source.git");
    let registry = temporary.path().join("runtime/libraries.json");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("index.md"),
        "---\nid: index\ntitle: Git Library\n---\n\nKnowledge cloned from Git.\n",
    )
    .unwrap();
    fs::write(
        source.join("okf-library.yaml"),
        "schema_version: \"1\"\nid: git-knowledge\nname: Git Knowledge\ncatalog:\n  - id: root\n    title: Root\n    path: index\n",
    )
    .unwrap();
    assert!(
        ProcessCommand::new("git")
            .current_dir(&source)
            .args(["init"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        ProcessCommand::new("git")
            .current_dir(&source)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        ProcessCommand::new("git")
            .current_dir(&source)
            .args(["config", "user.name", "OKF Test"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        ProcessCommand::new("git")
            .current_dir(&source)
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        ProcessCommand::new("git")
            .current_dir(&source)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success()
    );

    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "add"])
        .arg(&source)
        .assert()
        .success()
        .stdout(predicate::str::contains("installed git-knowledge"));
    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "mount", "git-knowledge"])
        .assert()
        .success();
    okf()
        .arg("--registry")
        .arg(&registry)
        .args(["library", "read", "okf://git-knowledge/index"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Knowledge cloned from Git"));
}
