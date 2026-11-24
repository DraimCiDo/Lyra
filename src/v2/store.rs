use super::{TaskReport, TaskState};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const STATE_FILE: &str = "task.json";
const REPORT_LIMIT: usize = 4 * 1024 * 1024;

pub struct TaskStore {
    folder: PathBuf,
    _lock: File,
}

impl TaskStore {
    pub fn create(folder: &Path, report: &TaskReport) -> io::Result<Self> {
        let parent = folder
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        if parent.starts_with(&report.request.repo) {
            return Err(io::Error::other(
                "Choose a v2 state directory outside the project",
            ));
        }
        let name = folder
            .file_name()
            .ok_or_else(|| io::Error::other("Invalid state directory"))?;
        let folder = parent.join(name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&folder)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&folder)?;
        // Remove only what this attempt created, so the same path can be retried.
        let discard = |error: io::Error| {
            let _ = fs::remove_file(folder.join("task.lock"));
            let _ = fs::remove_dir(&folder);
            error
        };
        let store = Self::lock(&folder).map_err(discard)?;
        if let Err(error) = store.save(report) {
            drop(store);
            return Err(discard(error));
        }
        Ok(store)
    }

    pub fn open(folder: &Path) -> io::Result<Self> {
        if !fs::symlink_metadata(folder.join(STATE_FILE))?.is_file() {
            return Err(io::Error::other("An ordinary v2 task report is required"));
        }
        Self::lock(folder)
    }

    fn lock(folder: &Path) -> io::Result<Self> {
        if !fs::symlink_metadata(folder)?.is_dir() {
            return Err(io::Error::other("A regular v2 state directory is required"));
        }
        let folder = folder.canonicalize()?;
        let lock_path = folder.join("task.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = match options.open(&lock_path) {
            Ok(lock) => lock,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&lock_path)?.is_file() {
                    return Err(io::Error::other("Invalid v2 task lock"));
                }
                OpenOptions::new().read(true).write(true).open(&lock_path)?
            }
            Err(error) => return Err(error),
        };
        // A child process forked elsewhere in this program can share the lock's open file
        // description until its exec closes it, so allow a short retry before refusing.
        let mut attempts = 0;
        while let Err(error) = lock.try_lock() {
            attempts += 1;
            if attempts == 40 {
                return Err(io::Error::other(format!("Task is already in use: {error}")));
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        Ok(Self {
            folder,
            _lock: lock,
        })
    }

    pub fn load(&self) -> io::Result<TaskReport> {
        let path = self.folder.join(STATE_FILE);
        if !fs::symlink_metadata(&path)?.is_file() {
            return Err(io::Error::other("Invalid v2 task report"));
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take((REPORT_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > REPORT_LIMIT {
            return Err(io::Error::other("Task report exceeds 4 MiB"));
        }
        let report: TaskReport = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if report.schema_version != 1 || self.folder.starts_with(&report.request.repo) {
            return Err(io::Error::other(
                "Unsupported v2 task report or overlapping workspace",
            ));
        }
        Ok(report)
    }

    pub fn save(&self, report: &TaskReport) -> io::Result<()> {
        let bytes = serde_json::to_vec(report).map_err(io::Error::other)?;
        if bytes.len() > REPORT_LIMIT {
            return Err(io::Error::other("Task report exceeds 4 MiB"));
        }
        let mut temp = tempfile::NamedTempFile::new_in(&self.folder)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(self.folder.join(STATE_FILE))
            .map_err(|error| error.error)?;
        #[cfg(unix)]
        File::open(&self.folder)?.sync_all()?;
        Ok(())
    }

    pub fn apply(&self) -> io::Result<TaskReport> {
        let mut report = self.load()?;
        if report.state == TaskState::Applied {
            return Ok(report);
        }
        if !matches!(report.state, TaskState::ReadyToApply | TaskState::Applying)
            || report.proposals.is_empty()
        {
            return Err(io::Error::other("This task has no proposal ready to apply"));
        }
        let repo = &report.request.repo;
        if repo.canonicalize()? != *repo {
            return Err(io::Error::other("The task workspace has moved"));
        }
        if report.state == TaskState::Applying
            && crate::publication::is_applied(repo, &report.proposals)?
        {
            report.state = TaskState::Applied;
            self.save(&report)?;
            return Ok(report);
        }
        report.check_candidate()?;
        report.state = TaskState::Applying;
        self.save(&report)?;
        let checked = match &report.verification {
            super::Verification::Passed { result } => {
                result
                    .source_sha256
                    .clone()
                    .map(|sha256| crate::publication::CheckedSource {
                        scope: None,
                        sha256,
                    })
            }
            _ => None,
        };
        if let Err(error) = crate::publication::apply_checked(
            &report.request.repo,
            &report.proposals,
            checked,
            || report.check_candidate(),
        ) {
            if matches!(
                crate::publication::is_applied(&report.request.repo, &report.proposals),
                Ok(false)
            ) {
                report.state = TaskState::ReadyToApply;
                self.save(&report)?;
            }
            return Err(error);
        }
        report.state = TaskState::Applied;
        self.save(&report)?;
        Ok(report)
    }
}
