use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn required_arguments_and_numeric_ranges_are_enforced() {
    for args in [
        vec!["generate"],
        vec![
            "generate",
            "--model",
            "unused",
            "--prompt",
            "hi",
            "--context",
            "0",
        ],
        vec![
            "generate",
            "--model",
            "unused",
            "--prompt",
            "hi",
            "--threads",
            "0",
        ],
        vec![
            "generate",
            "--model",
            "unused",
            "--prompt",
            "hi",
            "--timeout-secs",
            "0",
        ],
    ] {
        assert_eq!(run(&args).status.code(), Some(2));
    }
}

#[test]
fn invalid_files_and_budgets_fail_without_generating() {
    for args in [
        vec![
            "generate",
            "--model",
            "missing-lyra-model.gguf",
            "--prompt",
            "hi",
        ],
        vec!["generate", "--model", "unused", "--prompt", " "],
        vec![
            "generate",
            "--model",
            "unused",
            "--prompt",
            "hi",
            "--context",
            "128",
            "--max-tokens",
            "128",
        ],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}

#[test]
fn a_corrupt_header_is_rejected_before_native_loading() {
    let file =
        std::env::temp_dir().join(format!("lyra invalid модель {}.gguf", std::process::id()));
    std::fs::write(&file, b"not-gguf").unwrap();
    let output = run(&[
        "generate",
        "--model",
        file.to_str().unwrap(),
        "--prompt",
        "hello",
    ]);
    std::fs::remove_file(file).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("GGUF версии"));
}

#[test]
#[ignore = "requires explicitly supplied LYRA_TEST_MODEL; ordinary CI downloads no weights"]
fn real_local_model_generates_with_a_token_limit() {
    let path = std::env::var("LYRA_TEST_MODEL").expect("set LYRA_TEST_MODEL to a local test GGUF");
    let output = run(&[
        "generate",
        "--model",
        &path,
        "--prompt",
        "Once upon a time",
        "--raw",
        "--context",
        "128",
        "--max-tokens",
        "4",
        "--threads",
        "2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("лимит ответа 4"));
}
