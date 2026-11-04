//! Recoverable multi-file publication. Files are replaced sequentially, never
//! presented as a filesystem-wide atomic transaction. A durable undo journal
//! precedes writes; cooperating CLI publishers hold one repository lock.
use crate::workspace::{Edit, FILE_LIMIT, Workspace};
use cap_std::fs::{Dir, OpenOptions};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const JOURNAL: &str = "apply-journal.json";
const STAGING: &str = "apply-journal.tmp";
const JOURNAL_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Debug, Args)]
pub struct RecoveryArgs {
    #[arg(long)]
    pub repo: PathBuf,
    #[arg(long, value_enum, default_value_t = RecoveryAction::Status)]
    pub action: RecoveryAction,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RecoveryAction {
    Status,
    Rollback,
    Complete,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    root: PathBuf,
    edits: Vec<Edit>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FileState {
    Original,
    Applied,
}
struct State {
    dir: Dir,
    _lock: std::fs::File,
}
fn error(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
fn ordinary(meta: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !meta.file_type().is_symlink()
}
fn checked_exists(dir: &Dir, path: &str) -> io::Result<bool> {
    match dir.symlink_metadata(path) {
        Ok(meta) if ordinary(&meta) && meta.is_file() => Ok(true),
        Ok(_) => Err(error("Publication state must contain only ordinary files")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
fn sync_dir(dir: &Dir) -> io::Result<()> {
    #[cfg(unix)]
    dir.open(".")?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}
impl State {
    fn open(workspace: &Workspace) -> io::Result<Self> {
        match workspace.dir.create_dir(".lyra") {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let meta = workspace.dir.symlink_metadata(".lyra")?;
        if !ordinary(&meta) || !meta.is_dir() {
            return Err(error(".lyra must be an ordinary directory"));
        }
        let dir = workspace.dir.open_dir(".lyra")?;
        let lock = if checked_exists(&dir, "apply.lock")? {
            let mut options = OpenOptions::new();
            options.read(true).write(true);
            dir.open_with("apply.lock", &options)?
        } else {
            match dir.open_with("apply.lock", &private_options()) {
                Ok(lock) => lock,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(error("Publication lock was created concurrently; retry"));
                }
                Err(e) => return Err(e),
            }
        }
        .into_std();
        lock.try_lock()
            .map_err(|e| error(format!("Another publication/recovery is active: {e}")))?;
        Ok(Self { dir, _lock: lock })
    }
    fn pending(&self) -> io::Result<bool> {
        Ok(checked_exists(&self.dir, JOURNAL)? || checked_exists(&self.dir, STAGING)?)
    }
    fn begin(&self, root: &Path, edits: &[Edit]) -> io::Result<()> {
        if self.pending()? {
            return Err(error(
                "Unfinished publication: run lyra recover --repo <project> first",
            ));
        }
        let bytes = serde_json::to_vec(&Journal {
            schema_version: 1,
            root: root.to_owned(),
            edits: edits.to_vec(),
        })
        .map_err(error)?;
        if bytes.len() as u64 > JOURNAL_LIMIT {
            return Err(error("Publication journal exceeds limit"));
        }
        // A crash during staging cannot leave any project writes. Recovery may
        // discard STAGING only when the authoritative JOURNAL is absent.
        let mut file = self.dir.open_with(STAGING, &private_options())?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        self.dir.rename(STAGING, &self.dir, JOURNAL)?;
        sync_dir(&self.dir)
    }
    fn read(&self, root: &Path) -> io::Result<Journal> {
        if !checked_exists(&self.dir, JOURNAL)? {
            return Err(error("No publication journal"));
        }
        let mut bytes = Vec::new();
        self.dir
            .open(JOURNAL)?
            .take(JOURNAL_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > JOURNAL_LIMIT {
            return Err(error("Publication journal exceeds limit"));
        }
        let journal: Journal = serde_json::from_slice(&bytes).map_err(error)?;
        if journal.schema_version != 1 || journal.root != root {
            return Err(error(
                "Unsupported publication journal or different project",
            ));
        }
        validate_set(&journal.edits)?;
        Ok(journal)
    }
    fn clear(&self) -> io::Result<()> {
        self.dir.remove_file(JOURNAL)?;
        sync_dir(&self.dir)
    }
}

fn validate_set(edits: &[Edit]) -> io::Result<()> {
    if edits.is_empty() || edits.len() > 8 {
        return Err(error("Publication requires 1..8 proposals"));
    }
    let mut paths = BTreeSet::new();
    for edit in edits {
        crate::project::validate_path(&edit.path, false)?;
        if edit.original.len() > FILE_LIMIT
            || edit.replacement.len() > FILE_LIMIT
            || edit.original.contains('\0')
            || edit.replacement.contains('\0')
            || edit.create && !edit.original.is_empty()
            || !edit.create && edit.original == edit.replacement
            || !paths.insert(edit.path.to_lowercase())
        {
            return Err(error("Invalid or duplicate publication proposal"));
        }
    }
    for path in &paths {
        if paths
            .iter()
            .any(|other| other.starts_with(&format!("{path}/")))
        {
            return Err(error("Publication paths cannot be parents of one another"));
        }
    }
    Ok(())
}
fn open_workspace(repo: &Path, edits: &[Edit]) -> io::Result<Workspace> {
    let selected: Vec<_> = edits
        .iter()
        .filter(|e| !e.create)
        .map(|e| e.path.clone())
        .collect();
    Workspace::open(repo, &selected)
}
fn states(workspace: &mut Workspace, edits: &[Edit]) -> io::Result<Vec<FileState>> {
    edits
        .iter()
        .map(|edit| {
            if edit.create && workspace.new_path(&edit.path).is_ok() {
                return Ok(FileState::Original);
            }
            workspace.safe_path(&edit.path)?;
            if !workspace.dir.symlink_metadata(&edit.path)?.is_file() {
                return Err(error(format!(
                    "Recovery conflict: {} is not a regular file",
                    edit.path
                )));
            }
            let file = workspace.dir.open(&edit.path)?;
            if !file.metadata()?.is_file() {
                return Err(error(format!(
                    "Recovery conflict: {} is not a regular file",
                    edit.path
                )));
            }
            let mut current = String::new();
            file.take((FILE_LIMIT + 1) as u64)
                .read_to_string(&mut current)?;
            if current == edit.replacement {
                Ok(FileState::Applied)
            } else if !edit.create && current == edit.original {
                Ok(FileState::Original)
            } else {
                Err(error(format!(
                    "Recovery conflict: {}; external changes preserved",
                    edit.path
                )))
            }
        })
        .collect()
}
fn sync_project(workspace: &Workspace, edits: &[Edit]) -> io::Result<()> {
    for edit in edits {
        let mut parent = Path::new(&edit.path).parent();
        while let Some(path) = parent.filter(|p| !p.as_os_str().is_empty()) {
            // Parents of uncreated files need not exist after rollback.
            match workspace.dir.open_dir(path) {
                Ok(dir) => sync_dir(&dir)?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            parent = path.parent();
        }
    }
    sync_dir(&workspace.dir)
}
fn restore(workspace: &mut Workspace, edits: &[Edit], action: RecoveryAction) -> io::Result<()> {
    // Check the whole set before touching anything, including unchanged paths.
    let initial = states(workspace, edits)?;
    let mut changes = Vec::new();
    for (edit, state) in edits.iter().zip(initial) {
        match (action, state) {
            (RecoveryAction::Complete, FileState::Original) => changes.push(edit.clone()),
            (RecoveryAction::Rollback, FileState::Applied) if !edit.create => {
                changes.push(Edit {
                    path: edit.path.clone(),
                    original: edit.replacement.clone(),
                    replacement: edit.original.clone(),
                    create: false,
                });
            }
            _ => {}
        }
    }
    for edit in &changes {
        workspace.files.insert(edit.path.clone());
        workspace.validate_edit(edit)?;
    }
    for edit in &changes {
        workspace.apply(edit)?;
    }
    if action == RecoveryAction::Rollback {
        for edit in edits.iter().rev().filter(|e| e.create) {
            if states(workspace, std::slice::from_ref(edit))?[0] == FileState::Applied {
                // Recheck content immediately before unlink; never remove a foreign file.
                workspace.dir.remove_file(&edit.path)?;
            }
        }
    }
    sync_project(workspace, edits)?;
    ensure_final(workspace, edits, action)
}
fn ensure_final(
    workspace: &mut Workspace,
    edits: &[Edit],
    action: RecoveryAction,
) -> io::Result<()> {
    let expected = if action == RecoveryAction::Rollback {
        FileState::Original
    } else {
        FileState::Applied
    };
    if states(workspace, edits)?.iter().any(|s| *s != expected) {
        return Err(error("Files changed during publication; journal retained"));
    }
    Ok(())
}

pub fn apply(repo: &Path, edits: &[Edit]) -> io::Result<()> {
    apply_with(repo, edits, |workspace, edit| workspace.apply(edit))
}
fn apply_with(
    repo: &Path,
    edits: &[Edit],
    mut publish: impl FnMut(&mut Workspace, &Edit) -> io::Result<()>,
) -> io::Result<()> {
    validate_set(edits)?;
    let mut workspace = open_workspace(repo, edits)?;
    let state = State::open(&workspace)?;
    if state.pending()? {
        return Err(error(
            "Unfinished publication: run lyra recover --repo <project> first",
        ));
    }
    for edit in edits {
        workspace.validate_edit(edit)?;
    }
    state.begin(&workspace.root, edits)?;
    let result = edits
        .iter()
        .try_for_each(|edit| publish(&mut workspace, edit))
        .and_then(|()| sync_project(&workspace, edits))
        .and_then(|()| ensure_final(&mut workspace, edits, RecoveryAction::Complete));
    match result {
        Ok(()) => state.clear(),
        Err(cause) => match restore(&mut workspace, edits, RecoveryAction::Rollback)
            .and_then(|()| state.clear())
        {
            Ok(()) => Err(error(format!(
                "Publication failed; changes rolled back: {cause}"
            ))),
            Err(rollback) => Err(error(format!(
                "Publication interrupted: {cause}; journal retained: {rollback}. Run lyra recover --repo <project>"
            ))),
        },
    }
}

pub fn recover(args: RecoveryArgs, output: &mut impl Write) -> io::Result<()> {
    let workspace = Workspace::open(&args.repo, &[])?;
    let state = State::open(&workspace)?;
    if !checked_exists(&state.dir, JOURNAL)? {
        let staged = checked_exists(&state.dir, STAGING)?;
        if staged && args.action != RecoveryAction::Status {
            state.dir.remove_file(STAGING)?;
            sync_dir(&state.dir)?;
        }
        return writeln!(
            output,
            "{}",
            serde_json::json!({"status": if staged && args.action == RecoveryAction::Status {"staging_only_no_project_writes"} else if staged {"staging_discarded_no_project_writes"} else {"no_pending_publication"}})
        );
    }
    let journal = state.read(&workspace.root)?;
    let mut workspace = open_workspace(&args.repo, &journal.edits)?;
    let current = states(&mut workspace, &journal.edits)?;
    if args.action != RecoveryAction::Status {
        restore(&mut workspace, &journal.edits, args.action)?;
        state.clear()?;
    }
    writeln!(
        output,
        "{}",
        serde_json::json!({
            "status": match args.action { RecoveryAction::Status => "pending", RecoveryAction::Rollback => "rolled_back", RecoveryAction::Complete => "completed" },
            "files": journal.edits.iter().zip(current).map(|(e,s)| serde_json::json!({"path":e.path,"state_before":s})).collect::<Vec<_>>()
        })
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edits() -> Vec<Edit> {
        vec![
            Edit {
                path: "src/данные.rs".into(),
                original: "old a\r\n".into(),
                replacement: "new a\r\n".into(),
                create: false,
            },
            Edit {
                path: "b.txt".into(),
                original: "dirty user content\n".into(),
                replacement: "new b\n".into(),
                create: false,
            },
            Edit {
                path: "new/deep/file.txt".into(),
                original: "".into(),
                replacement: "created\r\n".into(),
                create: true,
            },
        ]
    }
    fn fixture() -> tempfile::TempDir {
        let repo = tempfile::Builder::new()
            .prefix("lyra публикация с пробелами-")
            .tempdir()
            .unwrap();
        std::fs::create_dir(repo.path().join("src")).unwrap();
        for edit in edits().iter().filter(|e| !e.create) {
            std::fs::write(repo.path().join(&edit.path), &edit.original).unwrap();
        }
        repo
    }
    fn interrupted(repo: &Path, count: usize) -> State {
        let edits = edits();
        let mut workspace = open_workspace(repo, &edits).unwrap();
        let state = State::open(&workspace).unwrap();
        for edit in &edits {
            workspace.validate_edit(edit).unwrap();
        }
        state.begin(&workspace.root, &edits).unwrap();
        for edit in edits.iter().take(count) {
            workspace.apply(edit).unwrap();
        }
        // Dropping the lock does not roll back; the journal survives restart.
        state
    }
    fn recovery(repo: &Path, action: RecoveryAction) -> io::Result<String> {
        let mut output = Vec::new();
        recover(
            RecoveryArgs {
                repo: repo.into(),
                action,
            },
            &mut output,
        )?;
        Ok(String::from_utf8(output).unwrap())
    }
    fn assert_original(repo: &Path) {
        for edit in edits() {
            if edit.create {
                assert!(!repo.join(edit.path).exists());
            } else {
                assert_eq!(
                    std::fs::read_to_string(repo.join(edit.path)).unwrap(),
                    edit.original
                );
            }
        }
    }
    #[test]
    fn whole_set_preflight_prevents_early_writes_and_preserves_dirty_content() {
        let repo = fixture();
        let edits = edits();
        std::fs::write(repo.path().join("b.txt"), "external edit").unwrap();
        assert!(apply(repo.path(), &edits).is_err());
        assert_eq!(
            std::fs::read_to_string(repo.path().join(&edits[0].path)).unwrap(),
            edits[0].original
        );
        assert!(!repo.path().join(&edits[2].path).exists());
        assert!(!repo.path().join(".lyra").join(JOURNAL).exists());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("b.txt")).unwrap(),
            "external edit"
        );
    }
    #[test]
    fn batch_handles_crlf_unicode_nested_files_and_preserves_permissions() {
        let repo = fixture();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                repo.path().join(&edits()[0].path),
                std::fs::Permissions::from_mode(0o750),
            )
            .unwrap();
        }
        apply(repo.path(), &edits()).unwrap();
        for edit in edits() {
            assert_eq!(
                std::fs::read_to_string(repo.path().join(edit.path)).unwrap(),
                edit.replacement
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(repo.path().join(&edits()[0].path))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o750
            );
        }
        assert!(!repo.path().join(".lyra").join(JOURNAL).exists());
        assert!(apply(repo.path(), &edits()).is_err());
    }
    #[test]
    fn each_interruption_point_can_roll_back_or_complete_after_restart() {
        for count in 0..=edits().len() {
            for action in [RecoveryAction::Rollback, RecoveryAction::Complete] {
                let repo = fixture();
                interrupted(repo.path(), count);
                let before = std::fs::read(repo.path().join(&edits()[0].path)).unwrap();
                assert!(
                    recovery(repo.path(), RecoveryAction::Status)
                        .unwrap()
                        .contains("pending")
                );
                assert_eq!(
                    std::fs::read(repo.path().join(&edits()[0].path)).unwrap(),
                    before
                );
                assert!(
                    apply(repo.path(), &edits())
                        .unwrap_err()
                        .to_string()
                        .contains("Unfinished")
                );
                recovery(repo.path(), action).unwrap();
                if action == RecoveryAction::Rollback {
                    assert_original(repo.path());
                } else {
                    for edit in edits() {
                        assert_eq!(
                            std::fs::read_to_string(repo.path().join(edit.path)).unwrap(),
                            edit.replacement
                        );
                    }
                }
                assert!(
                    recovery(repo.path(), action)
                        .unwrap()
                        .contains("no_pending")
                );
            }
        }
    }
    #[test]
    fn external_change_blocks_all_recovery_writes_and_keeps_journal() {
        for action in [RecoveryAction::Rollback, RecoveryAction::Complete] {
            let repo = fixture();
            interrupted(repo.path(), 1);
            std::fs::write(repo.path().join("b.txt"), "external edit").unwrap();
            assert!(
                recovery(repo.path(), action)
                    .unwrap_err()
                    .to_string()
                    .contains("external changes preserved")
            );
            assert_eq!(
                std::fs::read_to_string(repo.path().join(&edits()[0].path)).unwrap(),
                edits()[0].replacement
            );
            assert!(repo.path().join(".lyra").join(JOURNAL).exists());
            assert_eq!(
                std::fs::read_to_string(repo.path().join("b.txt")).unwrap(),
                "external edit"
            );
            assert!(!repo.path().join(&edits()[2].path).exists());
        }
    }
    #[test]
    fn repository_lock_blocks_other_publishers_and_recovery() {
        let repo = fixture();
        let workspace = open_workspace(repo.path(), &edits()).unwrap();
        let lock = State::open(&workspace).unwrap();
        assert!(apply(repo.path(), &edits()).is_err());
        assert!(recovery(repo.path(), RecoveryAction::Rollback).is_err());
        assert_original(repo.path());
        drop(lock);
        apply(repo.path(), &edits()).unwrap();
    }
    #[test]
    fn readonly_late_file_is_rejected_before_first_write() {
        let repo = fixture();
        let path = repo.path().join("b.txt");
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        let original = permissions.clone();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(apply(repo.path(), &edits()).is_err());
        assert_original(repo.path());
        std::fs::set_permissions(path, original).unwrap();
    }
    #[test]
    fn malformed_wrong_root_duplicate_and_parent_paths_fail_closed() {
        let repo = fixture();
        let mut duplicate = edits();
        duplicate.push(duplicate[0].clone());
        assert!(apply(repo.path(), &duplicate).is_err());
        duplicate = edits();
        duplicate[1].path = duplicate[0].path.to_uppercase();
        assert!(apply(repo.path(), &duplicate).is_err());
        duplicate = edits();
        duplicate[1].path = "new".into();
        duplicate[1].create = true;
        duplicate[1].original.clear();
        assert!(apply(repo.path(), &duplicate).is_err());
        interrupted(repo.path(), 1);
        let path = repo.path().join(".lyra").join(JOURNAL);
        let mut journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        journal["root"] = serde_json::json!("another-project");
        std::fs::write(&path, journal.to_string()).unwrap();
        assert!(recovery(repo.path(), RecoveryAction::Complete).is_err());
        std::fs::write(&path, "{truncated").unwrap();
        assert!(recovery(repo.path(), RecoveryAction::Rollback).is_err());
        assert_eq!(
            std::fs::read_to_string(repo.path().join(&edits()[0].path)).unwrap(),
            edits()[0].replacement
        );
        assert!(path.exists());
    }
    #[test]
    fn crash_before_journal_rename_only_leaves_discardable_staging() {
        let repo = fixture();
        let workspace = open_workspace(repo.path(), &edits()).unwrap();
        let state = State::open(&workspace).unwrap();
        std::fs::write(repo.path().join(".lyra").join(STAGING), "{unfinished").unwrap();
        drop(state);
        assert!(apply(repo.path(), &edits()).is_err());
        assert!(
            recovery(repo.path(), RecoveryAction::Status)
                .unwrap()
                .contains("staging_only")
        );
        assert!(repo.path().join(".lyra").join(STAGING).exists());
        recovery(repo.path(), RecoveryAction::Rollback).unwrap();
        assert_original(repo.path());
        apply(repo.path(), &edits()).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn state_and_target_symlinks_cannot_escape_or_delete_other_files() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let repo = fixture();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), repo.path().join(".lyra")).unwrap();
        assert!(apply(repo.path(), &edits()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        std::fs::remove_file(repo.path().join(".lyra")).unwrap();
        interrupted(repo.path(), 3);
        let path = repo.path().join(".lyra").join(JOURNAL);
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let target = repo.path().join(&edits()[2].path);
        std::fs::remove_file(&target).unwrap();
        let sentinel = outside.path().join("sentinel");
        std::fs::write(&sentinel, "created\r\n").unwrap();
        symlink(&sentinel, &target).unwrap();
        assert!(recovery(repo.path(), RecoveryAction::Rollback).is_err());
        assert!(target.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "created\r\n");
    }
    #[test]
    fn write_error_after_one_file_rolls_back_the_whole_set() {
        let repo = fixture();
        let mut count = 0;
        let result = apply_with(repo.path(), &edits(), |workspace, edit| {
            count += 1;
            if count == 2 {
                return Err(io::Error::other("injected disk write failure"));
            }
            workspace.apply(edit)
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("changes rolled back")
        );
        assert_original(repo.path());
        assert!(!repo.path().join(".lyra").join(JOURNAL).exists());
    }
    #[test]
    fn external_change_during_write_preserves_it_and_keeps_undo_for_earlier_files() {
        let repo = fixture();
        let mut count = 0;
        let result = apply_with(repo.path(), &edits(), |workspace, edit| {
            count += 1;
            if count == 2 {
                std::fs::write(repo.path().join("b.txt"), "concurrent user edit")?;
            }
            workspace.apply(edit)
        });
        assert!(result.unwrap_err().to_string().contains("journal retained"));
        assert_eq!(
            std::fs::read_to_string(repo.path().join("b.txt")).unwrap(),
            "concurrent user edit"
        );
        assert_eq!(
            std::fs::read_to_string(repo.path().join(&edits()[0].path)).unwrap(),
            edits()[0].replacement
        );
        assert!(repo.path().join(".lyra").join(JOURNAL).exists());
        std::fs::write(repo.path().join("b.txt"), &edits()[1].original).unwrap();
        recovery(repo.path(), RecoveryAction::Rollback).unwrap();
        assert_original(repo.path());
    }
    #[test]
    fn incomplete_new_file_is_preserved_for_manual_review_instead_of_deleted_as_ours() {
        let repo = fixture();
        let result = apply_with(repo.path(), &edits(), |workspace, edit| {
            if edit.create {
                std::fs::create_dir_all(repo.path().join("new/deep"))?;
                std::fs::write(repo.path().join(&edit.path), "cre")?;
                return Err(io::Error::other("interrupted CREATE_NEW write"));
            }
            workspace.apply(edit)
        });
        assert!(result.unwrap_err().to_string().contains("journal retained"));
        assert_eq!(
            std::fs::read_to_string(repo.path().join(&edits()[2].path)).unwrap(),
            "cre"
        );
        assert!(recovery(repo.path(), RecoveryAction::Rollback).is_err());
        let journal: Journal = serde_json::from_slice(
            &std::fs::read(repo.path().join(".lyra").join(JOURNAL)).unwrap(),
        )
        .unwrap();
        assert_eq!(journal.edits[0].original, edits()[0].original);
        // Explicitly reviewing/removing the unexpected file allows safe recovery.
        std::fs::remove_file(repo.path().join(&edits()[2].path)).unwrap();
        recovery(repo.path(), RecoveryAction::Rollback).unwrap();
        assert_original(repo.path());
    }
    #[test]
    fn crash_child() {
        let Some(root) = std::env::var_os("LYRA_TEST_PUBLICATION_ROOT") else {
            return;
        };
        let _state = interrupted(Path::new(&root), 1);
        std::process::exit(73);
    }
    #[test]
    fn hard_process_exit_releases_lock_and_retains_recoverable_journal() {
        let repo = fixture();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "publication::tests::crash_child"])
            .env("LYRA_TEST_PUBLICATION_ROOT", repo.path())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        assert_eq!(
            std::fs::read_to_string(repo.path().join(&edits()[0].path)).unwrap(),
            edits()[0].replacement
        );
        recovery(repo.path(), RecoveryAction::Rollback).unwrap();
        assert_original(repo.path());
    }
}
