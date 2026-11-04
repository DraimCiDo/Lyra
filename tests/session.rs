use lyra::{
    agent::{self, Model, WorkflowOptions},
    session::SessionStore,
    workspace::Workspace,
};
use std::io;

struct Script(std::collections::VecDeque<serde_json::Value>);
impl Model for Script {
    fn respond(&mut self, _: &str) -> io::Result<String> {
        self.0
            .pop_front()
            .map(|value| value.to_string())
            .ok_or_else(|| io::Error::other("simulated disconnect"))
    }
}

#[test]
fn checkpoints_resume_in_an_isolated_copy_and_keep_dirty_originals_safe() {
    let repo = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    std::fs::write(
        repo.path().join("source.py"),
        "answer = 0\r\n# user's uncommitted change\r\n",
    )
    .unwrap();
    std::fs::write(repo.path().join(".env"), "PRIVATE_SENTINEL").unwrap();
    let db = local.path().join("история задач.sqlite");
    let mut store = SessionStore::open(&db).unwrap();
    let id = store.create(repo.path(), &[], "Return 42").unwrap();
    let docs = lyra::knowledge::Pack::empty();
    {
        let snapshot = store.materialize(&id).unwrap();
        assert!(!snapshot.directory.path().join(".env").exists());
        let mut workspace = Workspace::open(snapshot.directory.path(), &[]).unwrap();
        let save = |report: &agent::Report| store.checkpoint(&id, report);
        let mut script = Script([serde_json::json!({"tool":"propose_edit","path":"source.py","old":"answer = 0","new":"answer = 41"})].into());
        let report = agent::execute_workflow(
            &mut script,
            &mut workspace,
            "Return 42",
            WorkflowOptions {
                subagents: false,
                max_calls: 4,
                docs: &docs,
                dedup_tool_context: false,
                check: None,
                deadline: None,
                previous: None,
                checkpoint: Some(&save),
            },
        );
        assert!(report.status.starts_with("model_error"));
        // Even if the worker is lost before saving its final error, the accepted edit survives.
        assert_eq!(store.report(&id).unwrap().unwrap().proposals.len(), 1);
    }
    drop(store);
    let mut store = SessionStore::open(&db).unwrap();
    let mut snapshot = store.materialize(&id).unwrap();
    let mut workspace = Workspace::open(snapshot.directory.path(), &[]).unwrap();
    let mut script = Script([
        serde_json::json!({"tool":"propose_edit","path":"source.py","old":"answer = 41","new":"answer = 42"}),
        serde_json::json!({"tool":"finish","summary":"Prepared; tests not run"}),
    ].into());
    let report = agent::execute_workflow(
        &mut script,
        &mut workspace,
        "Return 42",
        WorkflowOptions {
            subagents: false,
            max_calls: 8,
            docs: &docs,
            dedup_tool_context: false,
            check: None,
            deadline: None,
            previous: snapshot.report.take(),
            checkpoint: None,
        },
    );
    assert_eq!(report.status, "completed");
    store.checkpoint(&id, &report).unwrap();
    let saved = store.report(&id).unwrap().unwrap();
    assert_eq!(saved.workspace, repo.path().canonicalize().unwrap());
    assert!(
        saved.proposals[0]
            .replacement
            .contains("42\r\n# user's uncommitted change")
    );
    assert!(
        std::fs::read_to_string(repo.path().join("source.py"))
            .unwrap()
            .contains("answer = 0")
    );
    std::fs::write(repo.path().join("source.py"), "new external changes").unwrap();
    let mut original = Workspace::open(repo.path(), &[]).unwrap();
    assert!(original.apply(&saved.proposals[0]).is_err());
    assert!(store.delete(&id).unwrap());
    assert!(!store.exists(&id).unwrap());
    assert!(store.materialize(&id).is_err());
}

#[test]
fn snapshot_rejects_escaping_paths_and_unknown_database_versions() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.sqlite");
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("safe.txt"), "safe").unwrap();
    let mut store = SessionStore::open(&db).unwrap();
    let id = store.create(repo.path(), &[], "Inspect").unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE source_files SET path='../escape.txt' WHERE session_id=?1",
            [&id],
        )
        .unwrap();
    assert!(store.materialize(&id).is_err());
    connection.execute_batch("PRAGMA user_version=99").unwrap();
    assert!(SessionStore::open(&db).is_err());
}

#[cfg(unix)]
#[test]
fn session_sources_and_new_database_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("safe.txt"), "private code").unwrap();
    let db = dir.path().join("session.sqlite");
    let mut store = SessionStore::open(&db).unwrap();
    let id = store.create(repo.path(), &[], "Inspect").unwrap();
    let snapshot = store.materialize(&id).unwrap();
    assert_eq!(
        std::fs::metadata(&db).unwrap().permissions().mode() & 0o077,
        0
    );
    assert_eq!(
        std::fs::metadata(snapshot.directory.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o077,
        0
    );
}

#[test]
fn concurrent_checkpoint_cannot_overwrite_a_newer_session() {
    let repo = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("safe.txt"), "source").unwrap();
    let db = dir.path().join("sessions.sqlite");
    let mut first = SessionStore::open(&db).unwrap();
    let id = first.create(repo.path(), &[], "Inspect").unwrap();
    let second = SessionStore::open(&db).unwrap();
    let snapshot = second.materialize(&id).unwrap();
    let mut workspace = Workspace::open(snapshot.directory.path(), &[]).unwrap();
    let mut model = Script([serde_json::json!({"tool":"finish","summary":"Inspected"})].into());
    let report = agent::execute(&mut model, &mut workspace, "Inspect", false, 2);
    first.checkpoint(&id, &report).unwrap();
    assert!(second.checkpoint(&id, &report).is_err());
    assert_eq!(first.report(&id).unwrap().unwrap().status, "completed");
}
