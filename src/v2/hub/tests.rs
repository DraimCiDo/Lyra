use super::*;
use std::{io::Cursor, sync::Arc, thread};

const IMAGE: &str =
    "python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1";

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    hub: PathBuf,
}

fn fixture(check: Option<ExecutionRequest>) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("проект app");
    fs::create_dir(&repo).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Hub Test"],
        &["config", "user.email", "hub@example.invalid"],
        &["config", "commit.gpgsign", "false"],
        // Byte-exact fixtures regardless of the machine's line-ending conversion.
        &["config", "core.autocrlf", "false"],
    ] {
        git::ok(&repo, args).unwrap();
    }
    fs::write(repo.join("maths.py"), "def add(a, b):\n    return a - b\n").unwrap();
    fs::write(repo.join("README.md"), "demo\n").unwrap();
    git::ok(&repo, &["add", "-A"]).unwrap();
    git::ok(&repo, &["commit", "-q", "-m", "initial"]).unwrap();
    let hub = root.path().join("hub");
    Hub::init(&hub, &repo, "main", check).unwrap();
    Fixture {
        _root: root,
        repo,
        hub,
    }
}

/// Land whatever the task branch currently points to, as an owner who just reviewed it.
trait LandCurrent {
    fn land_current(&self, id: i64) -> io::Result<Value>;
}

impl LandCurrent for Hub {
    fn land_current(&self, id: i64) -> io::Result<Value> {
        let commit = git::ok(
            &self.repo,
            &[
                "rev-parse",
                &format!("refs/heads/{}", self.task(id)?.branch.unwrap()),
            ],
        )?;
        self.land(id, &commit)
    }
}

fn agent(fixture: &Fixture, name: &str) -> Hub {
    Hub::open(&fixture.hub, name, Some(AgentKind::Codex)).unwrap()
}

fn owner(fixture: &Fixture) -> Hub {
    Hub::open(&fixture.hub, OWNER, None).unwrap()
}

#[test]
fn a_hub_needs_a_git_project_and_a_folder_outside_it() {
    let root = tempfile::tempdir().unwrap();
    let plain = root.path().join("plain");
    fs::create_dir(&plain).unwrap();
    assert!(Hub::init(&root.path().join("hub"), &plain, "main", None).is_err());
    let fixture = fixture(None);
    assert!(Hub::init(&fixture.repo.join("hub"), &fixture.repo, "main", None).is_err());
    assert!(
        Hub::init(
            &fixture.hub.with_extension("2"),
            &fixture.repo,
            "missing",
            None
        )
        .is_err()
    );
    assert!(Hub::open(&fixture.hub, "bad name", None).is_err());
    assert!(Hub::open(&fixture.hub, OWNER, Some(AgentKind::Codex)).is_ok());
}

#[test]
fn concurrent_agents_never_claim_the_same_task_and_get_separate_worktrees() {
    let fixture = Arc::new(fixture(None));
    let owner = owner(&fixture);
    for index in 0..6 {
        owner
            .create_task(&format!("Task {index}"), "Do the work", &[])
            .unwrap();
    }
    let claims: Vec<_> = (0..4)
        .map(|index| {
            let fixture = Arc::clone(&fixture);
            thread::spawn(move || {
                let mut hub = agent(&fixture, &format!("agent-{index}"));
                let mut claimed = Vec::new();
                while let Ok(task) = hub.claim(None) {
                    claimed.push((task.id, task.worktree.unwrap(), task.branch.unwrap()));
                }
                claimed
            })
        })
        .collect();
    let mut all: Vec<_> = claims
        .into_iter()
        .flat_map(|handle| handle.join().unwrap())
        .collect();
    all.sort();
    assert_eq!(
        all.iter().map(|(id, ..)| *id).collect::<Vec<_>>(),
        (1..=6).collect::<Vec<_>>()
    );
    for (id, worktree, branch) in &all {
        assert!(
            branch.ends_with(&format!("/task-{id}")) && branch.starts_with("lyra/"),
            "{branch}"
        );
        assert_eq!(
            fs::read_to_string(worktree.join("README.md")).unwrap(),
            "demo\n"
        );
        assert_eq!(
            git::ok(worktree, &["branch", "--show-current"]).unwrap(),
            *branch
        );
    }
    let status = owner.status().unwrap();
    assert_eq!(status["agents"].as_array().unwrap().len(), 4);
    assert!(status["agents"][0]["active"].as_bool().unwrap());
}

