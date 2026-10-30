use std::process::{Command, Output};

fn lyra(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(args)
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env_remove("OPENAI_API_KEY")
        .output()
        .expect("CLI should start on the current platform")
}

#[test]
fn help_and_version_do_not_require_a_model_or_api_key() {
    let help = lyra(&["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("doctor"));
    assert!(help.contains("ещё не реализованы"));
    let version = lyra(&["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        concat!("lyra ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn malformed_arguments_fail_without_a_panic() {
    for args in [
        &["unknown"][..],
        &["doctor", "--unknown"],
        &["doctor", "unexpected"],
        &[],
    ] {
        let output = lyra(args);
        assert_eq!(output.status.code(), Some(2), "arguments: {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}

#[test]
fn doctor_json_is_an_offline_snapshot_not_a_readiness_claim() {
    let output = lyra(&["doctor", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["architecture"], std::env::consts::ARCH);
    assert_eq!(
        report["inference_status"],
        if cfg!(feature = "cuda") {
            "runtime_available_cuda"
        } else {
            "runtime_available_cpu"
        }
    );
    assert!(report["warnings"].is_array());
    assert!(report.get("cpu").is_some());
    assert!(report.get("memory").is_some());
    assert!(report.get("host_name").is_none());
    assert!(report.get("processes").is_none());
    assert!(report.get("environment").is_none());
}

#[test]
fn doctor_text_completes_and_identifies_the_current_machine() {
    let output = lyra(&["doctor"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("локальная диагностика"));
    assert!(text.contains("характеристики текущей среды запуска"));
}

#[test]
fn diagnostics_do_not_change_files_in_a_unicode_working_directory() {
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "lyra тест с пробелами-{}-{nonce}",
        std::process::id()
    )));
    std::fs::create_dir(&scratch.0).unwrap();
    let existing = scratch.0.join("существующий файл.txt");
    let contents = b"user edits\r\nkeep CRLF\r\n";
    std::fs::write(&existing, contents).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(["doctor", "--json"])
        .current_dir(&scratch.0)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(std::fs::read(&existing).unwrap(), contents);
    assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn full_output_device_is_a_failure_not_a_panic() {
    use std::fs::OpenOptions;
    for args in [&["--help"][..], &["doctor"], &["doctor", "--json"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
            .args(args)
            .stdout(OpenOptions::new().write(true).open("/dev/full").unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "arguments: {args:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}
