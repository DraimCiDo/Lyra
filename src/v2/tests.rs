use super::*;
use std::{collections::VecDeque, fs};

const IMAGE: &str =
    "python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1";
const ORIGINAL: &str = "def add(a, b):\r\n    return a - b\r\n";
const REPLACEMENT: &str = "def add(a, b):\r\n    return a + b\r\n";

fn fixture(with_check: bool) -> (tempfile::TempDir, TaskReport) {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("project");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("maths.py"), ORIGINAL).unwrap();
    let report = TaskReport::new(TaskRequest {
        repo,
        task: "Repair addition".into(),
        files: vec!["maths.py".into()],
        check: with_check.then(|| ExecutionRequest {
            image: IMAGE.into(),
            program: "python".into(),
            args: vec![
                "-c".into(),
                "from maths import add; assert add(2, 3) == 5; assert add(-4, 4) == 0".into(),
            ],
            timeout_secs: 15,
            memory_mib: 256,
        }),
        max_calls: 12,
        reviewer: false,
    })
    .unwrap();
    (root, report)
}

fn reply(actions: impl IntoIterator<Item = Value>) -> impl FnMut(&str) -> io::Result<String> {
    let mut actions = actions.into_iter().collect::<VecDeque<_>>();
    move |_| {
        actions
            .pop_front()
            .map(|value| value.to_string())
            .ok_or_else(|| io::Error::other("Unexpected extra model call"))
    }
}

fn edit(old: &str, new: &str) -> Value {
    json!({"tool":"replace","path":"maths.py","old":old,"new":new})
}

fn finish() -> Value {
    json!({"tool":"finish","summary":"Fixed addition"})
}

#[test]
fn failed_candidate_is_repaired_and_checked_before_explicit_apply() {
    let (root, mut report) = fixture(true);
    let state = root.path().join("job");
    let store = TaskStore::create(&state, &report).unwrap();
    assert!(TaskStore::open(&state).is_err());
    let mut checks = 0;
    report
        .run_with_checker(
            &AtomicBool::new(false),
            &mut reply([
                json!({"tool":"create","path":"other.py","content":"outside the selection"}),
                json!({"tool":"read","path":"maths.py","start":1}),
                edit("a - b", "a * b"),
                finish(),
                finish(),
                edit("a * b", "a + b"),
                finish(),
            ]),
            &mut |request, proposals, _| {
                checks += 1;
                assert_eq!(fs::read_to_string(request.repo.join("maths.py"))?, ORIGINAL);
                let passed = proposals[0].replacement == REPLACEMENT;
                Ok(VerificationResult {
                    exit_code: Some(if passed { 0 } else { 1 }),
                    timed_out: false,
                    cancelled: false,
                    stdout: String::new(),
                    stderr: if passed {
                        String::new()
                    } else {
                        "2 + 3 must equal 5".into()
                    },
                    output_truncated: false,
                    elapsed_ms: 1,
                    cleanup_confirmed: true,
                    source_sha256: Some(executor::candidate_source_hash(&request.repo, proposals)?),
                    source_scope: vec!["maths.py".into()],
                })
            },
            &mut |report| store.save(report),
        )
        .unwrap();
    assert_eq!(checks, 2);
    assert_eq!(report.state, TaskState::ReadyToApply);
    assert!(!report.events[0].ok);
    assert!(!report.events[3].ok);
    assert!(!report.events[4].ok);
    assert!(matches!(report.verification, Verification::Passed { .. }));
    assert_eq!(
        fs::read_to_string(report.request.repo.join("maths.py")).unwrap(),
        ORIGINAL
    );
    let added = report.request.repo.join("unrelated.py");
    fs::write(&added, "user data").unwrap();
    assert!(store.apply().is_err());
    assert_eq!(fs::read_to_string(&added).unwrap(), "user data");
    fs::remove_file(added).unwrap();
    assert_eq!(store.apply().unwrap().state, TaskState::Applied);
    assert_eq!(store.apply().unwrap().state, TaskState::Applied);
    assert_eq!(
        fs::read_to_string(report.request.repo.join("maths.py")).unwrap(),
        REPLACEMENT
    );
}