#[test]
fn dependencies_and_ownership_control_task_transitions() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let first = owner.create_task("Schema", "Add the schema", &[]).unwrap();
    let second = owner
        .create_task("API", "Use the schema", &[first])
        .unwrap();
    assert!(owner.create_task("Broken", "x", &[99]).is_err());
    let mut codex = agent(&fixture, "codex-1");
    let mut claude = agent(&fixture, "claude-1");
    assert!(codex.claim(Some(second)).is_err());
    assert_eq!(codex.claim(None).unwrap().id, first);
    assert_eq!(
        claude.claim(None).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert!(claude.update(first, Status::Review, "not mine").is_err());
    assert!(
        codex
            .update(first, Status::Done, "agents cannot finish")
            .is_err()
    );
    codex.update(first, Status::Review, "Schema added").unwrap();
    assert!(codex.land_current(first).is_err());
    owner.land_current(first).unwrap();
    assert_eq!(owner.task(first).unwrap().status, Status::Done);
    assert_eq!(claude.claim(None).unwrap().id, second);
    claude.update(second, Status::Open, "Releasing").unwrap();
    let task = owner.task(second).unwrap();
    assert_eq!((task.status, task.owner), (Status::Open, None));
    owner.cancel(second).unwrap();
    assert!(claude.claim(Some(second)).is_err());
}

