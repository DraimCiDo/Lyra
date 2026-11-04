//! Explicit, offline Docker checks. No command is executed on the host project.
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequest {
    pub image: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub timeout_secs: u64,
    pub memory_mib: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub stdout: String,
    pub stderr: String,
    pub output_truncated: bool,
    pub elapsed_ms: u64,
    pub cleanup_confirmed: bool,
}
impl VerificationResult {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && !self.cancelled && self.cleanup_confirmed
    }
}

impl ExecutionRequest {
    pub fn validate(&self) -> io::Result<()> {
        let pinned = self
            .image
            .rsplit_once("@sha256:")
            .is_some_and(|(name, digest)| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-._/:".contains(&c))
                    && name
                        .bytes()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
                    && digest.len() == 64
                    && digest.bytes().all(|c| c.is_ascii_hexdigit())
            });
        if !pinned
            || self.program.is_empty()
            || self.program.starts_with('-')
            || self.program.len() > 1024
            || self.program.chars().any(char::is_control)
            || self.args.len() > 128
            || self.args.iter().map(String::len).sum::<usize>() > 32768
            || self.args.iter().any(|arg| arg.contains('\0'))
            || !(1..=3600).contains(&self.timeout_secs)
            || !(128..=16384).contains(&self.memory_mib)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Check requires a digest-pinned image, bounded argv, memory and timeout",
            ));
        }
        Ok(())
    }

    fn command(&self, source: &Path, name: &str) -> io::Result<Command> {
        self.validate()?;
        let path = source
            .to_str()
            .filter(|path| !path.contains([',', '\n', '\r']))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unsupported Docker source path",
                )
            })?;
        let mut command = Command::new("docker");
        command.args([
            "run",
            "--pull=never",
            "--log-driver=none",
            "--restart=no",
            "--label=io.lyra.check=1",
            "--name",
            name,
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--pids-limit=128",
            "--cpus=2",
            "--memory",
            &format!("{}m", self.memory_mib),
            "--memory-swap",
            &format!("{}m", self.memory_mib),
            "--user=1000:1000",
            "--workdir=/source",
            "--tmpfs=/tmp:rw,nosuid,nodev,size=512m,mode=1777",
            "--mount",
            &format!("type=bind,src={path},dst=/source,readonly"),
            "--env=HOME=/tmp",
            "--env=CARGO_TARGET_DIR=/tmp/target",
            "--env=PYTHONDONTWRITEBYTECODE=1",
            "--env=HTTP_PROXY=",
            "--env=HTTPS_PROXY=",
            "--env=ALL_PROXY=",
            "--env=NO_PROXY=",
            "--env=http_proxy=",
            "--env=https_proxy=",
            "--env=all_proxy=",
            "--env=no_proxy=",
            "--entrypoint",
            "/usr/bin/timeout",
            &self.image,
        ]);
        command.args([
            "--signal=KILL",
            &format!("{}s", self.timeout_secs),
            &self.program,
        ]);
        command.args(&self.args);
        Ok(command)
    }
}

fn bounded_read(mut pipe: impl Read, limit: usize) -> io::Result<(String, bool)> {
    let mut kept = Vec::new();
    let mut chunk = [0; 8192];
    let mut truncated = false;
    loop {
        let count = pipe.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let take = count.min(limit.saturating_sub(kept.len()));
        kept.extend_from_slice(&chunk[..take]);
        truncated |= take < count;
    }
    Ok((String::from_utf8_lossy(&kept).into_owned(), truncated))
}

