//! Verification of pending proposals in a scratch copy of the project.
//!
//! The model never chooses commands: they come from the user (`--check`), are split into
//! separate arguments (no shell), and the program must be on a fixed list of build/test
//! tools. Checks run in a temporary copy with the proposals applied, never in the user's
//! tree. This is not an OS sandbox: the commands run with the user's privileges.
use crate::workspace::{Edit, Workspace};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const ALLOWED_PROGRAMS: &[&str] = &[
    "cargo", "npm", "pnpm", "yarn", "pytest", "python", "python3", "go", "dotnet", "node", "make",
    "gradle", "mvn", "tsc",
];
const MAX_COPY_FILES: usize = 20_000;
const MAX_COPY_BYTES: u64 = 256 * 1024 * 1024;
const OUTPUT_HEAD: usize = 3 * 1024;
const OUTPUT_TAIL: usize = 5 * 1024;
const SKIPPED_DIRS: &[&str] = &[".git", "target", "node_modules", ".lyra", "dist", ".venv"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub output: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRun {
    pub round: u32,
    pub passed: bool,
    pub results: Vec<CheckResult>,
}

/// Split a user-supplied command line into argv. Double quotes group words; no expansion.
pub fn parse_command(line: &str) -> io::Result<Vec<String>> {
    let mut argv = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    argv.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\0' => return Err(invalid("NUL in check command")),
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err(invalid("Unterminated quote in check command"));
    }
    if started {
        argv.push(current);
    }
    validate_argv(&argv)?;
    Ok(argv)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub fn validate_argv(argv: &[String]) -> io::Result<()> {
    let program = argv.first().ok_or_else(|| invalid("Empty check command"))?;
    if argv.len() > 32 || argv.iter().any(|a| a.len() > 512) {
        return Err(invalid("Check command is too long"));
    }
    let name = program.to_ascii_lowercase();
    if !ALLOWED_PROGRAMS.contains(&name.as_str()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Check program '{program}' is not allowed (allowed: {})",
                ALLOWED_PROGRAMS.join(", ")
            ),
        ));
    }
    Ok(())
}

/// Copy the project (without VCS/build/dependency folders and symlinks) into `dest`.
pub fn copy_project(src: &Path, dest: &Path) -> io::Result<usize> {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let walker = ignore::WalkBuilder::new(src)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| {
            !(entry.depth() > 0
                && entry.file_type().is_some_and(|t| t.is_dir())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| SKIPPED_DIRS.contains(&n)))
        })
        .build();
    for entry in walker {
        let entry = entry.map_err(io::Error::other)?;
        let Ok(relative) = entry.path().strip_prefix(src) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let Some(kind) = entry.file_type() else {
            continue;
        };
        let target = dest.join(relative);
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if kind.is_file() {
            files += 1;
            bytes += entry.metadata().map_err(io::Error::other)?.len();
            if files > MAX_COPY_FILES || bytes > MAX_COPY_BYTES {
                return Err(io::Error::other(
                    "Project is too large to verify in a scratch copy",
                ));
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(files)
}

/// Apply proposals to a scratch copy, checking the original content of every file.
pub fn apply_to_copy(scratch: &Path, proposals: &[Edit]) -> io::Result<()> {
    for edit in proposals {
        let selected: &[String] = if edit.create {
            &[]
        } else {
            std::slice::from_ref(&edit.path)
        };
        Workspace::open(scratch, selected)?.apply(edit)?;
    }
    Ok(())
}

/// Run every command in a scratch copy of `repo` that contains `proposals`.
/// Stops after the first failing command.
pub fn run_checks(
    repo: &Path,
    proposals: &[Edit],
    commands: &[Vec<String>],
    timeout: Duration,
    round: u32,
) -> io::Result<CheckRun> {
    let scratch = tempfile::Builder::new().prefix("lyra-check-").tempdir()?;
    copy_project(repo, scratch.path())?;
    apply_to_copy(scratch.path(), proposals)?;
    let mut results = Vec::new();
    for argv in commands {
        validate_argv(argv)?;
        let result = run_one(scratch.path(), argv, timeout)?;
        let passed = result.passed;
        results.push(result);
        if !passed {
            break;
        }
    }
    let passed = !results.is_empty() && results.iter().all(|r| r.passed);
    Ok(CheckRun {
        round,
        passed,
        results,
    })
}

fn run_one(dir: &Path, argv: &[String], timeout: Duration) -> io::Result<CheckResult> {
    let started = Instant::now();
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let line = argv.join(" ");
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Ok(CheckResult {
                command: line,
                passed: false,
                exit_code: None,
                timed_out: false,
                duration_ms: 0,
                output: format!("Failed to start: {e}"),
            });
        }
    };
    let pid = child.id();
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|stream| std::thread::spawn(move || bounded_capture(stream)))
    .collect();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            kill_tree(&mut child, pid);
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let mut output = String::new();
    for reader in readers {
        if let Ok(text) = reader.join() {
            output.push_str(&text);
        }
    }
    let exit_code = status.and_then(|s| s.code());
    Ok(CheckResult {
        command: line,
        passed: status.is_some_and(|s| s.success()),
        exit_code,
        timed_out,
        duration_ms: started.elapsed().as_millis() as u64,
        output,
    })
}