#[test]
fn file_leases_are_exclusive_case_insensitive_and_cover_folders() {
    let fixture = fixture(None);
    let mut codex = agent(&fixture, "codex-1");
    let mut claude = agent(&fixture, "claude-1");
    let paths = |items: &[&str]| {
        items
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        codex.lease(None, &paths(&["src/api"]), 600).unwrap()["leased"],
        true
    );
    let blocked = claude
        .lease(None, &paths(&["README.md", "SRC/API/routes.rs"]), 600)
        .unwrap();
    assert_eq!(blocked["leased"], false);
    assert_eq!(blocked["conflicts"][0]["agent"], "codex-1");
    // All-or-nothing: README.md was not leased either.
    assert_eq!(
        codex.lease(None, &paths(&["README.md"]), 600).unwrap()["leased"],
        true
    );
    assert!(claude.lease(None, &paths(&["../escape"]), 600).is_err());
    assert!(claude.lease(None, &paths(&["src"]), 10).is_err());
    assert_eq!(codex.release(&[]).unwrap(), 0);
    assert_eq!(codex.release_all().unwrap(), 2);
    assert_eq!(
        claude
            .lease(None, &paths(&["src/api/routes.rs"]), 600)
            .unwrap()["leased"],
        true
    );
    assert_eq!(
        owner(&fixture).status().unwrap()["leases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn messages_reach_their_recipients_once() {
    let fixture = fixture(None);
    let codex = agent(&fixture, "codex-1");
    let claude = agent(&fixture, "claude-1");
    let owner = owner(&fixture);
    codex.send(None, None, "I take the parser").unwrap();
    codex
        .send(Some("claude-1"), None, "Please review task 1")
        .unwrap();
    codex.send(Some(OWNER), None, "Ready for landing").unwrap();
    assert!(codex.send(Some("bad name"), None, "x").is_err());
    let inbox = claude.read(10).unwrap();
    assert_eq!(inbox.len(), 2);
    assert_eq!(inbox[1]["body"], "Please review task 1");
    assert!(claude.read(10).unwrap().is_empty());
    assert!(codex.read(10).unwrap().is_empty());
    assert_eq!(owner.read(10).unwrap().len(), 2);
}

#[test]
fn landing_merges_only_a_clean_reviewed_branch_into_a_clean_base() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    let task = codex.claim(None).unwrap();
    let worktree = task.worktree.unwrap();
    fs::write(
        worktree.join("maths.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    codex.update(id, Status::Review, "Fixed").unwrap();
    assert!(
        owner
            .land_current(id)
            .unwrap_err()
            .to_string()
            .contains("uncommitted")
    );
    git::ok(&worktree, &["commit", "-qam", "Fix add"]).unwrap();
    let diff = owner.diff(id, 4096).unwrap();
    assert!(
        diff["patch"]
            .as_str()
            .unwrap()
            .contains("+    return a + b")
    );
    fs::write(fixture.repo.join("README.md"), "local edit\n").unwrap();
    assert!(
        owner
            .land_current(id)
            .unwrap_err()
            .to_string()
            .contains("uncommitted")
    );
    git::ok(&fixture.repo, &["checkout", "--", "README.md"]).unwrap();
    owner.land_current(id).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.repo.join("maths.py")).unwrap(),
        "def add(a, b):\n    return a + b\n"
    );
    assert!(!worktree.exists());
    assert_eq!(owner.task(id).unwrap().status, Status::Done);
}

#[test]
fn a_configured_check_must_pass_on_the_exact_commit_before_landing() {
    let fixture = fixture(Some(ExecutionRequest {
        image: IMAGE.into(),
        program: "python".into(),
        args: vec!["-c".into(), "print(1)".into()],
        timeout_secs: 15,
        memory_mib: 256,
    }));
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    let worktree = codex.claim(None).unwrap().worktree.unwrap();
    fs::write(
        worktree.join("maths.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    git::ok(&worktree, &["commit", "-qam", "Fix add"]).unwrap();
    codex.update(id, Status::Review, "Fixed").unwrap();
    let error = owner.land_current(id).unwrap_err();
    assert!(
        error.to_string().contains("check has not passed"),
        "{error}"
    );
    // A passing record for an older commit does not count either.
    let head = git::head(&worktree).unwrap();
    let stale = json!({"passed": true, "head": "0".repeat(40), "clean": true, "exit_code": 0,
        "timed_out": false, "elapsed_ms": 1, "output": ""});
    owner
        .connection
        .execute(
            "UPDATE tasks SET check_result=?2 WHERE id=?1",
            params![id, stale.to_string()],
        )
        .unwrap();
    assert!(owner.land_current(id).is_err());
    let snapshot = executor::candidate_source_hash(&worktree, &[]).unwrap();
    let current = json!({"passed": true, "head": head, "clean": true, "exit_code": 0,
        "timed_out": false, "elapsed_ms": 1, "output": "", "source_sha256": snapshot});
    owner
        .connection
        .execute(
            "UPDATE tasks SET check_result=?2 WHERE id=?1",
            params![id, current.to_string()],
        )
        .unwrap();
    let source = worktree.join("maths.py");
    let original = fs::read(&source).unwrap();
    git::ok(
        &worktree,
        &["update-index", "--assume-unchanged", "maths.py"],
    )
    .unwrap();
    fs::write(&source, "changed since verification\n").unwrap();
    // Bytes hidden from `git status` still make the worktree dirty.
    assert!(git::dirty(&worktree).unwrap());
    assert!(
        owner
            .land_current(id)
            .unwrap_err()
            .to_string()
            .contains("uncommitted")
    );
    fs::write(source, original).unwrap();
    git::ok(
        &worktree,
        &["update-index", "--no-assume-unchanged", "maths.py"],
    )
    .unwrap();
    git::ok(
        &fixture.repo,
        &["commit", "--allow-empty", "-qm", "Base advances"],
    )
    .unwrap();
    assert!(
        owner
            .land_current(id)
            .unwrap_err()
            .to_string()
            .contains("base advanced")
    );
    assert_eq!(owner.task(id).unwrap().status, Status::Review);
    // Integrating the base invalidates the previous check even when the source tree is identical.
    git::ok(&worktree, &["merge", "--no-edit", "main"]).unwrap();
    assert!(
        owner
            .land_current(id)
            .unwrap_err()
            .to_string()
            .contains("check has not passed")
    );
    let mut current = current;
    let new_head = git::head(&worktree).unwrap();
    current["head"] = json!(new_head);
    owner
        .connection
        .execute(
            "UPDATE tasks SET check_result=?2 WHERE id=?1",
            params![id, current.to_string()],
        )
        .unwrap();
    assert_eq!(owner.land_current(id).unwrap()["commit"], new_head);
}

fn rpc(fixture: &Fixture, agent: &str, requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let mut output = Vec::new();
    mcp::serve(
        &fixture.hub,
        agent,
        AgentKind::ClaudeCode,
        Cursor::new(format!("{input}not json\n[1]\n")),
        &mut output,
    )
    .unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn mcp_clients_initialize_list_tools_and_coordinate_through_the_hub() {
    let fixture = fixture(None);
    let call = |id: u64, name: &str, arguments: Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}});
    let replies = rpc(
        &fixture,
        "claude-1",
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            call(
                3,
                "task_create",
                json!({"title":"Fix add","description":"Use addition"}),
            ),
            call(4, "task_claim", json!({})),
            call(5, "file_lease", json!({"paths":["maths.py"],"task_id":1})),
            call(
                6,
                "task_update",
                json!({"task_id":1,"status":"review","summary":"done"}),
            ),
            call(7, "task_claim", json!({})),
            call(8, "no_such_tool", json!({})),
            json!({"jsonrpc":"2.0","id":9,"method":"resources/list"}),
            json!({"jsonrpc":"2.0","id":10,"method":"ping"}),
        ],
    );
    assert_eq!(replies.len(), 12, "{replies:#?}");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "lyra-hub");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| tool["name"] == "task_claim"));
    assert!(
        tools
            .iter()
            .all(|tool| tool["inputSchema"]["type"] == "object")
    );
    assert_eq!(replies[2]["result"]["structuredContent"]["task_id"], 1);
    let claimed = &replies[3]["result"]["structuredContent"]["task"];
    assert_eq!(claimed["owner"], "claude-1");
    assert!(
        Path::new(claimed["worktree"].as_str().unwrap())
            .join("maths.py")
            .exists()
    );
    assert_eq!(replies[4]["result"]["structuredContent"]["leased"], true);
    assert_eq!(
        replies[5]["result"]["structuredContent"]["task"]["status"],
        "review"
    );
    assert_eq!(replies[6]["result"]["isError"], true);
    assert_eq!(replies[7]["error"]["code"], -32602);
    assert_eq!(replies[8]["error"]["code"], -32601);
    assert_eq!(replies[9]["result"], json!({}));
    assert_eq!(replies[10]["error"]["code"], -32700);
    assert_eq!(replies[11]["error"]["code"], -32600);
    let status = owner(&fixture).status().unwrap();
    assert_eq!(status["agents"][0]["kind"], "claude-code");
}

