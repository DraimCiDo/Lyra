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
        assert_eq!(branch, &format!("lyra/task-{id}"));
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
    assert!(codex.land(first).is_err());
    owner.land(first).unwrap();
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
    assert_eq!(codex.release(&[]).unwrap(), 2);
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
            .land(id)
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
            .land(id)
            .unwrap_err()
            .to_string()
            .contains("uncommitted")
    );
    git::ok(&fixture.repo, &["checkout", "--", "README.md"]).unwrap();
    owner.land(id).unwrap();
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
    let error = owner.land(id).unwrap_err();
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
    assert!(owner.land(id).is_err());
    let current = json!({"passed": true, "head": head, "clean": true, "exit_code": 0,
        "timed_out": false, "elapsed_ms": 1, "output": ""});
    owner
        .connection
        .execute(
            "UPDATE tasks SET check_result=?2 WHERE id=?1",
            params![id, current.to_string()],
        )
        .unwrap();
    owner.land(id).unwrap();
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
    let result = worker::run_one(&mut lyra, None, 8, &mut |report, save| {
        report.run(
            &AtomicBool::new(false),
            &mut |prompt| {
                assert!(prompt.contains("untrusted data"));
                Ok(actions.next().unwrap().to_string())
            },
            save,
        )
    })
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
    owner.land(id).unwrap();
    assert!(
        fs::read_to_string(fixture.repo.join("maths.py"))
            .unwrap()
            .contains("a + b")
    );
}

#[test]
fn a_worker_that_finds_no_change_releases_the_task() {
    let fixture = fixture(None);
    let owner = owner(&fixture);
    let id = owner
        .create_task("Explain", "Nothing to change", &[])
        .unwrap();
    let mut lyra = Hub::open(&fixture.hub, "lyra-1", Some(AgentKind::Lyra)).unwrap();
    let result = worker::run_one(&mut lyra, None, 4, &mut |report, save| {
        report.run(
            &AtomicBool::new(false),
            &mut |_| Ok(json!({"tool":"finish","summary":"No change needed"}).to_string()),
            save,
        )
    })
    .unwrap();
    assert_eq!(result["status"], "open");
    assert_eq!(owner.task(id).unwrap().status, Status::Open);
    assert_eq!(owner.read(10).unwrap().len(), 1);
}