#[test]
fn cancellation_resumes_without_replaying_the_completed_action() {
    let (root, mut report) = fixture(false);
    let state = root.path().join("job");
    let store = TaskStore::create(&state, &report).unwrap();
    let cancel = AtomicBool::new(false);
    report
        .run(
            &cancel,
            &mut reply([edit("a - b", "a + b")]),
            &mut |report| {
                if report.calls == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
                store.save(report)
            },
        )
        .unwrap();
    assert_eq!(report.state, TaskState::Cancelled);
    drop(store);
    let store = TaskStore::open(&state).unwrap();
    let mut restored = store.load().unwrap();
    restored
        .run(
            &AtomicBool::new(false),
            &mut reply([finish()]),
            &mut |report| store.save(report),
        )
        .unwrap();
    assert_eq!(restored.calls, 2);
    assert_eq!(restored.state, TaskState::ReadyToApply);
    fs::write(restored.request.repo.join("maths.py"), "external edit\n").unwrap();
    assert!(store.apply().is_err());
    assert_eq!(
        fs::read_to_string(restored.request.repo.join("maths.py")).unwrap(),
        "external edit\n"
    );
}

#[test]
fn unavailable_checker_prevents_apply() {
    let (root, mut report) = fixture(true);
    let store = TaskStore::create(&root.path().join("job"), &report).unwrap();
    report
        .run_with_checker(
            &AtomicBool::new(false),
            &mut reply([edit("a - b", "a + b"), finish()]),
            &mut |_, _, _| Err(io::Error::other("Docker daemon unavailable")),
            &mut |report| store.save(report),
        )
        .unwrap();
    assert_eq!(report.state, TaskState::Failed);
    assert!(matches!(
        report.verification,
        Verification::Unavailable { .. }
    ));
    assert!(store.apply().is_err());
}

#[test]
#[ignore = "requires local Docker and the digest-pinned Python image"]
fn independent_docker_check_fails_on_baseline_and_passes_after_apply() {
    let (root, mut report) = fixture(true);
    let store = TaskStore::create(&root.path().join("job"), &report).unwrap();
    let cancel = AtomicBool::new(false);
    let check = report.request.check.clone().unwrap();
    let baseline = executor::verify(&report.request.repo, &[], &check, &cancel).unwrap();
    assert_eq!(baseline.exit_code, Some(1), "{baseline:?}");
    assert!(baseline.cleanup_confirmed, "{baseline:?}");
    report
        .run(
            &cancel,
            &mut reply([edit("a - b", "a + b"), finish()]),
            &mut |report| store.save(report),
        )
        .unwrap();
    assert_eq!(report.state, TaskState::ReadyToApply, "{report:?}");
    assert_eq!(
        fs::read_to_string(report.request.repo.join("maths.py")).unwrap(),
        ORIGINAL
    );
    store.apply().unwrap();
    let applied = executor::verify(&report.request.repo, &[], &check, &cancel).unwrap();
    assert!(applied.passed(), "{applied:?}");
}