#[test]
fn an_unknown_protocol_version_gets_the_latest_supported_one() {
    let fixture = fixture(None);
    let replies = rpc(
        &fixture,
        "codex-1",
        &[
            json!({"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        ],
    );
    assert_eq!(replies[0]["id"], "a");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn a_local_worker_commits_its_proposal_on_the_task_branch_for_review() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner
        .create_task("Fix add", "maths.add subtracts; make it add", &[])
        .unwrap();
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let mut actions = vec![
        json!({"tool":"replace","path":"maths.py","old":"a - b","new":"a + b"}),
        json!({"tool":"finish","summary":"Use addition"}),
    ]
    .into_iter();
    let result = worker::run_one(
        &mut lyra,
        None,
        &[],
        8,
        &AtomicBool::new(false),
        &mut |report, save| {
            report.run(
                &AtomicBool::new(false),
                &mut |prompt| {
                    assert!(prompt.contains("untrusted data"));
                    Ok(actions.next().unwrap().to_string())
                },
                save,
            )
        },
    )
    .unwrap();
    assert_eq!(result["status"], "review");
    let task = owner.task(id).unwrap();
    assert_eq!(task.status, Status::Review);
    assert!(task.summary.contains("Use addition"));
    // The project checkout is untouched until the owner lands the branch.
    assert!(
        fs::read_to_string(fixture.repo.join("maths.py"))
            .unwrap()
            .contains("a - b")
    );
    owner.land_current(id).unwrap();
    assert!(
        fs::read_to_string(fixture.repo.join("maths.py"))
            .unwrap()
            .contains("a + b")
    );
    let committed = git::ok(
        &fixture.repo,
        &["show", "--name-only", "--format=", "HEAD^2"],
    )
    .unwrap();
    assert_eq!(committed, "maths.py");
}

#[test]
fn a_worker_that_finds_no_change_releases_the_task() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner
        .create_task("Explain", "Nothing to change", &[])
        .unwrap();
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let result = worker::run_one(
        &mut lyra,
        None,
        &[],
        4,
        &AtomicBool::new(false),
        &mut |report, save| {
            report.run(
                &AtomicBool::new(false),
                &mut |_| Ok(json!({"tool":"finish","summary":"No change needed"}).to_string()),
                save,
            )
        },
    )
    .unwrap();
    assert_eq!(result["status"], "open");
    assert_eq!(owner.task(id).unwrap().status, Status::Open);
    assert_eq!(owner.read(10).unwrap().len(), 1);
}

#[test]
fn mcp_rejects_malformed_arguments_and_survives_an_oversized_frame() {
    let fixture = fixture(None);
    let call = |id: u64, name: &str, arguments: Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}});
    let huge = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"ping\",\"pad\":\"{}\"}}",
        "x".repeat(2 * 1024 * 1024)
    );
    let mut input = String::new();
    for request in [
        call(1, "message_send", json!({"to": 7, "body": "hello"})),
        call(2, "message_send", json!({"to": ["owner"], "body": "hello"})),
        call(3, "message_read", json!({"limit": "ten"})),
        call(4, "file_lease", json!({"paths": ["a"], "ttl_secs": "long"})),
    ] {
        input.push_str(&format!("{request}\n"));
    }
    input.push_str(&huge);
    input.push('\n');
    input.push_str(&format!(
        "{}\n",
        json!({"jsonrpc":"2.0","id":5,"method":"ping"})
    ));
    let mut output = Vec::new();
    mcp::serve(
        &fixture.hub,
        "codex-1",
        AgentKind::Codex,
        Cursor::new(input),
        &mut output,
    )
    .unwrap();
    let replies: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 6, "{replies:#?}");
    for reply in &replies[..4] {
        assert_eq!(reply["result"]["isError"], true, "{reply}");
    }
    assert_eq!(replies[4]["error"]["code"], -32600);
    assert_eq!(replies[5]["result"], json!({}));
    // Nothing was broadcast by the rejected calls.
    assert!(owner(&fixture).read(10).unwrap().is_empty());
}

