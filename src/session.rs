//! Local bounded session snapshots and transactional checkpoints.
//! The original project is never a scratch directory. SQLite contains source;
//! callers must explicitly opt in and keep the database private.
use crate::{
    agent::Report,
    workspace::{SNAPSHOT_BUDGET, SNAPSHOT_FILE_LIMIT, SNAPSHOT_FILES, Workspace},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    io,
    path::{Path, PathBuf},
};

const APPLICATION_ID: u32 = 0x4c595241;
const SESSION_SCHEMA: &str = "CREATE TABLE sessions (id TEXT PRIMARY KEY, root TEXT NOT NULL, task TEXT NOT NULL, report TEXT, revision INTEGER NOT NULL DEFAULT 0)";
const SOURCE_SCHEMA: &str = "CREATE TABLE source_files (session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, path TEXT NOT NULL, content TEXT NOT NULL, PRIMARY KEY(session_id,path))";
fn normalized_schema(sql: &str) -> String {
    sql.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
fn verify_database_format(
    connection: &Connection,
    version: u32,
    application: u32,
) -> io::Result<()> {
    if application != 0 && application != APPLICATION_ID {
        return Err(error("Selected SQLite file is not a Lyra session database"));
    }
    let mut statement = connection
        .prepare(
            "SELECT name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
        )
        .map_err(error)?;
    let objects = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;
    if version == 0 && objects.is_empty() {
        return Ok(());
    }
    let expected = [
        ("sessions", SESSION_SCHEMA),
        ("source_files", SOURCE_SCHEMA),
    ];
    if version != 1
        || objects.len() != expected.len()
        || objects
            .iter()
            .zip(expected)
            .any(|((name, kind, sql), (wanted, schema))| {
                name != wanted
                    || kind != "table"
                    || normalized_schema(sql) != normalized_schema(schema)
            })
    {
        return Err(error(
            "Unknown or modified session database schema; selected file preserved",
        ));
    }
    Ok(())
}
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionListing {
    pub session_id: String,
    pub workspace: PathBuf,
    pub task: String,
    pub status: String,
    pub calls: u32,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionPage {
    pub sessions: Vec<SessionListing>,
    pub offset: usize,
    pub has_more: bool,
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
                "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON;",
            )
            .map_err(error)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(error)?;
        if version > 1 {
            return Err(error("Unsupported session database version"));
        }
        let application: u32 = connection
            .query_row("PRAGMA application_id", [], |row| row.get(0))
            .map_err(error)?;
        verify_database_format(&connection, version, application)?;
        let page_size: u32 = connection
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .map_err(error)?;
        if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err(error("Invalid SQLite page size"));
        }
        connection
            .pragma_update(None, "max_page_count", 32_u32 * 1024 * 1024 / page_size)
            .map_err(error)?;
        if version == 0 {
            connection.execute_batch(&format!("BEGIN IMMEDIATE; {SESSION_SCHEMA}; {SOURCE_SCHEMA}; PRAGMA application_id={APPLICATION_ID}; PRAGMA user_version=1; COMMIT;")).map_err(error)?;
        } else if application == 0 {
            // Only the exact known v1 schema may be adopted; unrelated SQLite files are never initialized.
            connection
                .pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(error)?;
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
        let workspace = Workspace::for_snapshot(repo, files)?;
        let root = workspace
            .root
            .to_str()
            .ok_or_else(|| error("Session root must be UTF-8"))?
            .to_owned();
        let mut sources = Vec::new();
        let mut total = 0usize;
        for path in workspace.files.clone() {
            crate::project::validate_path(&path, false)?;
            let content = workspace.read_snapshot(&path)?;
            total += content.len();
            if total > SNAPSHOT_BUDGET {
                return Err(error("Session snapshot exceeds 16 MiB"));
            }
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
        if count > SNAPSHOT_FILES as i64
            || bytes > SNAPSHOT_BUDGET as i64
            || longest_path > 240
            || largest_file > SNAPSHOT_FILE_LIMIT as i64
        {
            return Err(error("Session snapshot exceeds limits"));
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT path,content FROM source_files WHERE session_id=?1 ORDER BY path LIMIT 2049",
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
            if count > SNAPSHOT_FILES
                || content.len() > SNAPSHOT_FILE_LIMIT
                || total > SNAPSHOT_BUDGET
                || content.contains('\0')
            {
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
                || report.proposals.len() > crate::workspace::MAX_PROPOSALS
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

    pub fn list(&self, offset: usize, limit: usize) -> io::Result<SessionPage> {
        if !(1..=40).contains(&limit) || offset > 100_000 {
            return Err(error("Invalid session page"));
        }
        let mut statement=self.connection.prepare("SELECT id,root,task,substr(coalesce(json_extract(report,'$.status'),'prepared'),1,256),coalesce(json_extract(report,'$.calls'),0) FROM sessions WHERE length(CAST(id AS BLOB))<=128 AND length(CAST(root AS BLOB))<=16384 AND length(CAST(task AS BLOB))<=16384 ORDER BY rowid DESC LIMIT ?1 OFFSET ?2").map_err(error)?;
        let rows = statement
            .query_map(params![(limit + 1) as i64, offset as i64], |row| {
                Ok(SessionListing {
                    session_id: row.get(0)?,
                    workspace: PathBuf::from(row.get::<_, String>(1)?),
                    task: row.get(2)?,
                    status: row.get(3)?,
                    calls: row.get(4)?,
                })
            })
            .map_err(error)?;
        let mut sessions = rows.collect::<Result<Vec<_>, _>>().map_err(error)?;
        let has_more = sessions.len() > limit;
        sessions.truncate(limit);
        Ok(SessionPage {
            sessions,
            offset,
            has_more,
        })
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
    List {
        #[arg(long)]
        db: PathBuf,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
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
        SessionCommand::List { db, offset, limit } => {
            if !db.is_file() {
                return Err(error("Session database does not exist"));
            }
            let page = SessionStore::open(&db)?.list(offset, limit)?;
            serde_json::to_writer_pretty(&mut *output, &page).map_err(error)?;
            writeln!(output)
        }
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

#[cfg(test)]
mod size_tests {
    use super::*;
    #[test]
    fn sqlite_size_limit_uses_the_actual_database_page_size() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("large-pages.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "page_size", 65536_u32)
            .unwrap();
        connection.execute_batch("VACUUM;").unwrap();
        drop(connection);
        let store = SessionStore::open(&path).unwrap();
        let maximum: u32 = store
            .connection
            .query_row("PRAGMA max_page_count", [], |row| row.get(0))
            .unwrap();
        assert_eq!(maximum, 512);
    }
}
