#![cfg(feature = "gateway-http")]

use std::process::Command;

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