#[test]
fn landing_requires_the_reviewed_commit_and_recovers_after_git_moved_the_base() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    let worktree = codex.claim(None).unwrap().worktree.unwrap();
    fs::write(
        worktree.join("maths.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    git::ok(&worktree, &["commit", "-qam", "Fix add"]).unwrap();
    codex.update(id, Status::Review, "Fixed").unwrap();
    let reviewed = owner.diff(id, 4096).unwrap()["commit"]
        .as_str()
        .unwrap()
        .to_owned();
    // The agent resumes and commits again after the owner opened the diff.
    codex.update(id, Status::Claimed, "").unwrap();
    fs::write(worktree.join("README.md"), "changed after review\n").unwrap();
    git::ok(&worktree, &["commit", "-qam", "More"]).unwrap();
    codex.update(id, Status::Review, "More").unwrap();
    let refused = owner.land(id, &reviewed).unwrap_err();
    assert!(refused.to_string().contains("review it again"), "{refused}");
    assert!(owner.land(id, "abc").is_err());
    assert_eq!(owner.task(id).unwrap().status, Status::Review);
    assert!(fs::read_to_string(fixture.repo.join("README.md")).unwrap() == "demo\n");
    // Git moves the base, then the process stops before the database is updated.
    let head = owner.diff(id, 4096).unwrap()["commit"]
        .as_str()
        .unwrap()
        .to_owned();
    owner.log(Some(id), "land-intent", &head).unwrap();
    git::ok(&fixture.repo, &["merge", "--ff-only", &head]).unwrap();
    git::ok(
        &fixture.repo,
        &["commit", "--allow-empty", "-qm", "Owner continues"],
    )
    .unwrap();
    let result = owner.land(id, &head).unwrap();
    assert_eq!(result["landed"], id);
    assert_eq!(owner.task(id).unwrap().status, Status::Done);
}

#[test]
fn reopen_validates_the_note_and_commits_with_its_message() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    codex.claim(None).unwrap();
    codex.update(id, Status::Review, "Done").unwrap();
    assert!(owner.reopen(id, "   ").is_err());
    assert_eq!(owner.task(id).unwrap().status, Status::Review);
    owner.reopen(id, "Tests fail on Windows paths").unwrap();
    assert_eq!(owner.task(id).unwrap().status, Status::Open);
    let inbox = codex.read(10).unwrap();
    assert_eq!(inbox[0]["body"], "Tests fail on Windows paths");
}

