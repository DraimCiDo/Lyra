use lyra::executor::{self, ExecutionRequest};
use std::sync::atomic::AtomicBool;

#[test]
#[ignore = "requires Docker and LYRA_TEST_IMAGE pointing to a locally installed digest-pinned Python image"]
fn docker_checks_are_offline_readonly_bounded_and_reap_children() {
    let image = std::env::var("LYRA_TEST_IMAGE").expect("explicit pinned test image");
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("input.txt"), "source").unwrap();
    std::fs::write(repo.path().join(".env"), "PRIVATE_SENTINEL").unwrap();
    let script = r#"
import pathlib,socket,os
assert pathlib.Path('/source/input.txt').read_text() == 'source'
assert not pathlib.Path('/source/.env').exists()
assert not pathlib.Path('/var/run/docker.sock').exists()
assert os.getuid() != 0
cgroup = pathlib.Path('/sys/fs/cgroup')
if (cgroup / 'memory.max').exists():
    assert int((cgroup / 'memory.max').read_text()) == 256 * 1024 * 1024
    assert int((cgroup / 'pids.max').read_text()) == 128
    quota, period = map(int, (cgroup / 'cpu.max').read_text().split())
    assert quota / period == 2
try: pathlib.Path('/source/input.txt').write_text('clobber')
except OSError: pass
else: raise AssertionError('source writable')
try: pathlib.Path('/etc/escape').write_text('escape')
except OSError: pass
else: raise AssertionError('root writable')
s = socket.socket(); s.settimeout(.2)
try: s.connect(('1.1.1.1',443))
except OSError: pass
else: raise AssertionError('network available')
print('verified isolation')
"#;
    let mut request = ExecutionRequest {
        image,
        program: "python".into(),
        args: vec!["-c".into(), script.into()],
        timeout_secs: 15,
        memory_mib: 256,
    };
    let result = executor::verify(repo.path(), &[], &request, &AtomicBool::new(false)).unwrap();
    assert!(result.passed(), "{result:?}");
    assert!(result.stdout.contains("verified isolation"));
    assert_eq!(
        std::fs::read_to_string(repo.path().join("input.txt")).unwrap(),
        "source"
    );
    request.args = vec!["-c".into(), "print('x'*200000)".into()];
    let result = executor::verify(repo.path(), &[], &request, &AtomicBool::new(false)).unwrap();
    assert!(result.passed());
    assert!(result.output_truncated);
    assert!(result.stdout.len() <= 65536);
    request.memory_mib = 128;
    request.args = vec!["-c".into(), "data = bytearray(512 * 1024 * 1024)".into()];
    let result = executor::verify(repo.path(), &[], &request, &AtomicBool::new(false)).unwrap();
    assert!(!result.passed());
    assert!(result.cleanup_confirmed);
    request.memory_mib = 256;
    request.args = vec![
        "-c".into(),
        r#"
import subprocess
children = []
blocked = False
try:
    for _ in range(160):
        try: children.append(subprocess.Popen(['sleep','20']))
        except OSError:
            blocked = True
            break
    assert blocked and 0 < len(children) < 128
finally:
    for child in children: child.kill()
    for child in children: child.wait()
"#
        .into(),
    ];
    let result = executor::verify(repo.path(), &[], &request, &AtomicBool::new(false)).unwrap();
    assert!(result.passed(), "{result:?}");
    request.timeout_secs = 2;
    request.args = vec![
        "-c".into(),
        "import subprocess,time; subprocess.Popen(['sleep','60']); time.sleep(60)".into(),
    ];
    let result = executor::verify(repo.path(), &[], &request, &AtomicBool::new(false)).unwrap();
    assert!(!result.passed());
    assert!(result.timed_out);
    assert!(result.cleanup_confirmed);
    request.timeout_secs = 15;
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let signal = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(1));
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let result = executor::verify(repo.path(), &[], &request, &cancel).unwrap();
    signal.join().unwrap();
    assert!(result.cancelled);
    assert!(result.cleanup_confirmed);
    assert!(!result.passed());
}