fn kill_tree(child: &mut std::process::Child, pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = pid;
    let _ = child.kill();
}

/// Drain a stream, keeping only the first and last bytes so memory stays bounded.
fn bounded_capture(mut stream: Box<dyn Read + Send>) -> String {
    let mut head = Vec::new();
    let mut tail = std::collections::VecDeque::new();
    let mut dropped = 0usize;
    let mut buffer = [0u8; 4096];
    while let Ok(count) = stream.read(&mut buffer) {
        if count == 0 {
            break;
        }
        for &byte in &buffer[..count] {
            if head.len() < OUTPUT_HEAD {
                head.push(byte);
            } else {
                if tail.len() == OUTPUT_TAIL {
                    tail.pop_front();
                    dropped += 1;
                }
                tail.push_back(byte);
            }
        }
    }
    let mut text = String::from_utf8_lossy(&head).into_owned();
    if dropped > 0 {
        text.push_str(&format!("\n… [{dropped} bytes omitted] …\n"));
    }
    let tail: Vec<u8> = tail.into_iter().collect();
    text.push_str(&String::from_utf8_lossy(&tail));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_rejects_unlisted_programs_and_shell_text_is_not_interpreted() {
        assert_eq!(
            parse_command("cargo test --lib \"my test\"").unwrap(),
            ["cargo", "test", "--lib", "my test"]
        );
        assert!(parse_command("rm -rf /").is_err());
        assert!(parse_command("").is_err());
        assert!(parse_command("cargo \"unterminated").is_err());
        // `;` stays inside an argument; there is no shell to interpret it.
        assert_eq!(
            parse_command("cargo test ; rm").unwrap(),
            ["cargo", "test", ";", "rm"]
        );
    }

    #[test]
    fn proposals_run_only_in_scratch_copy_and_failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\n").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/skip.txt"), "x").unwrap();
        let edit = Edit {
            path: "a.txt".into(),
            original: "old\n".into(),
            replacement: "new\n".into(),
            create: false,
        };
        let cmd = vec![vec!["python3".to_owned(), "-c".into(), "import sys;sys.exit(0 if open('a.txt').read()=='new\\n' and not __import__('os').path.exists('target') else 3)".into()]];
        let run = run_checks(
            dir.path(),
            std::slice::from_ref(&edit),
            &cmd,
            Duration::from_secs(20),
            1,
        )
        .unwrap();
        assert!(run.passed, "{:?}", run.results);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "old\n"
        );
        let failing = vec![vec![
            "python3".to_owned(),
            "-c".into(),
            "print('boom');raise SystemExit(5)".into(),
        ]];
        let run = run_checks(dir.path(), &[edit], &failing, Duration::from_secs(20), 1).unwrap();
        assert!(!run.passed);
        assert_eq!(run.results[0].exit_code, Some(5));
        assert!(run.results[0].output.contains("boom"));
    }

    #[test]
    fn stale_proposal_and_timeout_are_reported_without_hanging() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "current\n").unwrap();
        let stale = Edit {
            path: "a.txt".into(),
            original: "other\n".into(),
            replacement: "new\n".into(),
            create: false,
        };
        assert!(run_checks(dir.path(), &[stale], &[], Duration::from_secs(5), 1).is_err());
        let slow = vec![vec![
            "python3".to_owned(),
            "-c".into(),
            "import time;time.sleep(60)".into(),
        ]];
        let started = Instant::now();
        let run = run_checks(dir.path(), &[], &slow, Duration::from_millis(400), 1).unwrap();
        assert!(run.results[0].timed_out && !run.passed);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