#[test]
fn a_hub_needs_a_local_branch_and_namespaces_its_task_branches() {
    let fixture = fixture(None);
    git::ok(&fixture.repo, &["tag", "release"]).unwrap();
    assert!(
        Hub::init(
            &fixture.hub.with_extension("tag"),
            &fixture.repo,
            "release",
            None
        )
        .is_err()
    );
    let second = fixture.hub.with_extension("two");
    Hub::init(&second, &fixture.repo, "main", None).unwrap();
    let first = owner(&fixture);
    let other = Hub::open(&second, OWNER, None).unwrap();
    first.create_task("A", "first hub", &[]).unwrap();
    other.create_task("B", "second hub", &[]).unwrap();
    let a = agent(&fixture, "codex-1").claim(None).unwrap();
    let b = Hub::open(&second, "codex-2", None)
        .unwrap()
        .claim(None)
        .unwrap();
    assert_ne!(a.branch, b.branch);
    assert!(a.worktree.unwrap().exists() && b.worktree.unwrap().exists());
}

#[test]
fn a_worker_loop_does_not_reclaim_the_task_it_just_released() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let first = owner
        .create_task("Explain", "Nothing to change", &[])
        .unwrap();
    let second = owner.create_task("Second", "Also nothing", &[]).unwrap();
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let mut released = Vec::new();
    for expected in [first, second] {
        let result = worker::run_one(
            &mut lyra,
            None,
            &released,
            4,
            &AtomicBool::new(false),
            &mut |report, save| {
                report.run(
                    &AtomicBool::new(false),
                    &mut |_| Ok(json!({"tool":"finish","summary":"No change"}).to_string()),
                    save,
                )
            },
        )
        .unwrap();
        assert_eq!(result["task"], expected);
        released.push(expected);
    }
    assert_eq!(
        worker::run_one(
            &mut lyra,
            None,
            &released,
            4,
            &AtomicBool::new(false),
            &mut |_, _| Ok(())
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn a_failed_commit_keeps_the_applied_task_claimed_and_retryable() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    // A hook that rejects commits stands in for failing signing.
    let hooks = fixture.repo.join(".git/hooks");
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let mut actions = vec![
        json!({"tool":"replace","path":"maths.py","old":"a - b","new":"a + b"}),
        json!({"tool":"finish","summary":"Use addition"}),
    ]
    .into_iter();
    let error = worker::run_one(
        &mut lyra,
        None,
        &[],
        8,
        &AtomicBool::new(false),
        &mut |report, save| {
            report.run(
                &AtomicBool::new(false),
                &mut |_| Ok(actions.next().unwrap().to_string()),
                save,
            )
        },
    )
    .unwrap_err()
    .to_string();
    let task = owner.task(id).unwrap();
    assert_eq!(task.status, Status::Claimed);
    assert_eq!(task.owner.as_deref(), Some("lyra-1"));
    let state = error
        .split("state: ")
        .nth(1)
        .expect(&error)
        .trim()
        .to_owned();
    assert!(
        owner.read(10).unwrap()[0]["body"]
            .as_str()
            .unwrap()
            .contains("could not commit")
    );
    // Once the cause is fixed the commit is retried from the saved state.
    fs::remove_file(&hook).unwrap();
    let result =
        worker::commit_applied(&lyra, id, Path::new(&state), &AtomicBool::new(false)).unwrap();
    assert_eq!(result["status"], "review");
    assert_eq!(owner.task(id).unwrap().status, Status::Review);
    owner.land_current(id).unwrap();
}

#[test]
fn a_worker_commit_contains_only_the_applied_proposal() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let mut actions = vec![
        json!({"tool":"replace","path":"maths.py","old":"a - b","new":"a + b"}),
        json!({"tool":"finish","summary":"Use addition"}),
    ]
    .into_iter();
    worker::run_one(
        &mut lyra,
        None,
        &[],
        8,
        &AtomicBool::new(false),
        &mut |report, save| {
            // Unrelated work already staged in the reused worktree.
            let worktree = report.request.repo.clone();
            fs::write(worktree.join("README.md"), "staged elsewhere\n").unwrap();
            git::ok(&worktree, &["add", "README.md"]).unwrap();
            report.run(
                &AtomicBool::new(false),
                &mut |_| Ok(actions.next().unwrap().to_string()),
                save,
            )
        },
    )
    .unwrap();
    let branch = owner.task(id).unwrap().branch.unwrap();
    let committed = git::ok(
        &fixture.repo,
        &[
            "show",
            "--name-only",
            "--format=",
            &format!("refs/heads/{branch}"),
        ],
    )
    .unwrap();
    assert_eq!(committed, "maths.py");
}

