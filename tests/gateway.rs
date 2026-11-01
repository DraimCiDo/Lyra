#![cfg(feature = "gateway-http")]

use std::process::Command;

#[test]
fn tool_pool_reduces_real_http_body_and_restores_identical_context() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let server = std::thread::spawn(move || {
        let mut bodies = Vec::new();
        for call in 0..8 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            let boundary = loop {
                if let Some(position) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break position + 4;
                }
                let mut chunk = [0u8; 4096];
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                assert!(bytes.len() < 2 * 1024 * 1024);
            };
            let head = String::from_utf8_lossy(&bytes[..boundary]);
            assert!(
                head.to_ascii_lowercase()
                    .contains("content-type: application/json")
            );
            let length: usize = head
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            assert!(length < 2 * 1024 * 1024);
            while bytes.len() - boundary < length {
                let mut chunk = [0u8; 4096];
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            bodies.push(bytes[boundary..boundary + length].to_vec());
            let action = if call % 4 == 3 {
                serde_json::json!({"tool":"finish","summary":"Read only; tests not run."})
            } else {
                serde_json::json!({"tool":"read_file","path":"source.txt"})
            };
            let body = serde_json::json!({"choices":[{"message":{"content":action.to_string()}}]})
                .to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        bodies
    });
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(repo.join("source.txt"), "exact source\r\n".repeat(500)).unwrap();
    let task = dir.path().join("task.txt");
    std::fs::write(&task, "Inspect; do not change files.").unwrap();
    let config = dir.path().join("providers.json");
    std::fs::write(&config, serde_json::json!({"descriptor":{"provider":"local","model":"test",
        "capabilities":{"tool_calls":false,"vision":false,"prompt_cache":false,"max_context_tokens":8192}},
        "endpoint":endpoint,"api_key_env":null}).to_string()).unwrap();
    let mut reports = Vec::new();
    let mut outputs = Vec::new();
    for dedup in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lyra"));
        command
            .arg("task")
            .arg("--repo")
            .arg(&repo)
            .arg("--task-file")
            .arg(&task)
            .arg("--provider-config")
            .arg(&config)
            .args(["--allow", "local", "--max-calls", "4"]);
        if dedup {
            command.arg("--dedup-tool-context");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        reports.push(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap());
        outputs.push(output);
    }
    let bodies = server.join().unwrap();
    assert!(bodies[7].len() < bodies[3].len());
    let decode = |body: &[u8]| {
        let wire: serde_json::Value = serde_json::from_slice(body).unwrap();
        serde_json::from_str::<serde_json::Value>(wire["messages"][1]["content"].as_str().unwrap())
            .unwrap()
    };
    assert_eq!(
        lyra::context_optimizer::restore(&decode(&bodies[7])).unwrap(),
        decode(&bodies[3])
    );
    assert_eq!(reports[1]["schema_version"], 3);
    assert_eq!(reports[1]["context_optimization"][3]["references"], 2);
    assert!(
        String::from_utf8_lossy(&outputs[1].stderr)
            .contains(&format!("request_body_bytes={}", bodies[7].len()))
    );
}

#[test]
fn cloud_task_timeout_covers_multiple_calls_and_response_body() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let server = std::thread::spawn(move || {
        let stop = Instant::now() + Duration::from_secs(5);
        for action in [
            r#"{"tool":"list"}"#,
            r#"{"tool":"finish","summary":"done"}"#,
        ] {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < stop =>
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 8192];
            if stream.read(&mut request).is_err() {
                return;
            }
            let body = serde_json::json!({"choices":[{"message":{"content":action}}]}).to_string();
            if write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).is_err() { return; }
            // Each response fits a one-second per-request timeout; together they do not.
            std::thread::sleep(Duration::from_millis(600));
            let _ = stream.write_all(body.as_bytes());
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("providers.json");
    let task = dir.path().join("task.txt");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(repo.join("hello.txt"), "hello").unwrap();
    std::fs::write(&task, "List files then finish").unwrap();
    std::fs::write(
        &config,
        serde_json::json!({
            "descriptor": {"provider":"local", "model":"test", "capabilities":{
                "tool_calls":false, "vision":false, "prompt_cache":false, "max_context_tokens":4096
            }},
            "endpoint":endpoint, "api_key_env":null
        })
        .to_string(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .arg("task")
        .arg("--repo")
        .arg(&repo)
        .arg("--task-file")
        .arg(&task)
        .arg("--provider-config")
        .arg(&config)
        .args([
            "--allow",
            "local",
            "--timeout-secs",
            "1",
            "--max-calls",
            "3",
        ])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        !output.status.success(),
        "task must not reset its deadline per request"
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "timeout");
    assert!(report["calls"].as_u64().unwrap() <= 2);
}

#[test]
fn cli_refuses_unapproved_provider_without_network_access() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("providers.json");
    std::fs::write(
        &config,
        r#"[{"descriptor":{"provider":"cloud","model":"test","capabilities":{"tool_calls":false,"vision":false,"prompt_cache":false,"max_context_tokens":4096}},"endpoint":"http://127.0.0.1:1/v1/chat/completions","api_key_env":null}]"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .arg("chat")
        .arg("--provider-config")
        .arg(&config)
        .args(["--allow", "local", "--prompt", "private content"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!error.contains("private content"));
    assert!(!error.contains("panicked"));
    assert!(output.stdout.is_empty());
}

#[test]
fn cli_rejects_zero_attempts_without_sending_a_request() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("providers.json");
    std::fs::write(
        &config,
        r#"{"descriptor":{"provider":"local","model":"test","capabilities":{"tool_calls":false,"vision":false,"prompt_cache":false,"max_context_tokens":4096}},"endpoint":"http://127.0.0.1:1/v1/chat/completions","api_key_env":null}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .arg("chat")
        .arg("--provider-config")
        .arg(&config)
        .args([
            "--allow",
            "local",
            "--max-attempts",
            "0",
            "--prompt",
            "test",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Routing attempts must be positive"));
}

#[test]
fn cli_rejects_invalid_request_file_before_http() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("providers.json");
    let request = dir.path().join("request.json");
    std::fs::write(
        &config,
        r#"{"descriptor":{"provider":"local","model":"test","capabilities":{"tool_calls":false,"vision":false,"prompt_cache":false,"max_context_tokens":4096}},"endpoint":"http://127.0.0.1:1/v1/chat/completions","api_key_env":null}"#,
    )
    .unwrap();
    std::fs::write(&request, b"not json").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .arg("chat")
        .arg("--provider-config")
        .arg(&config)
        .args(["--allow", "local", "--model", "test"])
        .arg("--request-file")
        .arg(&request)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Invalid chat request"));
    assert!(output.stdout.is_empty());
}
