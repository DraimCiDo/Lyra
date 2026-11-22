//! Agent hub: a local coordination board that lets several coding agents (Codex, Claude
//! Code, local Lyra workers) work on one project at the same time.
//!
//! Each claimed task gets its own git worktree and branch, so agents never edit the same
//! checkout. Agents coordinate through tasks, advisory file leases and messages stored in
//! one SQLite database that every agent process opens concurrently. Agents reach the hub
//! through MCP (`lyra-v2 hub mcp`); the owner keeps the decisions that change the project:
//! the check command, landing a branch and cancelling work.
pub mod cli;
#[cfg(feature = "desktop")]
pub mod desktop;
mod git;
pub mod mcp;
pub mod worker;

use crate::executor::{self, ExecutionRequest};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DATABASE: &str = "hub.db";
const SCHEMA_VERSION: u32 = 1;
const APPLICATION_ID: i32 = 0x4c59_4842; // "LYHB"
/// An agent that has not called the hub for this long is shown as idle.
const ACTIVE_SECS: i64 = 10 * 60;
const MAX_LEASE_SECS: u64 = 4 * 60 * 60;
const MAX_MESSAGES: i64 = 10_000;

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE agents(name TEXT PRIMARY KEY, kind TEXT NOT NULL,
    first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL);