#[test]
fn a_failed_worktree_reverts_the_claim_and_a_claimed_task_gets_its_worktree_back() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    // A file where the worktrees folder should be makes the checkout fail.
    let blocker = fixture.hub.join("worktrees");
    fs::write(&blocker, "not a folder").unwrap();
    assert!(codex.claim(None).is_err());
    let task = owner.task(id).unwrap();
    assert_eq!((task.status, task.owner), (Status::Open, None));
    fs::remove_file(&blocker).unwrap();
    let task = codex.claim(None).unwrap();
    let worktree = task.worktree.unwrap();
    // An interrupted agent claims the same task again to recover a missing worktree.
    git::ok(
        &fixture.repo,
        &["worktree", "remove", "--force", &worktree.to_string_lossy()],
    )
    .unwrap();
    assert!(!worktree.exists());
    assert_eq!(codex.claim(Some(id)).unwrap().id, id);
    assert!(worktree.join("maths.py").exists());
}

#[test]
fn cancelled_dependencies_block_a_task_until_the_owner_unblocks_it() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let first = owner.create_task("Schema", "Add the schema", &[]).unwrap();
    let second = owner
        .create_task("API", "Use the schema", &[first])
        .unwrap();
    owner.cancel(first).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    let error = codex.claim(Some(second)).unwrap_err().to_string();
    assert!(
        error.contains("cancelled") && error.contains("unblock"),
        "{error}"
    );
    assert!(codex.unblock(second).is_err());
    assert_eq!(owner.unblock(second).unwrap().depends_on, Vec::<i64>::new());
    assert_eq!(codex.claim(Some(second)).unwrap().id, second);
}

#[test]
fn a_malformed_reviewed_commit_is_an_error_not_a_panic() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    codex.claim(None).unwrap();
    codex.update(id, Status::Review, "Done").unwrap();
    let error = owner.land(id, "ааааааа").unwrap_err();
    assert!(error.to_string().contains("review it again"), "{error}");
}

