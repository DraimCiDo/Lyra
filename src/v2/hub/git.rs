//! Bounded `git` invocations for the agent hub. Arguments are passed separately; no shell.
use std::{
    io::{self, Read},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: u64 = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub(super) struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    /// Stdout was cut at the output limit.
    pub truncated: bool,
}

/// Read one pipe up to the output limit, then keep draining so the child never blocks.
fn drain(stream: Option<impl Read + Send + 'static>) -> mpsc::Receiver<(String, bool)> {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(stream) = stream {
            let mut limited = stream.take(OUTPUT_LIMIT);
            let _ = limited.read_to_end(&mut text);
            let mut rest = limited.into_inner();
            // Anything left after the limit means the output was cut.
            let mut probe = [0u8; 1];
            let cut = rest.read(&mut probe).unwrap_or(0) > 0;
            let _ = send.send((String::from_utf8_lossy(&text).into_owned(), cut));
            let _ = io::copy(&mut rest, &mut io::sink());
        } else {
            let _ = send.send((String::new(), false));
        }
    });
    receive
}

/// Terminate git and every process it started (hooks, credential helpers, filters).
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        // The child leads its own process group (see `run`).
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = crate::executor::quiet(&mut Command::new("taskkill"))
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn run(dir: &Path, args: &[&str]) -> io::Result<Output> {
    run_with_timeout(dir, args, TIMEOUT)
}

/// One deadline covers the process and the draining of its output pipes.
fn run_with_timeout(dir: &Path, args: &[&str], timeout: Duration) -> io::Result<Output> {
    let mut command = Command::new("git");
    crate::executor::quiet(&mut command)
        .arg("-c")
        .arg("core.quotepath=false")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("The agent hub needs git on PATH: {error}"),
        )
    })?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let timed_out = || io::Error::new(io::ErrorKind::TimedOut, "git timed out");
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            return Err(timed_out());
        }
        thread::sleep(Duration::from_millis(20));
    };
    // A descendant may keep a pipe open after git exits; do not wait for it past the deadline.
    let collect = |receiver: &mpsc::Receiver<(String, bool)>| {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| ())
    };
    match (collect(&stdout), collect(&stderr)) {
        (Ok((stdout, truncated)), Ok((stderr, _))) => Ok(Output {
            success: status.success(),
            stdout,
            stderr,
            truncated,
        }),
        _ => {
            kill_tree(&mut child);
            Err(timed_out())
        }
    }
}

/// Run git and return trimmed stdout, or an error with git's message. Output that hit the
/// limit is an error: callers that decide from complete output must never see a prefix.
pub(super) fn ok(dir: &Path, args: &[&str]) -> io::Result<String> {
    let output = checked(dir, args)?;
    if output.truncated {
        return Err(io::Error::other(format!(
            "git {} produced more than 1 MiB of output",
            args.first().copied().unwrap_or_default()
        )));
    }
    Ok(output.stdout.trim_end().to_owned())
}

/// Like `ok`, but a cut-off prefix is acceptable (for patches shown to a person).
pub(super) fn ok_prefix(dir: &Path, args: &[&str]) -> io::Result<String> {
    Ok(checked(dir, args)?.stdout.trim_end().to_owned())
}

fn checked(dir: &Path, args: &[&str]) -> io::Result<Output> {
    let output = run(dir, args)?;
    if output.success {
        Ok(output)
    } else {
        let message = output.stderr.trim();
        Err(io::Error::other(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            &message[..message.floor_char_boundary(1500)]
        )))
    }
}

pub(super) fn head(dir: &Path) -> io::Result<String> {
    ok(dir, &["rev-parse", "HEAD"])
}

/// Lines of `ls-files -v` whose tag is not the plain `H` (cached): assume-unchanged,
/// skip-worktree and similar. The listing of a large repository exceeds the output limit,
/// so it is filtered while it streams instead of being collected.
fn hidden_index_entries(dir: &Path) -> io::Result<bool> {
    let mut command = Command::new("git");
    crate::executor::quiet(&mut command)
        .args(["-c", "core.quotepath=false", "ls-files", "-v"])
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("The agent hub needs git on PATH: {error}"),
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing git output"))?;
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = io::BufReader::new(stdout);
        let mut line = Vec::new();
        let mut hidden = false;
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => hidden |= line.first().is_some_and(|tag| *tag != b'H'),
            }
        }
        let _ = send.send(hidden);
    });
    let deadline = Instant::now() + TIMEOUT;
    let hidden = receive.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    let status = match hidden {
        Ok(_) => child.wait()?,
        Err(_) => {
            kill_tree(&mut child);
            return Err(io::Error::new(io::ErrorKind::TimedOut, "git timed out"));
        }
    };
    if !status.success() {
        return Err(io::Error::other("git ls-files failed"));
    }
    Ok(hidden.unwrap_or(true))
}