fn capture(
    mut command: Command,
    timeout: Duration,
    cancel: &AtomicBool,
) -> io::Result<VerificationResult> {
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing check stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Missing check stderr"))?;
    let out = match thread::Builder::new()
        .name("lyra-check-stdout".into())
        .spawn(move || bounded_read(stdout, OUTPUT_LIMIT))
    {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let err = match thread::Builder::new()
        .name("lyra-check-stderr".into())
        .spawn(move || bounded_read(stderr, OUTPUT_LIMIT))
    {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            return Err(error);
        }
    };
    let mut cancelled = false;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
            Ok(None) => {}
        }
        cancelled = cancel.load(Ordering::Relaxed);
        timed_out = started.elapsed() >= timeout;
        if cancelled || timed_out {
            let _ = child.kill();
            break child.wait();
        }
        thread::sleep(Duration::from_millis(25));
    };
    let (stdout, out_cut) = out
        .join()
        .map_err(|_| io::Error::other("Check stdout reader failed"))??;
    let (stderr, err_cut) = err
        .join()
        .map_err(|_| io::Error::other("Check stderr reader failed"))??;
    Ok(VerificationResult {
        exit_code: status?.code(),
        timed_out,
        cancelled,
        stdout,
        stderr,
        output_truncated: out_cut || err_cut,
        elapsed_ms: started.elapsed().as_millis() as u64,
        cleanup_confirmed: false,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerCapabilities {
    #[serde(rename = "OSType")]
    os: String,
    #[serde(default)]
    memory_limit: bool,
    #[serde(default)]
    swap_limit: bool,
    #[serde(default)]
    pids_limit: bool,
    #[serde(default)]
    cpu_cfs_quota: bool,
    #[serde(default)]
    cpu_cfs_period: bool,
    #[serde(default)]
    security_options: Vec<String>,
}
impl DockerCapabilities {
    fn supported(&self) -> bool {
        self.os == "linux"
            && self.memory_limit
            && self.swap_limit
            && self.pids_limit
            && self.cpu_cfs_quota
            && self.cpu_cfs_period
            && self
                .security_options
                .iter()
                .any(|option| option.starts_with("name=seccomp"))
    }
}

pub fn preflight(cancel: &AtomicBool) -> io::Result<()> {
    let mut command = Command::new("docker");
    command.args(["info", "--format", "{{json .}}"]);
    let result = capture(command, Duration::from_secs(5), cancel)?;
    if result.cancelled {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Check cancelled",
        ));
    }
    let supported = result.exit_code == Some(0)
        && !result.timed_out
        && !result.output_truncated
        && serde_json::from_str::<DockerCapabilities>(&result.stdout)
            .is_ok_and(|caps| caps.supported());
    if !supported {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Docker requires a Linux daemon with enforced memory, PID, CPU limits and default seccomp; execution blocked",
        ));
    }
    Ok(())
}

/// Copies only permitted regular UTF-8 files; no host sockets, links or credentials
/// are mounted. Source is read-only; all check output must go to container /tmp.
/// The caller grants this check explicitly; models cannot select an image or argv.
pub fn verify(
    repo: &Path,
    files: &[String],
    request: &ExecutionRequest,
    cancel: &AtomicBool,
) -> io::Result<VerificationResult> {
    verify_changes(repo, files, &[], request, cancel)
}

pub fn verify_changes(
    repo: &Path,
    files: &[String],
    proposals: &[crate::workspace::Edit],
    request: &ExecutionRequest,
    cancel: &AtomicBool,
) -> io::Result<VerificationResult> {
    request.validate()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Check cancelled before start",
        ));
    }
    preflight(cancel)?;
    let mut workspace = crate::workspace::Workspace::open(repo, files)?;
    if workspace.truncated {
        return Err(io::Error::other(
            "Check snapshot is incomplete; select files explicitly",
        ));
    }
    let snapshot = crate::workspace::private_tempdir("lyra-check-")?;
    let source_root = snapshot.path().join("source");
    std::fs::create_dir(&source_root)?;
    for file in workspace.files.clone() {
        let content = workspace.read(&file)?;
        let path = source_root.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }
    if proposals.len() > 8 {
        return Err(io::Error::other("Too many proposals for check"));
    }
    for edit in proposals {
        if edit.create {
            workspace.propose_new(&edit.path, &edit.replacement)?;
        } else if workspace.read(&edit.path)? != edit.original
            || edit.replacement.len() > crate::workspace::FILE_LIMIT
        {
            return Err(io::Error::other("Check proposal conflicts with original"));
        }
        let path = source_root.join(&edit.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &edit.replacement)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut pending = vec![source_root.clone()];
        while let Some(directory) = pending.pop() {
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))?;
            for entry in std::fs::read_dir(&directory)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    pending.push(entry.path());
                } else {
                    std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o644))?;
                }
            }
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let name = format!("lyra-check-{}-{nonce}", std::process::id());
    let command = request.command(&source_root, &name)?;
    let result = capture(command, Duration::from_secs(request.timeout_secs), cancel);
    let mut cleanup = Command::new("docker");
    cleanup.args(["rm", "--force", &name]);
    let cleaned = capture(cleanup, Duration::from_secs(5), &AtomicBool::new(false));
    let mut result = result?;
    if !result.cancelled
        && result.exit_code == Some(137)
        && result.elapsed_ms >= request.timeout_secs * 1000
    {
        result.timed_out = true;
    }
    result.cleanup_confirmed = cleaned
        .as_ref()
        .is_ok_and(|r| r.exit_code == Some(0) && !r.timed_out);
    if !result.cleanup_confirmed {
        result.stderr.push_str(&format!(
            "\nContainer cleanup could not be confirmed for {name}; check is not verified."
        ));
    }
    Ok(result)
}

