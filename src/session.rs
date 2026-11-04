//! Local bounded session snapshots and transactional checkpoints.
//! The original project is never a scratch directory. SQLite contains source;
//! callers must explicitly opt in and keep the database private.
use crate::{
    agent::Report,
    workspace::{FILE_LIMIT, READ_BUDGET, Workspace},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    io,
    path::{Path, PathBuf},
};

const REPORT_LIMIT: usize = 2 * 1024 * 1024;
fn error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

type SessionMetadata = (String, String, Option<String>, i64);

pub struct SessionStore {
    connection: Connection,
    revisions: std::cell::RefCell<std::collections::HashMap<String, i64>>,
}
pub struct MaterializedSession {
    pub directory: tempfile::TempDir,
    pub original_root: PathBuf,
    pub task: String,
    pub report: Option<Report>,
}

impl SessionStore {
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        if path.try_exists()? && std::fs::metadata(path)?.len() > 32 * 1024 * 1024 {
            return Err(error("Session database exceeds 32 MiB"));
        }
        if !path.try_exists()? {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(path) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let connection = Connection::open(path).map_err(error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(2))
            .map_err(error)?;
        connection
            .execute_batch(
                "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA max_page_count=8192;",
            )
            .map_err(error)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(error)?;
        if version > 1 {
            return Err(error("Unsupported session database version"));
        }
        if version == 0 {
            connection.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE sessions (id TEXT PRIMARY KEY, root TEXT NOT NULL, task TEXT NOT NULL, report TEXT, revision INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE source_files (session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    path TEXT NOT NULL, content TEXT NOT NULL, PRIMARY KEY(session_id,path));
                PRAGMA user_version=1; COMMIT;").map_err(error)?;
        }
        Ok(Self {
            connection,
            revisions: Default::default(),
        })
    }

    pub fn create(&mut self, repo: &Path, files: &[String], task: &str) -> io::Result<String> {
        if task.trim().is_empty() || task.len() > 16384 {
            return Err(error("Invalid session task"));
        }
        let mut workspace = Workspace::open(repo, files)?;
        if workspace.truncated {
            return Err(error(
                "Incomplete session snapshot; select files explicitly",
            ));
        }
        let root = workspace
            .root
            .to_str()
            .ok_or_else(|| error("Session root must be UTF-8"))?
            .to_owned();
        let mut sources = Vec::new();
        for path in workspace.files.clone() {
            crate::project::validate_path(&path, false)?;
            let content = workspace.read(&path)?;
            sources.push((path, content));
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(error)?
            .as_nanos();
        let id = format!("{}-{nonce}", std::process::id());
        let transaction = self.connection.transaction().map_err(error)?;
        transaction
            .execute(
                "INSERT INTO sessions(id,root,task) VALUES (?1,?2,?3)",
                params![id, root, task],
            )
            .map_err(error)?;
        for (path, content) in sources {
            transaction
                .execute(
                    "INSERT INTO source_files(session_id,path,content) VALUES (?1,?2,?3)",
                    params![id, path, content],
                )
                .map_err(error)?;
        }
        transaction.commit().map_err(error)?;
        self.revisions.borrow_mut().insert(id.clone(), 0);
        Ok(id)
    }

    fn metadata(&self, id: &str) -> io::Result<SessionMetadata> {
        if id.len() > 128 {
            return Err(error("Invalid session id"));
        }
        self.connection.query_row("SELECT root,task,report,revision FROM sessions WHERE id=?1 AND length(CAST(root AS BLOB))<=16384 AND length(CAST(task AS BLOB))<=16384 AND (report IS NULL OR length(CAST(report AS BLOB))<=2097152)", [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).map_err(error)
    }

    pub fn materialize(&self, id: &str) -> io::Result<MaterializedSession> {
        let (root, task, report, revision) = self.metadata(id)?;
        self.revisions.borrow_mut().insert(id.into(), revision);
        let directory = crate::workspace::private_tempdir("lyra-session-")?;
        let (count, bytes, longest_path, largest_file): (i64, i64, i64, i64) = self.connection.query_row(
            "SELECT count(*),coalesce(sum(length(CAST(content AS BLOB))),0),coalesce(max(length(CAST(path AS BLOB))),0),coalesce(max(length(CAST(content AS BLOB))),0) FROM source_files WHERE session_id=?1",
            [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(error)?;
        if count > 129
            || bytes > READ_BUDGET as i64
            || longest_path > 240
            || largest_file > FILE_LIMIT as i64
        {
            return Err(error("Session snapshot exceeds limits"));
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT path,content FROM source_files WHERE session_id=?1 ORDER BY path LIMIT 130",
            )
            .map_err(error)?;
        let mut rows = statement.query([id]).map_err(error)?;
        let mut total = 0usize;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(error)? {
            let path: String = row.get(0).map_err(error)?;
            let content: String = row.get(1).map_err(error)?;
            count += 1;
            total = total.saturating_add(content.len());
            if count > 129 || content.len() > FILE_LIMIT || total > READ_BUDGET {
                return Err(error("Session snapshot exceeds limits"));
            }
            crate::project::validate_path(&path, false)?;
            let destination = directory.path().join(path);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(destination, content)?;
        }
        let mut report: Option<Report> = report
            .map(|json| serde_json::from_str(&json).map_err(error))
            .transpose()?;
        if let Some(report) = &mut report {
            if report.schema_version != 4
                || report.workspace != Path::new(&root)
                || report.calls > 256
                || report.proposals.len() > 8
            {
                return Err(error("Invalid session report"));
            }
            report.workspace = directory.path().canonicalize()?;
        }
        Ok(MaterializedSession {
            directory,
            original_root: root.into(),
            task,
            report,
        })
    }

    pub fn checkpoint(&self, id: &str, report: &Report) -> io::Result<()> {
        let (root, _, _, _) = self.metadata(id)?;
        let expected = *self
            .revisions
            .borrow()
            .get(id)
            .ok_or_else(|| error("Materialize the session before checkpointing"))?;
        let mut value = serde_json::to_value(report).map_err(error)?;
        value["workspace"] = serde_json::Value::String(root);
        let json = serde_json::to_string(&value).map_err(error)?;
        if json.len() > REPORT_LIMIT {
            return Err(error("Session report exceeds 2 MiB"));
        }
        if self
            .connection
            .execute(
                "UPDATE sessions SET report=?1,revision=revision+1 WHERE id=?2 AND revision=?3",
                params![json, id, expected],
            )
            .map_err(error)?
            != 1
        {
            return Err(error(
                "Session changed concurrently or disappeared before checkpoint",
            ));
        }
        self.revisions.borrow_mut().insert(id.into(), expected + 1);
        Ok(())
    }

    pub fn report(&self, id: &str) -> io::Result<Option<Report>> {
        let (_, _, report, _) = self.metadata(id)?;
        report
            .map(|json| serde_json::from_str(&json).map_err(error))
            .transpose()
    }

    pub fn delete(&mut self, id: &str) -> io::Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM sessions WHERE id=?1", [id])
            .map_err(error)?
            == 1)
    }

    pub fn exists(&self, id: &str) -> io::Result<bool> {
        self.connection
            .query_row("SELECT 1 FROM sessions WHERE id=?1", [id], |_| Ok(()))
            .optional()
            .map(|row| row.is_some())
            .map_err(error)
    }
}

#[derive(Debug, clap::Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}
#[derive(Debug, clap::Subcommand)]
pub enum SessionCommand {
    Show {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        id: String,
        /// Export the complete report, including code, for review/apply
        #[arg(long)]
        report: bool,
    },
    Delete {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        id: String,
    },
}
pub fn run(args: SessionArgs, output: &mut impl io::Write) -> io::Result<()> {
    match args.command {
        SessionCommand::Show { db, id, report } => {
            if !db.is_file() {
                return Err(error("Session database does not exist"));
            }
            let store = SessionStore::open(&db)?;
            let saved = store.report(&id)?;
            let value = if report {
                serde_json::to_value(saved.ok_or_else(|| error("No checkpoint report yet"))?)
                    .map_err(error)?
            } else {
                match saved {
                    Some(saved) => {
                        serde_json::json!({"session_id":id,"workspace":saved.workspace,"status":saved.status,
                        "outcome":saved.outcome,"calls":saved.calls,"pending_paths":saved.proposals.iter().map(|p| &p.path).collect::<Vec<_>>() })
                    }
                    None => serde_json::json!({"session_id":id,"status":"prepared"}),
                }
            };
            serde_json::to_writer_pretty(&mut *output, &value).map_err(error)?;
            writeln!(output)
        }
        SessionCommand::Delete { db, id } => {
            if !db.is_file() {
                return Err(error("Session database does not exist"));
            }
            if !SessionStore::open(&db)?.delete(&id)? {
                return Err(error("Session does not exist"));
            }
            writeln!(output, "Session deleted: {id}")
        }
    }
}
