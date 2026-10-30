use lyra::{
    agent::{self, Model},
    workspace::Workspace,
};
use std::{collections::VecDeque, io, path::PathBuf};

struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lyra агент с пробелами-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, content: &str) {
        std::fs::write(self.0.join(path), content).unwrap();
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Script {
    actions: VecDeque<String>,
    roles: Vec<String>,
}
impl Script {
    fn new(actions: Vec<serde_json::Value>) -> Self {
        Self {
            actions: actions.into_iter().map(|v| v.to_string()).collect(),
            roles: Vec::new(),
        }
    }
}
impl Model for Script {
    fn respond(&mut self, prompt: &str) -> io::Result<String> {
        let value: serde_json::Value = serde_json::from_str(prompt).unwrap();
        self.roles.push(value["role"].as_str().unwrap().into());
        self.actions
            .pop_front()
            .ok_or_else(|| io::Error::other("script exhausted"))
    }
}
fn done(text: &str) -> serde_json::Value {
    serde_json::json!({"tool":"finish","summary":text})
}

#[test]
fn task_subagents_proposal_review_and_explicit_apply() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "pub fn add(a:i32,b:i32)->i32 { a-b }\r\n");
    repo.write(
        "Cargo.toml",
        "[package]\nname=\"lyra-fixture\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n",
    );
    let test_source = "pub fn add(a:i32,b:i32)->i32 { a-b }\r\n#[test] fn adds_two_and_three() { assert_eq!(add(2,3),5); }\r\n";
    repo.write("src/lib.rs", test_source);
    let verify = || {
        std::process::Command::new(env!("CARGO"))
            .args(["test", "--offline", "--manifest-path"])
            .env("CARGO_TARGET_DIR", repo.0.join("target"))
            .arg(repo.0.join("Cargo.toml"))
            .output()
            .unwrap()
    };
    let baseline = verify();
    assert!(!baseline.status.success());
    assert!(String::from_utf8_lossy(&baseline.stdout).contains("adds_two_and_three"));
    let mut workspace = Workspace::open(&repo.0, &[]).unwrap();
    let mut model = Script::new(vec![
        serde_json::json!({"tool":"read_file","path":"src/lib.rs"}),
        done("Investigated add."),
        serde_json::json!({"tool":"read_file","path":"src/lib.rs"}),
        serde_json::json!({"tool":"propose_edit","path":"src/lib.rs","old":"a-b","new":"a+b"}),
        done("Proposed add fix; tests not run."),
        done("Reviewed proposal; no executable test evidence."),
    ]);
    let report = agent::execute(&mut model, &mut workspace, "Fix add", true, 12);
    assert_eq!(report.status, "completed");
    assert_eq!(
        model.roles,
        vec![
            "researcher",
            "researcher",
            "coder",
            "coder",
            "coder",
            "reviewer"
        ]
    );
    assert_eq!(report.calls, 6);
    assert!(!report.model_review_is_verified_test);
    assert!(
        std::fs::read_to_string(repo.0.join("src/lib.rs"))
            .unwrap()
            .contains("a-b")
    );
    assert_eq!(report.proposals.len(), 1);
    let report_path = repo.0.join("report.json");
    std::fs::write(&report_path, serde_json::to_vec(&report).unwrap()).unwrap();
    let applied = std::process::Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(["apply", "--repo"])
        .arg(&repo.0)
        .arg("--report")
        .arg(&report_path)
        .args(["--index", "0"])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let modified = std::fs::read_to_string(repo.0.join("src/lib.rs")).unwrap();
    assert_eq!(modified, test_source.replace("a-b", "a+b"));
    let report_path = repo.0.join("report.json");
    std::fs::write(&report_path, serde_json::to_vec(&report).unwrap()).unwrap();
    // CLI application rejects a report whose original is now stale.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(["apply", "--repo"])
        .arg(&repo.0)
        .arg("--report")
        .arg(&report_path)
        .args(["--index", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Конфликт"));
}

#[test]
fn secrets_gitignore_nested_ignore_and_parent_paths_are_denied() {
    let repo = Repo::new();
    repo.write(".env", "SECRET=example");
    repo.write("production.json", "secret");
    repo.write(".gitignore", "production.json\n");
    repo.write("src/.gitignore", "private.rs\n");
    repo.write("src/private.rs", "secret");
    repo.write("src/public.rs", "safe");
    let mut workspace = Workspace::open(&repo.0, &[]).unwrap();
    assert_eq!(workspace.read("src/public.rs").unwrap(), "safe");
    for path in [
        ".env",
        "production.json",
        "src/private.rs",
        "../outside",
        "/etc/passwd",
        "C:/Windows/test",
        "src/../production.json",
    ] {
        assert!(workspace.read(path).is_err(), "{path}");
        assert!(
            Workspace::open(&repo.0, &[path.into()]).is_err(),
            "selected {path}"
        );
    }
}

#[test]
fn read_only_workers_cannot_propose_and_budgets_stop_loops() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "old");
    let mut workspace = Workspace::open(&repo.0, &[]).unwrap();
    let mut model = Script::new(vec![
        serde_json::json!({"tool":"propose_edit","path":"src/lib.rs","old":"old","new":"new"}),
    ]);
    let report = agent::execute(&mut model, &mut workspace, "fix", true, 3);
    assert!(report.status.starts_with("step_limit"));
    assert!(report.proposals.is_empty());
    assert!(report.events[0].outcome.contains("запрещены"));
    let mut model = Script::new(vec![serde_json::json!({"tool":"list"}); 2]);
    let report = agent::execute(&mut model, &mut workspace, "fix", false, 2);
    assert_eq!(report.calls, 2);
    assert_eq!(report.status, "step_limit: coder");
}

