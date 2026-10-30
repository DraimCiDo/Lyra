//! One owned worker, bounded pipes, no shell and no detached inference on window close.
use std::{
    collections::VecDeque,
    ffi::OsString,
    io::{self, Read},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tempfile::TempDir;

pub const OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const LOG_LIMIT: usize = 64 * 1024;

pub struct ResultData {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub cancelled: bool,
    pub elapsed: Duration,
}

pub struct Job {
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<io::Result<ResultData>>,
    thread: Option<JoinHandle<()>>,
    pub started: Instant,
    pub cancellable: bool,
}

impl Job {
    pub fn start(
        args: Vec<OsString>,
        temp: TempDir,
        cancellable: bool,
        deadline: Option<Duration>,
        repaint: impl FnOnce() + Send + 'static,
    ) -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let mut command = Command::new(executable);
        command.arg("--worker").args(args);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW; inherited redirected handles.
        }
        Self::start_command(command, temp, cancellable, deadline, repaint)
    }

    fn start_command(
        command: Command,
        temp: TempDir,
        cancellable: bool,
        deadline: Option<Duration>,
        repaint: impl FnOnce() + Send + 'static,
    ) -> io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let started = Instant::now();
        let thread = thread::Builder::new()
            .name("lyra-worker".into())
            .spawn(move || {
                let _temp = temp; // Removed only after worker and pipe readers have terminated.
                let result = capture(command, flag, deadline);
                let _ = sender.send(result);
                repaint();
            })?;
        Ok(Self {
            cancel,
            receiver,
            thread: Some(thread),
            started,
            cancellable,
        })
    }

    pub fn cancel(&self) {
        if self.cancellable {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn poll(&self) -> Option<io::Result<ResultData>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("Worker завершился без отчёта.")))
            }
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_output(mut reader: impl Read, limit: usize, tail: bool) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = VecDeque::new();
    let mut chunk = [0; 8192];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if tail {
            bytes.extend(&chunk[..count]);
            if bytes.len() > limit {
                bytes.drain(..bytes.len() - limit);
                truncated = true;
            }
        } else {
            let keep = count.min(limit.saturating_sub(bytes.len()));
            bytes.extend(&chunk[..keep]);
            truncated |= keep < count;
        }
    }
    Ok((bytes.into(), truncated))
}

fn capture(
    mut command: Command,
    cancel: Arc<AtomicBool>,
    deadline: Option<Duration>,
) -> io::Result<ResultData> {
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Нет stdout worker."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Нет stderr worker."))?;
    let out_thread = match thread::Builder::new()
        .name("lyra-stdout".into())
        .spawn(move || read_output(stdout, OUTPUT_LIMIT, false))
    {
        Ok(thread) => thread,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let err_thread = match thread::Builder::new()
        .name("lyra-stderr".into())
        .spawn(move || read_output(stderr, LOG_LIMIT, true))
    {
        Ok(thread) => thread,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_thread.join();
            return Err(error);
        }
    };
    let mut cancelled = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
        }
        if cancel.load(Ordering::Relaxed) || deadline.is_some_and(|d| started.elapsed() > d) {
            cancelled = true;
            let _ = child.kill();
            break child.wait();
        }
        thread::sleep(Duration::from_millis(50));
    };
    let out = out_thread
        .join()
        .map_err(|_| io::Error::other("Сбой чтения stdout."))?;
    let err = err_thread
        .join()
        .map_err(|_| io::Error::other("Сбой чтения stderr."))?;
    let (stdout, overflow) = out?;
    let (stderr, log_cut) = err?;
    if overflow {
        return Err(io::Error::other("Отчёт worker превышает 2 MiB."));
    }
    let stderr = format!(
        "{}{}",
        if log_cut {
            "[начало журнала сокращено]\n"
        } else {
            ""
        },
        String::from_utf8_lossy(&stderr)
    );
    Ok(ResultData {
        code: status?.code(),
        stdout,
        stderr,
        cancelled,
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_pipes_keep_json_prefix_and_log_tail() {
        let data = b"0123456789";
        assert_eq!(
            read_output(&data[..], 4, false).unwrap(),
            (b"0123".to_vec(), true)
        );
        assert_eq!(
            read_output(&data[..], 4, true).unwrap(),
            (b"6789".to_vec(), true)
        );
    }

    fn sleeper() -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "desktop::job::tests::worker_sleeps",
            "--ignored",
            "--nocapture",
        ]);
        command
    }

    #[test]
    #[ignore = "subprocess fixture; invoked explicitly by cancellation tests"]
    fn worker_sleeps() {
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn deadline_reaps_worker() {
        let now = Instant::now();
        let result = capture(
            sleeper(),
            Arc::new(AtomicBool::new(false)),
            Some(Duration::from_millis(100)),
        )
        .unwrap();
        assert!(now.elapsed() < Duration::from_secs(5));
        assert!(result.cancelled);
        assert!(result.stdout.len() < OUTPUT_LIMIT);
    }

    #[test]
    fn explicit_cancel_reaps_worker() {
        let flag = Arc::new(AtomicBool::new(false));
        let other = flag.clone();
        let handle = thread::spawn(move || capture(sleeper(), other, None).unwrap());
        thread::sleep(Duration::from_millis(100));
        flag.store(true, Ordering::Relaxed);
        assert!(handle.join().unwrap().cancelled);
    }
    #[test]
    fn closing_job_reaps_worker_and_cleans_private_temp() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();
        let job = Job::start_command(sleeper(), temp, true, None, || {}).unwrap();
        drop(job);
        assert!(!path.exists());
    }

    #[test]
    #[ignore = "subprocess fixture; invoked explicitly by output-limit test"]
    fn worker_floods_output() {
        use std::io::Write;
        io::stdout()
            .lock()
            .write_all(&vec![b'x'; OUTPUT_LIMIT + 8192])
            .unwrap();
    }

    #[test]
    fn oversized_worker_report_is_rejected_without_pipe_deadlock() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "desktop::job::tests::worker_floods_output",
            "--ignored",
            "--nocapture",
        ]);
        let result = capture(
            command,
            Arc::new(AtomicBool::new(false)),
            Some(Duration::from_secs(10)),
        );
        assert!(result.err().unwrap().to_string().contains("2 MiB"));
    }
}
