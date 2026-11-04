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
        "capabilities":{"tool_calls":false,"vision":false,"prompt_cache":false,"max_context_tokens":32768}},
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
            .args(["--allow", "local", "--max-calls", "4", "--context", "32768"]);
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
    assert_eq!(reports[1]["schema_version"], 4);
    assert_eq!(reports[1]["context_optimization"][3]["references"], 2);
    assert!(
        String::from_utf8_lossy(&outputs[1].stderr)
            .contains(&format!("request_body_bytes={}", bodies[7].len()))
    );
}

#[test]
fn cloud_task_timeout_covers_multiple_calls_and_response_body() {
    use std::io::{BufRead, BufReader, Read, Write};
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
            // Accepted sockets may inherit nonblocking mode on Windows. A first
            // read before the client writes must wait, not tear down the server.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            {
                let mut reader = BufReader::new(&mut stream);
                let mut length = None;
                let mut header_bytes = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    header_bytes += line.len();
                    assert!(header_bytes < 32768);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':')
                        && key.eq_ignore_ascii_case("content-length")
                    {
                        length = Some(value.trim().parse::<usize>().unwrap());
                    }
                }
                let length = length.unwrap();
                assert!(length < 2 * 1024 * 1024);
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
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
    assert_eq!(output.status.code(), Some(124));
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

#[test]
fn cli_checkpoint_resume_preserves_pending_edits_and_original_project_identity() {
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let server = std::thread::spawn(move || {
        for call in 0..3 {
            let started = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(started.elapsed().as_secs() < 20, "CLI never sent request");
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let bytes = {
                let mut reader = std::io::BufReader::new(&mut stream);
                let mut length = None;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':')
                        && key.eq_ignore_ascii_case("content-length")
                    {
                        length = Some(value.trim().parse::<usize>().unwrap());
                    }
                }
                let length = length.unwrap();
                assert!(length < 2 * 1024 * 1024);
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                bytes
            };
            assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE_SENTINEL"));
            let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let envelope: serde_json::Value =
                serde_json::from_str(wire["messages"][1]["content"].as_str().unwrap()).unwrap();
            let action = match call {
                0 => {
                    serde_json::json!({"tool":"propose_edit","path":"source.py","old":"answer = 0","new":"answer = 41"})
                }
                1 => {
                    assert!(
                        envelope["proposals"][0]["replacement"]
                            .as_str()
                            .unwrap()
                            .contains("41")
                    );
                    serde_json::json!({"tool":"propose_edit","path":"source.py","old":"answer = 41","new":"answer = 42"})
                }
                _ => serde_json::json!({"tool":"finish","summary":"Prepared; tests not run"}),
            };
            let body = serde_json::json!({"choices":[{"message":{"content":action.to_string()}}]})
                .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("проект с пробелами");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        repo.join("source.py"),
        "answer = 0\r\n# user dirty change\r\n",
    )
    .unwrap();
    std::fs::write(repo.join(".env"), "PRIVATE_SENTINEL").unwrap();
    let task = dir.path().join("TASK.txt");
    std::fs::write(&task, "Return 42").unwrap();
    let db = dir.path().join("sessions.sqlite");
    let config = dir.path().join("providers.json");
    std::fs::write(&config,serde_json::json!({"descriptor":{"provider":"local","model":"test","capabilities":{"tool_calls":false,"vision":false,"prompt_cache":false,"max_context_tokens":4096}},"endpoint":endpoint,"api_key_env":null}).to_string()).unwrap();
    let task_command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lyra"));
        command
            .arg("task")
            .arg("--repo")
            .arg(&repo)
            .arg("--task-file")
            .arg(&task)
            .arg("--provider-config")
            .arg(&config)
            .args(["--allow", "local"])
            .arg("--session-db")
            .arg(&db);
        command
    };
    let first = task_command().args(["--max-calls", "1"]).output().unwrap();
    assert!(!first.status.success());
    let first_report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first_report["status"], "step_limit: coder");
    let stderr = String::from_utf8_lossy(&first.stderr);
    let id = stderr
        .lines()
        .find_map(|line| line.strip_prefix("Lyra session: "))
        .unwrap()
        .to_owned();
    let resumed = task_command()
        .args(["--resume", &id, "--max-calls", "4"])
        .output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    server.join().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(
        report["workspace"],
        serde_json::to_value(repo.canonicalize().unwrap()).unwrap()
    );
    assert_eq!(report["calls"], 3);
    assert_eq!(report["outcome"], "changes_proposed");
    assert!(
        report["proposals"][0]["replacement"]
            .as_str()
            .unwrap()
            .contains("42\r\n# user dirty change")
    );
    assert!(
        std::fs::read_to_string(repo.join("source.py"))
            .unwrap()
            .contains("answer = 0")
    );
    let saved = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(["session", "show", "--db"])
        .arg(&db)
        .args(["--id", &id, "--report"])
        .output()
        .unwrap();
    assert!(saved.status.success());
    let saved_report: serde_json::Value = serde_json::from_slice(&saved.stdout).unwrap();
    assert_eq!(saved_report, report);
}
