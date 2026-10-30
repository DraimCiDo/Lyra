#![cfg(feature = "desktop")]
use std::{fs, process::Command};

fn worker() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lyra-desktop"));
    cmd.arg("--worker");
    cmd
}

#[test]
fn standalone_worker_reports_hardware_without_cli_companion() {
    let output = worker().args(["doctor", "--json"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
}

#[test]
fn model_error_preserves_project_and_reports_failure() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("Проект с пробелом");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("lib.rs"), "original\r\n").unwrap();
    let task = temp.path().join("task.txt");
    fs::write(&task, "Исправь ошибку").unwrap();
    let output = worker()
        .arg("task")
        .arg("--repo")
        .arg(&repo)
        .arg("--model")
        .arg(temp.path().join("missing.gguf"))
        .arg("--task-file")
        .arg(task)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(repo.join("lib.rs")).unwrap(),
        "original\r\n"
    );
}

#[test]
fn standalone_apply_rechecks_conflicts_and_rejects_incomplete_report() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("Проект с пробелом");
    fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    let source = repo.join("lib.rs");
    fs::write(&source, "old\r\n").unwrap();
    let path = temp.path().join("report.json");
    let mut report = serde_json::json!({"schema_version":1,"workspace":repo,"status":"incomplete","summaries":[],"proposals":[{"path":"lib.rs","original":"old\r\n","replacement":"new\r\n"}],"events":[],"calls":1,"subagents":false,"truncated_index":false,"model_review_is_verified_test":false});
    fs::write(&path, report.to_string()).unwrap();
    let apply = || {
        worker()
            .arg("apply")
            .arg("--repo")
            .arg(&repo)
            .arg("--report")
            .arg(&path)
            .args(["--index", "0"])
            .output()
            .unwrap()
    };
    assert!(!apply().status.success());
    assert_eq!(fs::read_to_string(&source).unwrap(), "old\r\n");
    report["status"] = "completed".into();
    fs::write(&path, report.to_string()).unwrap();
    assert!(apply().status.success());
    assert_eq!(fs::read_to_string(&source).unwrap(), "new\r\n");
    assert!(!apply().status.success());
    assert_eq!(fs::read_to_string(&source).unwrap(), "new\r\n");
}

#[test]
fn standalone_gui_worker_supports_new_project_and_document_commands() {
    let temp = tempfile::tempdir().unwrap();
    let output = worker()
        .args([
            "project",
            "starter",
            "--name",
            "gui-starter",
            "--profile",
            "python",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let plan = temp.path().join("plan.json");
    fs::write(&plan, &output.stdout).unwrap();
    let output = worker()
        .args(["project", "create", "--plan"])
        .arg(plan)
        .arg("--parent")
        .arg(temp.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(temp.path().join("gui-starter/main.py").is_file());
    let source = temp.path().join("guide.md");
    fs::write(
        &source,
        "# Python\nargparse defines a command-line parser.\n",
    )
    .unwrap();
    let pack = temp.path().join("pack.json");
    let output = worker()
        .args(["docs", "import", "--file"])
        .arg(source)
        .arg("--output")
        .arg(&pack)
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = worker()
        .args(["docs", "search", "--pack"])
        .arg(pack)
        .args(["--query", "argparse"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("local:guide.md"));
}