pub fn load_config(path: &Path) -> io::Result<ExecutionRequest> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(io::Error::other("Check configuration exceeds 64 KiB"));
    }
    let request: ExecutionRequest = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    request.validate()?;
    Ok(request)
}

pub fn run(
    repo: &Path,
    config: &Path,
    files: &[String],
    output: &mut impl io::Write,
) -> io::Result<()> {
    let request = load_config(config)?;
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed)).map_err(io::Error::other)?;
    let result = verify(repo, files, &request, &cancelled)?;
    serde_json::to_writer_pretty(&mut *output, &result).map_err(io::Error::other)?;
    writeln!(output)?;
    if result.passed() {
        Ok(())
    } else if result.cancelled {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Check cancelled",
        ))
    } else if result.timed_out {
        Err(io::Error::new(io::ErrorKind::TimedOut, "Check timed out"))
    } else {
        Err(io::Error::other("Check failed or cleanup is unconfirmed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> ExecutionRequest {
        ExecutionRequest {
            image: format!("python@sha256:{}", "a".repeat(64)),
            program: "python".into(),
            args: vec!["-c".into(), "print('test')".into()],
            timeout_secs: 5,
            memory_mib: 256,
        }
    }
    #[test]
    fn requires_pinned_images_and_enforces_container_boundaries() {
        let mut req = request();
        assert!(req.validate().is_ok());
        let command = req
            .command(Path::new("/tmp/копия проекта"), "lyra-test")
            .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        for required in [
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--pull=never",
            "--log-driver=none",
            "--security-opt=no-new-privileges",
        ] {
            assert!(args.iter().any(|arg| arg == required));
        }
        assert!(
            args.iter()
                .any(|arg| arg == "type=bind,src=/tmp/копия проекта,dst=/source,readonly")
        );
        req.image = "python:latest".into();
        assert!(req.validate().is_err());
        req.image = format!("--privileged@sha256:{}", "a".repeat(64));
        assert!(req.validate().is_err());
        req = request();
        req.args = vec!["bad\0arg".into()];
        assert!(req.validate().is_err());
    }
    #[test]
    fn missing_resource_enforcement_is_not_a_supported_sandbox() {
        let mut caps = DockerCapabilities {
            os: "linux".into(),
            memory_limit: true,
            swap_limit: true,
            pids_limit: true,
            cpu_cfs_quota: true,
            cpu_cfs_period: true,
            security_options: vec!["name=seccomp,profile=builtin".into()],
        };
        assert!(caps.supported());
        caps.memory_limit = false;
        assert!(!caps.supported());
        caps.memory_limit = true;
        caps.pids_limit = false;
        assert!(!caps.supported());
        caps.pids_limit = true;
        caps.security_options.clear();
        assert!(!caps.supported());
    }

    #[test]
    fn bounded_logs_and_verified_evidence() {
        let (out, truncated) = bounded_read(&b"abcdef"[..], 3).unwrap();
        assert_eq!(out, "abc");
        assert!(truncated);
        let mut result = VerificationResult {
            exit_code: Some(0),
            timed_out: false,
            cancelled: false,
            stdout: String::new(),
            stderr: String::new(),
            output_truncated: false,
            elapsed_ms: 0,
            cleanup_confirmed: false,
        };
        assert!(!result.passed());
        result.cleanup_confirmed = true;
        assert!(result.passed());
        result.timed_out = true;
        assert!(!result.passed());
    }
}
