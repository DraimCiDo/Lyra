use serde_json::json;
use std::{
    path::Path,
    process::{Command, Output},
};
fn cli(repo: &Path, command: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lyra"))
        .arg(command)
        .arg("--repo")
        .arg(repo)
        .args(extra)
        .output()
        .unwrap()
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("lyra CLI публикация-")
        .tempdir()
        .unwrap();
    std::fs::write(root.path().join("a.txt"), "original\r\n").unwrap();
    std::fs::write(root.path().join("b.txt"), "dirty user file\n").unwrap();
    let report = json!({
        "schema_version":4,"workspace":root.path().canonicalize().unwrap(),
        "status":"completed","outcome":"changes_proposed","summaries":[],
        "proposals":[
            {"path":"a.txt","original":"original\r\n","replacement":"changed\r\n"},
            {"path":"b.txt","original":"dirty user file\n","replacement":"new file\n"},
            {"path":"новый/file.txt","original":"","replacement":"new\r\n","create":true}
        ],"events":[],"calls":0,"subagents":false,"truncated_index":false,
        "model_review_is_verified_test":false
    });
    std::fs::write(root.path().join("report.json"), report.to_string()).unwrap();
    root
}
#[test]
fn cli_apply_all_checks_conflicts_before_any_writes_and_accepts_unicode_crlf() {
    let repo = fixture();
    std::fs::write(repo.path().join("b.txt"), "external").unwrap();
    let result = cli(
        repo.path(),
        "apply",
        &[
            "--report",
            repo.path().join("report.json").to_str().unwrap(),
            "--all",
        ],
    );
    assert!(!result.status.success());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "original\r\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("b.txt")).unwrap(),
        "external"
    );
    std::fs::write(repo.path().join("b.txt"), "dirty user file\n").unwrap();
    let result = cli(
        repo.path(),
        "apply",
        &[
            "--report",
            repo.path().join("report.json").to_str().unwrap(),
            "--all",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "changed\r\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("новый/file.txt")).unwrap(),
        "new\r\n"
    );
    let result = cli(repo.path(), "recover", &[]);
    assert!(result.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()["status"],
        "no_pending_publication"
    );
}
#[test]
fn cli_legacy_single_index_and_mutually_exclusive_arguments_are_preserved() {
    let repo = fixture();
    let report = repo.path().join("report.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    value["schema_version"] = json!(1);
    value["proposals"].as_array_mut().unwrap().truncate(2);
    std::fs::write(&report, value.to_string()).unwrap();
    for extra in [vec!["--all", "--index", "0"], vec![]] {
        let mut args = vec!["--report", report.to_str().unwrap()];
        args.extend(extra);
        assert!(!cli(repo.path(), "apply", &args).status.success());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "original\r\n"
        );
    }
    let result = cli(
        repo.path(),
        "apply",
        &["--report", report.to_str().unwrap(), "--index", "0"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("b.txt")).unwrap(),
        "dirty user file\n"
    );
}
#[test]
fn cli_recovery_inspects_and_rolls_back_partial_publication_without_exposing_source() {
    let repo = fixture();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join("report.json")).unwrap()).unwrap();
    std::fs::create_dir(repo.path().join(".lyra")).unwrap();
    let journal = json!({"schema_version":1,"root":repo.path().canonicalize().unwrap(),"edits":report["proposals"]});
    std::fs::write(
        repo.path().join(".lyra/apply-journal.json"),
        journal.to_string(),
    )
    .unwrap();
    std::fs::write(repo.path().join("a.txt"), "changed\r\n").unwrap();
    let result = cli(repo.path(), "recover", &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("pending"));
    assert!(!text.contains("dirty user file"));
    assert!(!text.contains("original\\r"));
    let result = cli(
        repo.path(),
        "apply",
        &[
            "--report",
            repo.path().join("report.json").to_str().unwrap(),
            "--index",
            "1",
        ],
    );
    assert!(!result.status.success());
    let result = cli(repo.path(), "recover", &["--action", "rollback"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "original\r\n"
    );
    assert!(!repo.path().join(".lyra/apply-journal.json").exists());
}