#[test]
fn landing_a_worker_task_removes_its_worktree_and_branch() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let mut actions = vec![
        json!({"tool":"replace","path":"maths.py","old":"a - b","new":"a + b"}),
        json!({"tool":"finish","summary":"Use addition"}),
    ]
    .into_iter();
    worker::run_one(
        &mut lyra,
        None,
        &[],
        8,
        &AtomicBool::new(false),
        &mut |report, save| {
            report.run(
                &AtomicBool::new(false),
                &mut |_| Ok(actions.next().unwrap().to_string()),
                save,
            )
        },
    )
    .unwrap();
    let task = owner.task(id).unwrap();
    let worktree = task.worktree.clone().unwrap();
    assert!(worktree.join(".lyra").exists());
    let result = owner.land_current(id).unwrap();
    assert_eq!(result["worktree_retained"], false);
    assert!(!worktree.exists());
    let branch = task.branch.unwrap();
    assert!(
        !git::run(
            &fixture.repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}")
            ]
        )
        .unwrap()
        .success
    );
}

#[test]
fn a_task_whose_check_cannot_run_stays_claimed_with_its_commit() {
    // A digest that is not present locally: the isolated check cannot start.
    let fixture = fixture(Some(ExecutionRequest {
        image: format!("python@sha256:{}", "0".repeat(64)),
        program: "python".into(),
        args: vec!["-c".into(), "print(1)".into()],
        timeout_secs: 15,
        memory_mib: 256,
    }));
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let task = lyra.claim(None).unwrap();
    let worktree = task.worktree.unwrap();
    fs::write(
        worktree.join("maths.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    git::ok(&worktree, &["commit", "-qam", "work"]).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(worker::hand_off(&lyra, id, "Done", &cancel, Path::new("state")).is_err());
    let task = owner.task(id).unwrap();
    assert_eq!(task.status, Status::Claimed);
    assert!(task.check.is_none());
}

#[test]
fn landing_needs_the_full_reviewed_commit_and_the_inbox_view_keeps_messages_unread() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner.create_task("Fix add", "Use addition", &[]).unwrap();
    let mut codex = agent(&fixture, "codex-1");
    codex.claim(None).unwrap();
    codex.update(id, Status::Review, "Done").unwrap();
    let commit = owner.diff(id, 4096).unwrap()["commit"]
        .as_str()
        .unwrap()
        .to_owned();
    // A prefix of the right commit is not enough.
    assert!(owner.land(id, &commit[..12]).is_err());
    assert_eq!(owner.task(id).unwrap().status, Status::Review);
    // Viewing the inbox does not mark messages read.
    codex.send(Some(OWNER), None, "ping").unwrap();
    assert_eq!(owner.recent_messages(10).unwrap().len(), 1);
    assert_eq!(owner.recent_messages(10).unwrap().len(), 1);
    assert_eq!(owner.read(10).unwrap().len(), 1);
    assert!(owner.read(10).unwrap().is_empty());
    assert_eq!(owner.recent_messages(10).unwrap().len(), 1);
}

#[test]
fn text_limits_count_characters_like_the_mcp_schema() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    // 150 Cyrillic characters are 300 bytes: valid for a 200-character title.
    let title = "ж".repeat(150);
    let id = owner.create_task(&title, "ok", &[]).unwrap();
    assert!(owner.create_task(&"ж".repeat(201), "ok", &[]).is_err());
    let mut codex = agent(&fixture, "codex-1");
    codex.claim(None).unwrap();
    // 5000 characters (10 KB) fit an 8192-character summary and note.
    let summary = "я".repeat(5000);
    codex.update(id, Status::Review, &summary).unwrap();
    owner.reopen(id, &summary).unwrap();
}

#[test]
fn leases_may_cover_ordinary_dot_folders_but_not_git_or_escapes() {
    let fixture = fixture(None);
    let mut codex = agent(&fixture, "codex-1");
    let paths = |items: &[&str]| {
        items
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        codex
            .lease(
                None,
                &paths(&["src/lib.rs", ".github/workflows/ci.yml"]),
                600
            )
            .unwrap()["leased"],
        true
    );
    for bad in [".git/config", "src/.LYRA/x", "../x", "/abs", "a//b", "a\\b"] {
        assert!(codex.lease(None, &paths(&[bad]), 600).is_err(), "{bad}");
    }
}