CREATE TABLE tasks(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL,
    description TEXT NOT NULL, status TEXT NOT NULL, owner TEXT, created_by TEXT NOT NULL,
    depends_on TEXT NOT NULL, branch TEXT, worktree TEXT, summary TEXT NOT NULL,
    check_result TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE leases(key TEXT PRIMARY KEY, path TEXT NOT NULL, agent TEXT NOT NULL,
    task INTEGER, expires_at INTEGER NOT NULL);
CREATE TABLE messages(id INTEGER PRIMARY KEY AUTOINCREMENT, sender TEXT NOT NULL,
    recipient TEXT, task INTEGER, body TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE cursors(agent TEXT PRIMARY KEY, last_message INTEGER NOT NULL);
CREATE TABLE events(id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL,
    agent TEXT NOT NULL, task INTEGER, kind TEXT NOT NULL, detail TEXT NOT NULL);
";

/// The name the owner's own CLI commands use in the hub log.
pub const OWNER: &str = "owner";

fn error(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}

fn db(error: rusqlite::Error) -> io::Error {
    match error {
        rusqlite::Error::SqliteFailure(code, _)
            if code.code == rusqlite::ErrorCode::DatabaseBusy =>
        {
            io::Error::new(io::ErrorKind::WouldBlock, "The agent hub is busy; retry")
        }
        other => io::Error::other(other),
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs() as i64)
}

fn bounded(name: &str, text: &str, limit: usize) -> io::Result<()> {
    if text.trim().is_empty() || text.len() > limit || text.contains('\0') {
        return Err(error(format!("{name} must be 1 to {limit} bytes of text")));
    }
    Ok(())
}

pub fn validate_agent(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > 64
        || name == OWNER
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(error(
            "An agent name uses 1-64 letters, digits, '-', '_' or '.' and is not 'owner'",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    Codex,
    ClaudeCode,
    Lyra,
    Other,
}

impl AgentKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::Lyra => "lyra",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Claimed,
    Review,
    Done,
    Cancelled,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Claimed => "claimed",
            Self::Review => "review",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }
    fn parse(text: &str) -> io::Result<Self> {
        Ok(match text {
            "open" => Self::Open,
            "claimed" => Self::Claimed,
            "review" => Self::Review,
            "done" => Self::Done,
            "cancelled" => Self::Cancelled,
            _ => return Err(error("Unknown task status in the hub database")),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub status: Status,
    pub owner: Option<String>,
    pub created_by: String,
    pub depends_on: Vec<i64>,
    pub branch: Option<String>,
    pub worktree: Option<PathBuf>,
    pub summary: String,
    pub check: Option<Value>,
}

/// The result of the owner-configured check for one exact branch commit.
#[derive(Debug, Serialize, serde::Deserialize)]
struct CheckRecord {
    passed: bool,
    head: String,
    clean: bool,
    exit_code: Option<i32>,
    timed_out: bool,
    elapsed_ms: u64,
    output: String,
}

pub struct Hub {
    connection: Connection,
    root: PathBuf,
    repo: PathBuf,
    base: String,
    check: Option<ExecutionRequest>,
    agent: String,
}

fn open_connection(path: &Path) -> io::Result<Connection> {
    let connection = Connection::open(path).map_err(db)?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(db)?;
    connection
        .execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;",
        )
        .map_err(db)?;
    Ok(connection)
}

impl Hub {
    /// Create a hub for a git project. The hub folder lives outside the project.
    pub fn init(
        root: &Path,
        repo: &Path,
        base: &str,
        check: Option<ExecutionRequest>,
    ) -> io::Result<()> {
        let repo = repo.canonicalize()?;
        let top = git::ok(&repo, &["rev-parse", "--show-toplevel"])
            .map_err(|_| error("The agent hub needs a git repository"))?;
        if Path::new(&top).canonicalize()? != repo {
            return Err(error("Choose the top folder of the git repository"));
        }
        if base.is_empty() || base.starts_with('-') || base.len() > 200 {
            return Err(error("Invalid base branch"));
        }
        git::ok(
            &repo,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )
        .map_err(|_| error(format!("Base branch '{base}' was not found")))?;
        if let Some(check) = &check {
            check.validate()?;
        }
        let parent = root
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        let root = parent.join(
            root.file_name()
                .ok_or_else(|| error("Invalid hub folder"))?,
        );
        if root.starts_with(&repo) {
            return Err(error("Choose a hub folder outside the project"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&root)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&root)?;
        let connection = open_connection(&root.join(DATABASE))?;
        let check = check
            .map(|check| serde_json::to_string(&check))
            .transpose()
            .map_err(error)?;
        let mut connection = connection;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        transaction.execute_batch(SCHEMA).map_err(db)?;
        for (key, value) in [
            ("repo", Some(repo.to_string_lossy().into_owned())),
            ("base", Some(base.to_owned())),
            ("check", check),
        ] {
            if let Some(value) = value {
                transaction
                    .execute("INSERT INTO meta VALUES (?1, ?2)", params![key, value])
                    .map_err(db)?;
            }
        }
        transaction
            .execute_batch(&format!(
                "PRAGMA application_id={APPLICATION_ID}; PRAGMA user_version={SCHEMA_VERSION};"
            ))
            .map_err(db)?;
        transaction.commit().map_err(db)
    }

    /// Open the hub as an agent (registering it) or as the owner.
    pub fn open(root: &Path, agent: &str, kind: Option<AgentKind>) -> io::Result<Self> {
        if agent != OWNER {
            validate_agent(agent)?;
        }
        let root = root.canonicalize()?;
        let path = root.join(DATABASE);
        if !fs::symlink_metadata(&path)?.is_file() {
            return Err(error("Not an agent hub folder"));
        }
        let connection = open_connection(&path)?;
        let (application, version): (i32, u32) = connection
            .query_row(
                "SELECT (SELECT application_id FROM pragma_application_id), (SELECT user_version FROM pragma_user_version)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(db)?;
        if application != APPLICATION_ID || version != SCHEMA_VERSION {
            return Err(error("Unsupported agent hub database"));
        }
        let meta = |key: &str| -> io::Result<Option<String>> {
            connection
                .query_row("SELECT value FROM meta WHERE key=?1", [key], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(db)
        };
        let repo = PathBuf::from(meta("repo")?.ok_or_else(|| error("Hub has no project"))?);
        let base = meta("base")?.ok_or_else(|| error("Hub has no base branch"))?;
        let check = meta("check")?
            .map(|text| serde_json::from_str::<ExecutionRequest>(&text))
            .transpose()
            .map_err(error)?;
        let hub = Self {
            connection,
            root,
            repo,
            base,
            check,
            agent: agent.to_owned(),
        };
        if agent != OWNER {
            let kind = kind.unwrap_or(AgentKind::Other).as_str();
            hub.connection
                .execute(
                    "INSERT INTO agents VALUES (?1, ?2, ?3, ?3)
                     ON CONFLICT(name) DO UPDATE SET kind=?2, last_seen=?3",
                    params![agent, kind, now()],
                )
                .map_err(db)?;
        }
        Ok(hub)
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    fn touch(&self) -> io::Result<()> {
        if self.agent != OWNER {
            self.connection
                .execute(
                    "UPDATE agents SET last_seen=?2 WHERE name=?1",
                    params![self.agent, now()],
                )
                .map_err(db)?;
        }
        Ok(())
    }

    fn log(&self, task: Option<i64>, kind: &str, detail: &str) -> io::Result<()> {
        self.connection
            .execute(
                "INSERT INTO events(at, agent, task, kind, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    now(),
                    self.agent,
                    task,
                    kind,
                    &detail[..detail.floor_char_boundary(500)]
                ],
            )
            .map_err(db)?;
        Ok(())
    }

    fn row_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Task, String)> {
        let depends: String = row.get(6)?;
        let check: Option<String> = row.get(10)?;
        Ok((
            Task {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                status: Status::Open,
                owner: row.get(4)?,
                created_by: row.get(5)?,
                depends_on: serde_json::from_str(&depends).unwrap_or_default(),
                branch: row.get(7)?,
                worktree: row.get::<_, Option<String>>(8)?.map(PathBuf::from),
                summary: row.get(9)?,
                check: check.and_then(|text| serde_json::from_str(&text).ok()),
            },
            row.get(3)?,
        ))
    }

    const TASK_COLUMNS: &str = "id, title, description, status, owner, created_by, depends_on, branch, worktree, summary, check_result";

    pub fn task(&self, id: i64) -> io::Result<Task> {
        let (mut task, status) = self
            .connection
            .query_row(
                &format!("SELECT {} FROM tasks WHERE id=?1", Self::TASK_COLUMNS),
                [id],
                Self::row_task,
            )
            .optional()
            .map_err(db)?
            .ok_or_else(|| error(format!("Task {id} does not exist")))?;
        task.status = Status::parse(&status)?;
        Ok(task)
    }

    pub fn tasks(&self, status: Option<Status>) -> io::Result<Vec<Task>> {
        self.touch()?;
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {} FROM tasks WHERE ?1 IS NULL OR status=?1 ORDER BY id",
                Self::TASK_COLUMNS
            ))
            .map_err(db)?;
        let rows = statement
            .query_map([status.map(Status::as_str)], Self::row_task)
            .map_err(db)?;
        rows.map(|row| {
            let (mut task, status) = row.map_err(db)?;
            task.status = Status::parse(&status)?;
            Ok(task)
        })
        .collect()
    }

    pub fn create_task(
        &self,
        title: &str,
        description: &str,
        depends_on: &[i64],
    ) -> io::Result<i64> {
        self.touch()?;
        bounded("A title", title, 200)?;
        bounded("A description", description, 16 * 1024)?;
        if depends_on.len() > 16 {
            return Err(error("A task can depend on at most 16 tasks"));
        }
        for id in depends_on {
            self.task(*id)?;
        }
        let at = now();
        self.connection
            .execute(
                "INSERT INTO tasks(title, description, status, created_by, depends_on, summary, created_at, updated_at)
                 VALUES (?1, ?2, 'open', ?3, ?4, '', ?5, ?5)",
                params![title.trim(), description, self.agent, serde_json::to_string(depends_on).map_err(error)?, at],
            )
            .map_err(db)?;
        let id = self.connection.last_insert_rowid();
        self.log(Some(id), "create", title)?;
        Ok(id)
    }

    /// Claim a specific open task, or the oldest open task whose dependencies are done.
    /// The task gets its own branch and worktree; the claim is atomic across agents.
    pub fn claim(&mut self, id: Option<i64>) -> io::Result<Task> {
        if self.agent == OWNER {
            return Err(error("Only an agent can claim a task"));
        }
        self.touch()?;
        let agent = self.agent.clone();
        let worktrees = self.root.join("worktrees");
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let candidates: Vec<(i64, String)> = {
            let mut statement = transaction
                .prepare("SELECT id, depends_on FROM tasks WHERE status='open' AND (?1 IS NULL OR id=?1) ORDER BY id")
                .map_err(db)?;
            statement
                .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
        };
        let mut chosen = None;
        for (task, depends) in candidates {
            let depends: Vec<i64> = serde_json::from_str(&depends).unwrap_or_default();
            let mut ready = true;
            for dependency in depends {
                let status: Option<String> = transaction
                    .query_row(
                        "SELECT status FROM tasks WHERE id=?1",
                        [dependency],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(db)?;
                ready &= status.as_deref() == Some("done");
            }
            if ready {
                chosen = Some(task);
                break;
            }
        }
        let Some(task) = chosen else {
            return Err(match id {
                Some(id) => error(format!(
                    "Task {id} is not open or waits for unfinished dependencies"
                )),
                None => io::Error::new(io::ErrorKind::NotFound, "No open task is ready to claim"),
            });
        };
        let branch = format!("lyra/task-{task}");
        let worktree = worktrees.join(format!("task-{task}"));
        transaction
            .execute(
                "UPDATE tasks SET status='claimed', owner=?2, branch=?3, worktree=?4, updated_at=?5 WHERE id=?1",
                params![task, agent, branch, worktree.to_string_lossy(), now()],
            )
            .map_err(db)?;
        transaction.commit().map_err(db)?;
        if let Err(cause) = self.ensure_worktree(&branch, &worktree) {
            self.connection
                .execute(
                    "UPDATE tasks SET status='open', owner=NULL, updated_at=?2 WHERE id=?1",
                    params![task, now()],
                )
                .map_err(db)?;
            return Err(cause);
        }
        self.log(Some(task), "claim", &branch)?;
        self.task(task)
    }

    fn ensure_worktree(&self, branch: &str, worktree: &Path) -> io::Result<()> {
        if worktree.join(".git").exists() {
            return Ok(());
        }
        fs::create_dir_all(self.root.join("worktrees"))?;
        let path = worktree.to_string_lossy();
        let exists = git::run(
            &self.repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )?
        .success;
        let args: Vec<&str> = if exists {
            vec!["worktree", "add", &path, branch]
        } else {
            vec!["worktree", "add", "-b", branch, &path, &self.base]
        };
        // Agents in other processes may hold git's ref or config lock for a moment.
        let mut attempt = 1;
        loop {
            match git::ok(&self.repo, &args) {
                Err(error) if attempt < 4 && error.to_string().contains(".lock") => {
                    std::thread::sleep(Duration::from_millis(250 * attempt));
                    attempt += 1;
                }
                result => return result.map(drop),
            }
        }
    }

    fn owned_task(&self, id: i64) -> io::Result<Task> {
        let task = self.task(id)?;
        if self.agent != OWNER && task.owner.as_deref() != Some(&self.agent) {
            return Err(error(format!("Task {id} is not claimed by {}", self.agent)));
        }
        Ok(task)
    }

    /// Agent transitions: claimed -> review (hand off), claimed -> open (release),
    /// review -> claimed (continue). The owner may also reopen a task under review.
    pub fn update(&self, id: i64, status: Status, summary: &str) -> io::Result<Task> {
        self.touch()?;
        let task = self.owned_task(id)?;
        if summary.len() > 8 * 1024 || summary.contains('\0') {
            return Err(error("A summary is at most 8 KiB of text"));
        }
        let allowed = matches!(
            (task.status, status),
            (Status::Claimed, Status::Review | Status::Open) | (Status::Review, Status::Claimed)
        ) || self.agent == OWNER
            && matches!((task.status, status), (Status::Review, Status::Open));
        if !allowed {
            return Err(error(format!(
                "Cannot move task {id} from {} to {}",
                task.status.as_str(),
                status.as_str()
            )));
        }
        let owner = if status == Status::Open {
            None
        } else {
            task.owner.clone()
        };
        let summary = if summary.trim().is_empty() {
            task.summary.as_str()
        } else {
            summary
        };
        self.connection
            .execute(
                "UPDATE tasks SET status=?2, owner=?3, summary=?4, updated_at=?5 WHERE id=?1",
                params![id, status.as_str(), owner, summary, now()],
            )
            .map_err(db)?;
        if status == Status::Open {
            self.connection
                .execute("DELETE FROM leases WHERE task=?1", [id])
                .map_err(db)?;
        }
        self.log(Some(id), status.as_str(), summary)?;
        self.task(id)
    }

    pub fn cancel(&self, id: i64) -> io::Result<Task> {
        if self.agent != OWNER {
            return Err(error("Only the owner can cancel a task"));
        }
        let task = self.task(id)?;
        if matches!(task.status, Status::Done | Status::Cancelled) {
            return Err(error(format!("Task {id} is already finished")));
        }
        self.connection
            .execute(
                "UPDATE tasks SET status='cancelled', owner=NULL, updated_at=?2 WHERE id=?1",
                params![id, now()],
            )
            .map_err(db)?;
        self.connection
            .execute("DELETE FROM leases WHERE task=?1", [id])
            .map_err(db)?;
        self.log(Some(id), "cancel", "")?;
        self.task(id)
    }

    fn lease_key(path: &str) -> io::Result<String> {
        let trimmed = path.trim_end_matches('/');
        crate::project::validate_path(trimmed, true)?;
        Ok(trimmed.to_lowercase())
    }

    /// Advisory exclusive leases on project paths (a folder covers everything below it).
    /// All requested paths are leased or none are; conflicts name the holder.
    pub fn lease(
        &mut self,
        task: Option<i64>,
        paths: &[String],
        ttl_secs: u64,
    ) -> io::Result<Value> {
        self.touch()?;
        if paths.is_empty() || paths.len() > 64 || !(60..=MAX_LEASE_SECS).contains(&ttl_secs) {
            return Err(error("Lease 1 to 64 paths for 60 to 14400 seconds"));
        }
        if let Some(task) = task {
            self.owned_task(task)?;
        }
        let keys = paths
            .iter()
            .map(|path| {
                Ok((
                    Self::lease_key(path)?,
                    path.trim_end_matches('/').to_owned(),
                ))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let agent = self.agent.clone();
        let at = now();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        transaction
            .execute("DELETE FROM leases WHERE expires_at<=?1", [at])
            .map_err(db)?;
        let held: Vec<(String, String, String)> = {
            let mut statement = transaction
                .prepare("SELECT key, path, agent FROM leases WHERE agent<>?1")
                .map_err(db)?;
            statement
                .query_map([&agent], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
        };
        let overlaps = |a: &str, b: &str| {
            a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
        };
        let conflicts: Vec<_> = keys
            .iter()
            .flat_map(|(key, path)| {
                held.iter()
                    .filter(|(other, ..)| overlaps(key, other))
                    .map(move |(_, held_path, holder)| json!({"path": path, "held": held_path, "agent": holder}))
            })
            .collect();
        if !conflicts.is_empty() {
            return Ok(json!({"leased": false, "conflicts": conflicts}));
        }
        let expires = at + ttl_secs as i64;
        for (key, path) in &keys {
            transaction
                .execute(
                    "INSERT INTO leases VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(key) DO UPDATE SET path=?2, task=?4, expires_at=?5",
                    params![key, path, agent, task, expires],
                )
                .map_err(db)?;
        }
        transaction.commit().map_err(db)?;
        self.log(task, "lease", &paths.join(", "))?;
        Ok(json!({"leased": true, "expires_at": expires, "paths": paths}))
    }

    /// Release the given paths, or every lease of this agent when `paths` is empty.
    pub fn release(&self, paths: &[String]) -> io::Result<usize> {
        self.touch()?;
        let mut released = 0;
        if paths.is_empty() {
            released = self
                .connection
                .execute("DELETE FROM leases WHERE agent=?1", [&self.agent])
                .map_err(db)?;
        }
        for path in paths {
            released += self
                .connection
                .execute(
                    "DELETE FROM leases WHERE key=?1 AND agent=?2",
                    params![Self::lease_key(path)?, self.agent],
                )
                .map_err(db)?;
        }
        Ok(released)
    }

    pub fn send(&self, to: Option<&str>, task: Option<i64>, body: &str) -> io::Result<i64> {
        self.touch()?;
        bounded("A message", body, 8 * 1024)?;
        if let Some(to) = to
            && to != OWNER
        {
            validate_agent(to)?;
        }
        if let Some(task) = task {
            self.task(task)?;
        }
        self.connection
            .execute(
                "INSERT INTO messages(sender, recipient, task, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![self.agent, to, task, body, now()],
            )
            .map_err(db)?;
        let id = self.connection.last_insert_rowid();
        self.connection
            .execute("DELETE FROM messages WHERE id<=?1", [id - MAX_MESSAGES])
            .map_err(db)?;
        Ok(id)
    }

    /// Unread messages addressed to this agent or to everyone, oldest first.
    pub fn read(&self, limit: usize) -> io::Result<Vec<Value>> {
        self.touch()?;
        let cursor: i64 = self
            .connection
            .query_row(
                "SELECT last_message FROM cursors WHERE agent=?1",
                [&self.agent],
                |row| row.get(0),
            )
            .optional()
            .map_err(db)?
            .unwrap_or(0);
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, sender, recipient, task, body, created_at FROM messages
                 WHERE id>?1 AND sender<>?2 AND (recipient IS NULL OR recipient=?2)
                 ORDER BY id LIMIT ?3",
            )
            .map_err(db)?;
        let messages: Vec<(i64, Value)> = statement
            .query_map(params![cursor, self.agent, limit.clamp(1, 100) as i64], |row| {
                Ok((
                    row.get(0)?,
                    json!({
                        "id": row.get::<_, i64>(0)?, "from": row.get::<_, String>(1)?,
                        "to": row.get::<_, Option<String>>(2)?, "task": row.get::<_, Option<i64>>(3)?,
                        "body": row.get::<_, String>(4)?, "at": row.get::<_, i64>(5)?,
                    }),
                ))
            })
            .map_err(db)?
            .collect::<rusqlite::Result<_>>()
            .map_err(db)?;
        if let Some((last, _)) = messages.last() {
            self.connection
                .execute(
                    "INSERT INTO cursors VALUES (?1, ?2) ON CONFLICT(agent) DO UPDATE SET last_message=?2",
                    params![self.agent, last],
                )
                .map_err(db)?;
        }
        Ok(messages.into_iter().map(|(_, message)| message).collect())
    }

    pub fn status(&self) -> io::Result<Value> {
        self.touch()?;
        let at = now();
        let agents: Vec<Value> =
            {
                let mut statement = self
                    .connection
                    .prepare("SELECT name, kind, last_seen FROM agents ORDER BY name")
                    .map_err(db)?;
                statement
                .query_map([], |row| {
                    let seen: i64 = row.get(2)?;
                    Ok(json!({"name": row.get::<_, String>(0)?, "kind": row.get::<_, String>(1)?,
                        "last_seen": seen, "active": at - seen < ACTIVE_SECS}))
                })
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
            };
        let leases: Vec<Value> = {
            let mut statement = self
                .connection
                .prepare("SELECT path, agent, task, expires_at FROM leases WHERE expires_at>?1 ORDER BY path")
                .map_err(db)?;
            statement
                .query_map([at], |row| {
                    Ok(json!({"path": row.get::<_, String>(0)?, "agent": row.get::<_, String>(1)?,
                        "task": row.get::<_, Option<i64>>(2)?, "expires_at": row.get::<_, i64>(3)?}))
                })
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
        };
        let tasks: Vec<Value> = self
            .tasks(None)?
            .into_iter()
            .filter(|task| !matches!(task.status, Status::Done | Status::Cancelled))
            .map(|task| {
                json!({"id": task.id, "title": task.title, "status": task.status,
                "owner": task.owner, "depends_on": task.depends_on,
                "check_passed": task.check.as_ref().map(|check| check["passed"].clone())})
            })
            .collect();
        Ok(json!({
            "project": self.repo, "base": self.base, "check_configured": self.check.is_some(),
            "agents": agents, "active_tasks": tasks, "leases": leases,
        }))
    }

    /// Run the owner-configured check in the task worktree and record it for the
    /// exact branch commit. Only a clean worktree produces a landable result.
    pub fn check(&self, id: i64, cancel: &AtomicBool) -> io::Result<Value> {
        self.touch()?;
        let task = self.owned_task(id)?;
        let check = self
            .check
            .as_ref()
            .ok_or_else(|| error("The owner did not configure a check for this hub"))?;
        let worktree = task
            .worktree
            .filter(|path| path.join(".git").exists())
            .ok_or_else(|| error(format!("Task {id} has no worktree")))?;
        let head = git::head(&worktree)?;
        let clean = !git::dirty(&worktree)?;
        let result = executor::verify(&worktree, &[], check, cancel)?;
        let output = format!("{}\n{}", result.stdout, result.stderr);
        let record = CheckRecord {
            passed: result.passed(),
            head,
            clean,
            exit_code: result.exit_code,
            timed_out: result.timed_out,
            elapsed_ms: result.elapsed_ms,
            output: output[..output.floor_char_boundary(4000)].to_owned(),
        };
        let value = serde_json::to_value(&record).map_err(error)?;
        self.connection
            .execute(
                "UPDATE tasks SET check_result=?2, updated_at=?3 WHERE id=?1",
                params![id, value.to_string(), now()],
            )
            .map_err(db)?;
        self.log(
            Some(id),
            "check",
            if record.passed { "passed" } else { "failed" },
        )?;
        Ok(value)
    }

    /// The task branch compared with the base, for review.
    pub fn diff(&self, id: i64, limit: usize) -> io::Result<Value> {
        self.touch()?;
        let task = self.task(id)?;
        let branch = task
            .branch
            .ok_or_else(|| error(format!("Task {id} has no branch")))?;
        let range = format!("{}...{branch}", self.base);
        let stat = git::ok(&self.repo, &["diff", "--stat", &range])?;
        let patch = git::ok(&self.repo, &["diff", &range])?;
        let limit = limit.clamp(1024, 512 * 1024);
        let uncommitted = task
            .worktree
            .filter(|path| path.join(".git").exists())
            .map(|path| git::dirty(&path))
            .transpose()?
            .unwrap_or(false);
        Ok(json!({
            "branch": branch, "base": self.base, "stat": stat,
            "patch": &patch[..patch.floor_char_boundary(limit)],
            "truncated": patch.len() > limit, "uncommitted_changes": uncommitted,
        }))
    }

    /// Owner only: merge a reviewed task branch into the base branch of the project.
    pub fn land(&self, id: i64) -> io::Result<Value> {
        if self.agent != OWNER {
            return Err(error("Only the owner can land a task"));
        }
        let task = self.task(id)?;
        if task.status != Status::Review {
            return Err(error(format!("Task {id} is not waiting for review")));
        }
        let branch = task
            .branch
            .clone()
            .ok_or_else(|| error("Task has no branch"))?;
        let head = git::ok(&self.repo, &["rev-parse", &format!("refs/heads/{branch}")])?;
        if let Some(worktree) = task
            .worktree
            .as_ref()
            .filter(|path| path.join(".git").exists())
            && git::dirty(worktree)?
        {
            return Err(error("The task worktree has uncommitted changes"));
        }
        if self.check.is_some() {
            let record: Option<CheckRecord> = task
                .check
                .clone()
                .and_then(|value| serde_json::from_value(value).ok());
            if !record.is_some_and(|record| record.passed && record.clean && record.head == head) {
                return Err(error(
                    "The configured check has not passed on the current branch commit",
                ));
            }
        }
        let current = git::ok(&self.repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        if current != self.base {
            return Err(error(format!(
                "Check out '{}' in the project before landing (current: {current})",
                self.base
            )));
        }
        if !git::ok(
            &self.repo,
            &["status", "--porcelain", "--untracked-files=no"],
        )?
        .is_empty()
        {
            return Err(error(
                "The project has uncommitted changes; commit or stash them first",
            ));
        }
        let message = format!("Merge task {id}: {}", task.title);
        let merge = git::run(&self.repo, &["merge", "--no-ff", "-m", &message, &branch])?;
        if !merge.success {
            let _ = git::run(&self.repo, &["merge", "--abort"]);
            return Err(error(format!(
                "The merge conflicts and was aborted: {}",
                merge.stdout.trim()
            )));
        }
        self.connection
            .execute(
                "UPDATE tasks SET status='done', updated_at=?2 WHERE id=?1",
                params![id, now()],
            )
            .map_err(db)?;
        self.connection
            .execute("DELETE FROM leases WHERE task=?1", [id])
            .map_err(db)?;
        if let Some(worktree) = &task.worktree {
            let _ = git::ok(
                &self.repo,
                // Only Lyra's lock files can remain; everything else was committed and merged.
                &["worktree", "remove", "--force", &worktree.to_string_lossy()],
            );
        }
        let _ = git::ok(&self.repo, &["branch", "-d", &branch]);
        self.log(Some(id), "land", &head)?;
        Ok(json!({"landed": id, "branch": branch, "commit": git::head(&self.repo)?}))
    }

    pub(crate) fn check_request(&self) -> Option<&ExecutionRequest> {
        self.check.as_ref()
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests;
