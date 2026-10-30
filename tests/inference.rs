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
#[ignore = "requires explicitly supplied LYRA_TEST_MODEL; tests never auto-download weights"]
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

#[test]
#[ignore = "requires explicitly supplied LYRA_TEST_MODEL"]
fn one_loaded_model_serves_independent_contexts_and_json_grammar() {
    use lyra::inference::{GenerateArgs, with_session};
    let path = std::env::var("LYRA_TEST_MODEL").expect("set LYRA_TEST_MODEL");
    let args = GenerateArgs {
        model: path.into(),
        prompt: "Once upon a time".into(),
        context: 128,
        max_tokens: 16,
        threads: 2,
        gpu_layers: 0,
        timeout_secs: 120,
        raw: true,
    };
    with_session(&args, |session| {
        let mut first = Vec::new();
        let mut second = Vec::new();
        session.complete(&args.prompt, None, &mut first)?;
        session.complete(&args.prompt, None, &mut second)?;
        assert_eq!(first, second);
        assert!(!first.is_empty());
        let mut constrained = Vec::new();
        session.complete("hi", Some("root ::= \"ok\""), &mut constrained)?;
        assert_eq!(String::from_utf8(constrained).unwrap().trim(), "ok");
        let mut system_output = Vec::new();
        session.complete_with_system(
            "hi",
            Some("system instruction"),
            Some("root ::= \"ok\""),
            &mut system_output,
        )?;
        assert_eq!(String::from_utf8(system_output).unwrap().trim(), "ok");
        let mut action = Vec::new();
        session.complete("hi", Some(lyra::agent::ACTION_GRAMMAR), &mut action)?;
        assert!(String::from_utf8(action).unwrap().starts_with('{'));
        Ok(())
    })
    .unwrap();
}
