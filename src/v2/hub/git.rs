//! Bounded `git` invocations for the agent hub. Arguments are passed separately; no shell.
use std::{
    io::{self, Read},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: u64 = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);

pub(super) struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

fn drain(stream: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(stream) = stream {
            let mut limited = stream.take(OUTPUT_LIMIT);
            let _ = limited.read_to_end(&mut text);
            // Keep reading so the child never blocks on a full pipe.
            let _ = io::copy(&mut limited.into_inner(), &mut io::sink());
        }
        String::from_utf8_lossy(&text).into_owned()
    })
}

pub(super) fn run(dir: &Path, args: &[&str]) -> io::Result<Output> {
    let mut child = Command::new("git")
        .arg("-c")
        .arg("core.quotepath=false")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("The agent hub needs git on PATH: {error}"),
            )
        })?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "git timed out"));
        }
        thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Run git and return trimmed stdout, or an error with git's message.
pub(super) fn ok(dir: &Path, args: &[&str]) -> io::Result<String> {
    let output = run(dir, args)?;
    if output.success {
        Ok(output.stdout.trim_end().to_owned())
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

/// Tracked changes and untracked files that are not ignored, except Lyra's own
/// publication lock and journal folder.
pub(super) fn dirty(dir: &Path) -> io::Result<bool> {
    Ok(!ok(
        dir,
        &["status", "--porcelain", "--", ".", ":(exclude).lyra"],
    )?
    .is_empty())
}