#[test]
#[ignore = "requires Docker and a locally installed LYRA_TEST_IMAGE"]
fn agent_repairs_a_real_failed_check_and_never_reuses_stale_evidence() {
    use lyra::agent::{self, Model, WorkflowOptions};
    struct Script(std::collections::VecDeque<serde_json::Value>);
    impl Model for Script {
        fn respond(&mut self, _: &str) -> std::io::Result<String> {
            self.0
                .pop_front()
                .map(|v| v.to_string())
                .ok_or_else(|| std::io::Error::other("script exhausted"))
        }
    }
    let image = std::env::var("LYRA_TEST_IMAGE").unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(
        repo.path().join("maths.py"),
        "def add(a, b):\r\n    return a - b\r\n",
    )
    .unwrap();
    let request = ExecutionRequest {
        image,
        program: "python".into(),
        args: vec![
            "-c".into(),
            "from maths import add; assert add(2, 3) == 5; assert add(-4, 4) == 0".into(),
        ],
        timeout_secs: 15,
        memory_mib: 256,
    };
    let mut workspace = lyra::workspace::Workspace::open(repo.path(), &[]).unwrap();
    let mut script = Script([
        serde_json::json!({"tool":"verify"}),
        serde_json::json!({"tool":"propose_edit","path":"maths.py","old":"a - b","new":"a + b"}),
        serde_json::json!({"tool":"verify"}),
        serde_json::json!({"tool":"finish","summary":"Fixed and checked"}),
    ].into());
    let docs = lyra::knowledge::Pack::empty();
    let report = agent::execute_workflow(
        &mut script,
        &mut workspace,
        "Fix addition",
        WorkflowOptions {
            subagents: false,
            max_calls: 6,
            docs: &docs,
            dedup_tool_context: false,
            check: Some(&request),
            deadline: None,
            previous: None,
            checkpoint: None,
        },
    );
    assert_eq!(report.status, "completed", "{report:?}");
    assert_eq!(report.outcome, agent::TaskOutcome::ChangesVerified);
    assert_eq!(report.verifications.len(), 2);
    assert!(!report.verifications[0].result.passed());
    assert!(report.verifications[1].result.passed());
    assert!(
        std::fs::read_to_string(repo.path().join("maths.py"))
            .unwrap()
            .contains("a - b")
    );
    let mut script = Script([
        serde_json::json!({"tool":"propose_edit","path":"maths.py","old":"a - b","new":"a + b"}),
        serde_json::json!({"tool":"verify"}),
        serde_json::json!({"tool":"propose_edit","path":"maths.py","old":"a + b","new":"a * b"}),
        serde_json::json!({"tool":"finish","summary":"Everything passes"}),
    ].into());
    let report = agent::execute_workflow(
        &mut script,
        &mut workspace,
        "Fix addition",
        WorkflowOptions {
            subagents: false,
            max_calls: 6,
            docs: &docs,
            dedup_tool_context: false,
            check: Some(&request),
            deadline: None,
            previous: None,
            checkpoint: None,
        },
    );
    assert_eq!(report.status, "verification_required");
    assert_eq!(report.outcome, agent::TaskOutcome::Incomplete);
}

#[test]
#[ignore = "requires Docker; validates the host's actual capability report without running code"]
fn docker_preflight_matches_actual_resource_enforcement() {
    let output = std::process::Command::new("docker")
        .args(["info", "--format", "{{json .}}"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let info: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected = info["OSType"] == "linux"
        && info["MemoryLimit"] == true
        && info["SwapLimit"] == true
        && info["PidsLimit"] == true
        && info["CpuCfsQuota"] == true
        && info["CpuCfsPeriod"] == true
        && info["SecurityOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| {
                option
                    .as_str()
                    .is_some_and(|text| text.starts_with("name=seccomp"))
            });
    let result = executor::preflight(&AtomicBool::new(false));
    assert_eq!(result.is_ok(), expected);
    if !expected {
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Unsupported);
    }
}