/// Whether the worktree differs from its commit: tracked changes and untracked files that
/// are not ignored, except Lyra's own lock folder. Files hidden from `git status`
/// (`assume-unchanged`, `skip-worktree`) count as dirty because their bytes can differ
/// from the commit without being reported.
pub(super) fn dirty(dir: &Path) -> io::Result<bool> {
    let status = checked(
        dir,
        &[
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--",
            ".",
            ":(exclude).lyra",
        ],
    )?;
    // Any output at all means a change, however long the listing is.
    if !status.stdout.trim().is_empty() || status.truncated {
        return Ok(true);
    }
    // Tags: `H` cached, `h` assume-unchanged, `S` skip-worktree, `s` both.
    hidden_index_entries(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "T"],
            &["config", "user.email", "t@example.invalid"],
            &["config", "commit.gpgsign", "false"],
        ] {
            ok(dir.path(), args).unwrap();
        }
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        ok(dir.path(), &["add", "-A"]).unwrap();
        ok(dir.path(), &["commit", "-q", "-m", "init"]).unwrap();
        dir
    }

    #[test]
    fn hidden_changes_and_untracked_files_make_a_worktree_dirty() {
        let repo = repo();
        assert!(!dirty(repo.path()).unwrap());
        // A repository setting must not hide new files.
        ok(repo.path(), &["config", "status.showUntrackedFiles", "no"]).unwrap();
        std::fs::write(repo.path().join("new.txt"), "x").unwrap();
        assert!(dirty(repo.path()).unwrap());
        std::fs::remove_file(repo.path().join("new.txt")).unwrap();
        assert!(!dirty(repo.path()).unwrap());
        // Lyra's own lock folder is not a change.
        std::fs::create_dir(repo.path().join(".lyra")).unwrap();
        std::fs::write(repo.path().join(".lyra/apply.lock"), "").unwrap();
        assert!(!dirty(repo.path()).unwrap());
        // Modified bytes hidden with assume-unchanged are still reported.
        ok(
            repo.path(),
            &["update-index", "--assume-unchanged", "a.txt"],
        )
        .unwrap();
        std::fs::write(repo.path().join("a.txt"), "two\n").unwrap();
        assert!(dirty(repo.path()).unwrap());
    }

    #[test]
    fn big_outputs_are_errors_for_decisions_and_do_not_break_the_cleanliness_test() {
        let repo = repo();
        // More than 1 MiB of listing output: 30k tracked files with long names.
        let folder = repo.path().join("d");
        std::fs::create_dir(&folder).unwrap();
        for index in 0..20_000 {
            std::fs::write(folder.join(format!("{index:05}-{}", "n".repeat(60))), "").unwrap();
        }
        ok(repo.path(), &["add", "-A"]).unwrap();
        let error = ok(repo.path(), &["ls-files", "-v"]).unwrap_err();
        assert!(error.to_string().contains("more than 1 MiB"), "{error}");
        assert!(ok_prefix(repo.path(), &["ls-files", "-v"]).unwrap().len() <= 1 << 20);
        // Large listings do not break the cleanliness test, and a hidden entry that sorts
        // after the output limit is still found.
        // Staged but uncommitted: dirty, even though the status listing is huge.
        assert!(dirty(repo.path()).unwrap());
        ok(repo.path(), &["commit", "-q", "-m", "many files"]).unwrap();
        assert!(!dirty(repo.path()).unwrap());
        std::fs::write(repo.path().join("zzz-last.txt"), "x").unwrap();
        ok(repo.path(), &["add", "zzz-last.txt"]).unwrap();
        ok(repo.path(), &["commit", "-q", "-m", "last"]).unwrap();
        assert!(!dirty(repo.path()).unwrap());
        ok(
            repo.path(),
            &["update-index", "--assume-unchanged", "zzz-last.txt"],
        )
        .unwrap();
        assert!(dirty(repo.path()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_holding_the_pipes_cannot_outlive_the_deadline() {
        let repo = repo();
        // A hook-like helper that detaches and keeps stdout open after git exits.
        let hook = repo.path().join(".git/hooks/post-checkout");
        std::fs::write(&hook, "#!/bin/sh\nsleep 30 &\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        let result = run_with_timeout(
            repo.path(),
            &["checkout", "-q", "-b", "other"],
            Duration::from_secs(2),
        );
        assert!(started.elapsed() < Duration::from_secs(10), "{result:?}");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }
}