#[test]
fn reviewer_cannot_edit_and_a_revision_needs_a_new_approval() {
    let (_root, mut report) = fixture(false);
    report.request.reviewer = true;
    let approval = json!({"tool":"review","approved":true,"summary":"Looks correct"});
    let rejection = json!({"tool":"review","approved":false,"summary":"Use addition"});
    report
        .run(
            &AtomicBool::new(false),
            &mut reply([
                edit("a - b", "a * b"),
                finish(),
                edit("a * b", "discard this reviewer edit"),
                finish(),
                rejection,
                edit("a * b", "a + b"),
                finish(),
                approval,
            ]),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(report.state, TaskState::ReadyToApply);
    assert_eq!(report.calls, 8);
    assert_eq!(report.proposals[0].replacement, REPLACEMENT);
    assert!(!report.events[1].ok);
    assert!(!report.events[2].ok);
    assert_eq!(report.summary, "Fixed addition");
}

#[test]
fn reviewer_approval_finishes_within_a_tight_budget() {
    let (_root, mut report) = fixture(false);
    report.request.reviewer = true;
    report.request.max_calls = 3;
    report
        .run(
            &AtomicBool::new(false),
            &mut reply([
                edit("a - b", "a + b"),
                finish(),
                json!({"tool":"review","approved":true,"summary":"Correct"}),
            ]),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(report.state, TaskState::ReadyToApply);
    assert_eq!(report.calls, 3);
}

#[test]
fn local_model_session_accepts_the_native_loader_configuration() {
    let folder = tempfile::tempdir().unwrap();
    let model = folder.path().join("model.gguf");
    fs::write(&model, b"GGUF\x03\0\0\0").unwrap();
    let args = cli::LocalModel {
        model,
        context: 4096,
        max_tokens: 512,
        threads: 1,
        timeout_secs: 15,
    }
    .inference_args();
    crate::inference::validate(&args).unwrap();
}

#[test]
fn structured_generation_stops_at_the_first_complete_action() {
    assert!(!action_complete(
        br#"{"tool":"finish","summary":"example }"#
    ));
    assert!(!action_complete(br#"{"tool":"finish"}"#));
    assert!(action_complete(
        br#"{"tool":"finish","summary":"example }"}  "#
    ));
}

#[test]
fn replayed_edit_produces_a_reviewable_candidate_without_claiming_verification() {
    let (_root, mut report) = fixture(false);
    let replacement = edit("a - b", "a + b");
    report
        .run(
            &AtomicBool::new(false),
            &mut reply([replacement.clone(), replacement.clone(), replacement]),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(report.state, TaskState::ReadyToApply);
    assert_eq!(report.calls, 3);
    assert_eq!(report.events[1].action, "repeat_replace");
    assert!(report.summary.contains("repeated"));
    assert!(matches!(report.verification, Verification::Skipped));
    assert_eq!(report.proposals[0].replacement, REPLACEMENT);
    assert_eq!(
        fs::read_to_string(report.request.repo.join("maths.py")).unwrap(),
        ORIGINAL
    );
}

#[test]
fn a_rejected_replacement_returns_the_current_file_for_recovery() {
    let (_root, mut report) = fixture(false);
    report.request.files.clear();
    let mut step = 0;
    report
        .run(
            &AtomicBool::new(false),
            &mut |prompt| {
                step += 1;
                match step {
                    1 => Ok(edit("undefined text", "unused").to_string()),
                    2 => {
                        let prompt: Value = serde_json::from_str(prompt).unwrap();
                        let latest = &prompt["observations"][0]["result"];
                        assert_eq!(latest["current_file"]["path"], "maths.py");
                        assert_eq!(latest["current_file"]["text"], ORIGINAL);
                        Ok(edit("a - b", "a + b").to_string())
                    }
                    _ => Ok(finish().to_string()),
                }
            },
            &mut |_| Ok(()),
        )
        .unwrap();
    assert!(!report.events[0].ok);
    assert_eq!(report.state, TaskState::ReadyToApply);
    assert_eq!(report.proposals[0].replacement, REPLACEMENT);
}

#[test]
fn a_line_longer_than_one_page_is_read_in_chunks() {
    let (_root, mut report) = fixture(false);
    let long = format!("x = '{}'\n", "é".repeat(3000));
    fs::write(
        report.request.repo.join("long.py"),
        format!("a = 1\n{long}b = 2\n"),
    )
    .unwrap();
    report.request.files.clear();
    let mut workspace = Workspace::open(&report.request.repo, &[]).unwrap();
    let first = report.read(&mut workspace, "long.py", 1, 0).unwrap();
    assert_eq!(first["text"], "a = 1\n");
    assert_eq!(first["next"], 2);
    let mut text = String::new();
    let mut byte = 0;
    loop {
        let page = report.read(&mut workspace, "long.py", 2, byte).unwrap();
        let chunk = page["text"].as_str().unwrap();
        assert!(chunk.len() <= 4096 && !chunk.is_empty());
        text.push_str(chunk);
        match page["next_byte"].as_u64() {
            Some(next) => byte = next as usize,
            None => {
                assert_eq!(page["next"], Value::Null);
                break;
            }
        }
    }
    assert_eq!(text, format!("{long}b = 2\n"));
    assert!(report.read(&mut workspace, "long.py", 2, 6).is_err());
}

#[test]
fn conflicting_created_paths_are_rejected_before_they_are_saved() {
    let (_root, mut report) = fixture(false);
    report.request.files.clear();
    report
        .run(
            &AtomicBool::new(false),
            &mut reply([
                json!({"tool":"create","path":"generated","content":"a"}),
                json!({"tool":"create","path":"generated/data.txt","content":"b"}),
                finish(),
            ]),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert!(report.events[0].ok);
    assert!(!report.events[1].ok);
    assert_eq!(report.proposals.len(), 1);
    assert_eq!(report.state, TaskState::ReadyToApply);
}

#[test]
fn truncated_search_results_are_marked_incomplete() {
    let (_root, mut report) = fixture(false);
    report.request.files.clear();
    for index in 0..9 {
        fs::write(
            report.request.repo.join(format!("m{index}.py")),
            "needle = 1\n",
        )
        .unwrap();
    }
    let mut workspace = Workspace::open(&report.request.repo, &[]).unwrap();
    let (_, result) = report.action(
        Action::Search {
            query: "needle".into(),
            offset: 0,
        },
        &mut workspace,
    );
    let result = result.unwrap();
    assert_eq!(result["matches"].as_array().unwrap().len(), 8);
    assert_eq!(result["incomplete"], true);
}

#[test]
fn a_failed_first_checkpoint_leaves_the_state_path_retryable() {
    let (root, mut report) = fixture(false);
    let state = root.path().join("job");
    report.summary = "x".repeat(5 * 1024 * 1024);
    assert!(TaskStore::create(&state, &report).is_err());
    assert!(!state.exists());
    report.summary.clear();
    let store = TaskStore::create(&state, &report).unwrap();
    assert_eq!(store.load().unwrap().state, TaskState::Running);
}

#[test]
fn an_accepted_edit_shows_the_proposed_file_to_the_model() {
    let (_root, mut report) = fixture(false);
    let mut step = 0;
    report
        .run(
            &AtomicBool::new(false),
            &mut |prompt| {
                step += 1;
                if step == 1 {
                    return Ok(edit("a - b", "a + b").to_string());
                }
                let prompt: Value = serde_json::from_str(prompt).unwrap();
                let latest = prompt["observations"].as_array().unwrap().last().unwrap();
                assert_eq!(latest["result"]["proposed_file"]["text"], REPLACEMENT);
                assert!(
                    latest["result"]["next_step"]
                        .as_str()
                        .unwrap()
                        .contains("finish")
                );
                Ok(finish().to_string())
            },
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(report.state, TaskState::ReadyToApply);
}

#[test]
fn protocol_examples_use_a_real_project_file() {
    let (_root, mut report) = fixture(false);
    report.request.files.clear();
    report
        .run(
            &AtomicBool::new(false),
            &mut |prompt| {
                let prompt: Value = serde_json::from_str(prompt).unwrap();
                let instructions = prompt["instructions"].as_str().unwrap();
                assert!(
                    instructions.contains(r#""path":"maths.py""#),
                    "{instructions}"
                );
                assert!(!instructions.contains("src/lib.rs"));
                Ok(finish().to_string())
            },
            &mut |_| Ok(()),
        )
        .unwrap();
}
