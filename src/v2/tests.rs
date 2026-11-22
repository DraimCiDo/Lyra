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
fn reviewer_cannot_edit_and_approval_is_invalidated_by_a_later_revision() {
    let (_root, mut report) = fixture(false);
    report.request.reviewer = true;
    let approval = json!({"tool":"review","approved":true,"summary":"Looks correct"});
    report
        .run(
            &AtomicBool::new(false),
            &mut reply([
                edit("a - b", "a * b"),
                finish(),
                edit("a * b", "discard this reviewer edit"),
                finish(),
                approval.clone(),
                edit("a * b", "a + b"),
                finish(),
                approval,
                finish(),
            ]),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(report.state, TaskState::ReadyToApply);
    assert_eq!(report.calls, 9);
    assert_eq!(report.proposals[0].replacement, REPLACEMENT);
    assert!(!report.events[1].ok);
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