#[test]
fn unknown_tools_and_fields_do_not_expand_authority() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "old");
    for value in [
        serde_json::json!({"tool":"shell","command":"delete all"}),
        serde_json::json!({"tool":"finish","summary":"ok","override_policy":true}),
    ] {
        let mut workspace = Workspace::open(&repo.0, &[]).unwrap();
        let mut model = Script::new(vec![value]);
        let report = agent::execute(&mut model, &mut workspace, "fix", false, 2);
        assert!(report.status.starts_with("invalid_response"));
        assert!(report.proposals.is_empty());
    }
}

#[test]
fn ambiguous_replacements_and_changed_files_do_not_overwrite() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "x x");
    let mut workspace = Workspace::open(&repo.0, &[]).unwrap();
    assert!(workspace.propose("src/lib.rs", "x", "y").is_err());
    repo.write("src/lib.rs", "old");
    let edit = workspace.propose("src/lib.rs", "old", "new").unwrap();
    repo.write("src/lib.rs", "user changes");
    assert!(workspace.apply(&edit).is_err());
    assert_eq!(
        std::fs::read_to_string(repo.0.join("src/lib.rs")).unwrap(),
        "user changes"
    );
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_the_workspace() {
    let repo = Repo::new();
    let outside = Repo::new();
    outside.write("secret.txt", "secret");
    std::os::unix::fs::symlink(&outside.0, repo.0.join("link")).unwrap();
    assert!(Workspace::open(&repo.0, &["link/secret.txt".into()]).is_err());
    let workspace = Workspace::open(&repo.0, &[]).unwrap();
    assert!(!workspace.files.iter().any(|p| p.starts_with("link/")));
}

#[test]
fn oversized_and_binary_files_fail_closed() {
    let repo = Repo::new();
    repo.write("src/large.rs", &"x".repeat(65537));
    repo.write("src/binary.rs", "a\0b");
    let mut workspace = Workspace::open(&repo.0, &[]).unwrap();
    assert!(workspace.read("src/large.rs").is_err());
    assert!(workspace.read("src/binary.rs").is_err());
}

#[test]
fn nested_whitelist_cannot_reopen_an_ignored_parent() {
    let repo = Repo::new();
    std::fs::create_dir(repo.0.join("sealed")).unwrap();
    repo.write(".gitignore", "sealed/\n");
    repo.write("sealed/.gitignore", "!public.rs\n");
    repo.write("sealed/public.rs", "private");
    assert!(Workspace::open(&repo.0, &["sealed/public.rs".into()]).is_err());
}

#[test]
fn read_only_files_are_not_replaced_or_left_with_staging_files() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "old");
    let mut workspace = Workspace::open(&repo.0, &[]).unwrap();
    let edit = workspace.propose("src/lib.rs", "old", "new").unwrap();
    let path = repo.0.join("src/lib.rs");
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).unwrap();
    let result = workspace.apply(&edit);
    std::fs::set_permissions(&path, original).unwrap();
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    assert!(!std::fs::read_dir(&repo.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lyra-apply")
    }));
}

#[cfg(windows)]
#[test]
fn windows_junction_cannot_escape_the_workspace() {
    let repo = Repo::new();
    let outside = Repo::new();
    outside.write("secret.txt", "secret");
    let junction = repo.0.join("junction");
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(Workspace::open(&repo.0, &["junction/secret.txt".into()]).is_err());
    let workspace = Workspace::open(&repo.0, &[]).unwrap();
    assert!(!workspace.files.iter().any(|p| p.starts_with("junction/")));
}
