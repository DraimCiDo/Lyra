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

#[test]
fn large_text_project_snapshot_survives_resume_beyond_the_old_index_and_read_budget() {
    let repo = tempfile::tempdir().unwrap();
    for i in 0..200 {
        std::fs::write(
            repo.path().join(format!("source{i:03}.py")),
            "# line\n".repeat(700),
        )
        .unwrap();
    }
    std::fs::write(
        repo.path().join("lock.json"),
        format!("\"{}\"", "x".repeat(100_000)),
    )
    .unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let mut store =
        lyra::session::SessionStore::open(&store_dir.path().join("sessions.db")).unwrap();
    let id = store.create(repo.path(), &[], "Large snapshot").unwrap();
    let snapshot = store.materialize(&id).unwrap();
    assert_eq!(
        std::fs::read_dir(snapshot.directory.path())
            .unwrap()
            .count(),
        201
    );
    assert_eq!(
        std::fs::read(snapshot.directory.path().join("lock.json")).unwrap(),
        std::fs::read(repo.path().join("lock.json")).unwrap()
    );
    std::fs::write(snapshot.directory.path().join("source199.py"), "changed").unwrap();
    assert!(
        std::fs::read_to_string(repo.path().join("source199.py"))
            .unwrap()
            .starts_with("# line")
    );
}

#[test]
fn full_database_rejects_new_snapshot_without_losing_the_previous_one() {
    let repo = tempfile::tempdir().unwrap();
    let content = "a".repeat(2 * 1024 * 1024);
    for index in 0..8 {
        std::fs::write(repo.path().join(format!("source{index}.txt")), &content).unwrap();
    }
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("sessions.db");
    let mut store = lyra::session::SessionStore::open(&path).unwrap();
    let first = store.create(repo.path(), &[], "first").unwrap();
    assert!(store.create(repo.path(), &[], "second").is_err());
    assert!(std::fs::metadata(&path).unwrap().len() <= 32 * 1024 * 1024);
    let recovered = store.materialize(&first).unwrap();
    assert_eq!(
        std::fs::read_to_string(recovered.directory.path().join("source7.txt")).unwrap(),
        content
    );
}

#[test]
fn session_listing_is_paginated_and_contains_tasks_but_not_source_snapshots() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("main.py"), "PRIVATE_SOURCE_SENTINEL").unwrap();
    let folder = tempfile::tempdir().unwrap();
    let db = folder.path().join("sessions.db");
    let mut store = lyra::session::SessionStore::open(&db).unwrap();
    let first = store.create(repo.path(), &[], "task one").unwrap();
    let second = store.create(repo.path(), &[], "task two").unwrap();
    let third = store.create(repo.path(), &[], "task three").unwrap();
    let page = store.list(0, 2).unwrap();
    assert!(page.has_more);
    assert_eq!(page.sessions[0].session_id, third);
    assert_eq!(page.sessions[1].session_id, second);
    assert_eq!(store.list(2, 2).unwrap().sessions[0].session_id, first);
    assert!(store.list(0, 0).is_err());
    assert!(store.list(0, 41).is_err());
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(["session", "list", "--db"])
        .arg(&db)
        .args(["--limit", "2"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(!text.contains("PRIVATE_SOURCE_SENTINEL"));
    assert!(text.contains("task three"));
}

#[test]
fn unrelated_sqlite_database_is_never_initialized_or_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("business.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE customers(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO customers VALUES(1,'preserve');").unwrap();
    drop(connection);
    let before = std::fs::read(&path).unwrap();
    assert!(lyra::session::SessionStore::open(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn legacy_v1_database_is_adopted_but_unexpected_schema_objects_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sessions.db");
    drop(lyra::session::SessionStore::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "application_id", 0_u32)
        .unwrap();
    drop(connection);
    drop(lyra::session::SessionStore::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    let id: u32 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .unwrap();
    assert_eq!(id, 0x4c595241);
    connection.execute_batch("CREATE TRIGGER unexpected AFTER DELETE ON sessions BEGIN DELETE FROM source_files; END;").unwrap();
    drop(connection);
    assert!(lyra::session::SessionStore::open(&path).is_err());
}
