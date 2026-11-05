//! Content-addressed binary proposals; never embed documents in model JSON.
use crate::workspace::Workspace;
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
pub const FILE_LIMIT: usize = 64 * 1024 * 1024;
pub const TOTAL_LIMIT: usize = 256 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Blob {
    pub sha256: String,
    pub bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub path: String,
    pub blob: Blob,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileProposal {
    pub path: String,
    pub original: Option<Blob>,
    pub replacement: Blob,
    #[serde(default)]
    pub inputs: Vec<Input>,
}
pub struct Store {
    pub root: PathBuf,
    dir: Dir,
}
fn bad(s: impl std::fmt::Display) -> io::Error {
    io::Error::other(s.to_string())
}
fn hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn ordinary(m: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !m.file_type().is_symlink()
}
impl Store {
    pub fn open(root: &Path) -> io::Result<Self> {
        if !root.exists() {
            std::fs::create_dir_all(root)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        let m = std::fs::symlink_metadata(root)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(bad("Artifact store must be an ordinary directory"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(bad("Reparse artifact store rejected"));
            }
        }
        Ok(Self {
            root: root.canonicalize()?,
            dir: Dir::open_ambient_dir(root, ambient_authority())?,
        })
    }
    pub(crate) fn session(root: &Path, id: &str) -> io::Result<Self> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(bad("Invalid session object directory"));
        }
        let parent = Self::open(root)?;
        Self::child(&parent.dir, id, parent.root.join(id))
    }
    fn child(parent: &Dir, name: &str, root: PathBuf) -> io::Result<Self> {
        match parent.create_dir(name) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let m = parent.symlink_metadata(name)?;
        if !ordinary(&m) || !m.is_dir() {
            return Err(bad("Invalid journal object directory"));
        }
        Ok(Self {
            root,
            dir: parent.open_dir(name)?,
        })
    }
    pub fn put(&self, bytes: &[u8]) -> io::Result<Blob> {
        if bytes.len() > FILE_LIMIT {
            return Err(bad("Artifact exceeds 64 MiB"));
        }
        let blob = Blob {
            sha256: crate::retrieval::sha256(bytes),
            bytes: bytes.len(),
        };
        if self.dir.try_exists(&blob.sha256)? {
            self.get(&blob)?;
            return Ok(blob);
        }
        let mut total = 0u64;
        let mut entries = 0;
        for e in self.dir.entries()? {
            let e = e?;
            entries += 1;
            total = total.saturating_add(e.metadata()?.len());
            if entries > 4096 || total + bytes.len() as u64 > 1024 * 1024 * 1024 {
                return Err(bad("Artifact store exceeds 1 GiB or 4096 entries"));
            }
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = match self.dir.open_with(&blob.sha256, &options) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                self.get(&blob)?;
                return Ok(blob);
            }
            Err(e) => return Err(e),
        };
        if let Err(e) = f.write_all(bytes).and_then(|()| f.sync_all()) {
            drop(f);
            let _ = self.dir.remove_file(&blob.sha256);
            return Err(e);
        }
        #[cfg(unix)]
        {
            self.dir.open(".")?.sync_all()?;
        }
        Ok(blob)
    }
    pub fn get(&self, b: &Blob) -> io::Result<Vec<u8>> {
        if !hash(&b.sha256) || b.bytes > FILE_LIMIT {
            return Err(bad("Invalid blob reference"));
        }
        let m = self.dir.symlink_metadata(&b.sha256)?;
        if !ordinary(&m) || !m.is_file() || m.len() != b.bytes as u64 {
            return Err(bad("Blob type or size changed"));
        }
        let mut bytes = Vec::new();
        self.dir
            .open(&b.sha256)?
            .take(FILE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != b.bytes || crate::retrieval::sha256(&bytes) != b.sha256 {
            return Err(bad("Artifact hash mismatch"));
        }
        Ok(bytes)
    }
}
pub fn read(workspace: &Workspace, path: &str) -> io::Result<Vec<u8>> {
    workspace.safe_path(path)?;
    let meta = workspace.dir.symlink_metadata(path)?;
    if !ordinary(&meta) || !meta.is_file() || meta.len() > FILE_LIMIT as u64 {
        return Err(bad("Input is not a bounded ordinary file"));
    }
    let mut bytes = Vec::new();
    workspace
        .dir
        .open(path)?
        .take(FILE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > FILE_LIMIT {
        return Err(bad("Input exceeds 64 MiB"));
    }
    Ok(bytes)
}
const JOURNAL: &str = "artifact-journal.json";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    root: PathBuf,
    files: Vec<FileProposal>,
}
fn checked(
    workspace: &Workspace,
    files: &[FileProposal],
    allow_applied: bool,
    check_inputs: bool,
    approved: &[FileProposal],
) -> io::Result<Vec<bool>> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    let mut total = 0;
    if files.is_empty() || files.len() > 32 {
        return Err(bad("Expected 1..32 file proposals"));
    }
    for f in files {
        crate::project::validate_path(&f.path, false)?;
        if !seen.insert(f.path.to_lowercase()) {
            return Err(bad("Duplicate output path"));
        }
        total += f.replacement.bytes;
        if total > TOTAL_LIMIT {
            return Err(bad("Output set exceeds 256 MiB"));
        }
        let current = match read(workspace, &f.path) {
            Ok(b) => Some(Blob {
                sha256: crate::retrieval::sha256(&b),
                bytes: b.len(),
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let applied = current.as_ref() == Some(&f.replacement);
        if current != f.original && !(allow_applied && applied) {
            return Err(bad(format!("File conflict: {}", f.path)));
        }
        for input in f.inputs.iter().filter(|_| check_inputs) {
            let bytes = read(workspace, &input.path)?;
            let actual = crate::retrieval::sha256(&bytes);
            if actual != input.blob.sha256
                && !approved.iter().any(|p| {
                    p.path == input.path
                        && ((p.replacement.sha256 == input.blob.sha256
                            && p.original.as_ref().is_some_and(|o| o.sha256 == actual))
                            || p.replacement.sha256 == actual)
                })
            {
                return Err(bad(format!(
                    "Input changed after execution: {}",
                    input.path
                )));
            }
        }
        result.push(applied);
    }
    for p in &seen {
        if seen.iter().any(|q| q.starts_with(&format!("{p}/"))) {
            return Err(bad("Output paths overlap"));
        }
    }
    Ok(result)
}
fn write(workspace: &Workspace, path: &str, bytes: Option<&[u8]>) -> io::Result<()> {
    if let Some(bytes) = bytes {
        let exists = workspace.dir.try_exists(path)?;
        if exists {
            workspace.safe_path(path)?;
        } else {
            let mut validation = Workspace::open(&workspace.root, &[])?;
            validation.new_path(path)?;
        }
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            workspace.dir.create_dir_all(parent)?;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(bad)?
            .as_nanos();
        let temporary = format!(".lyra/artifact-write-{}-{nonce}.tmp", std::process::id());
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = workspace.dir.open_with(&temporary, &opts)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if let Err(e) = workspace.dir.rename(&temporary, &workspace.dir, path) {
            let _ = workspace.dir.remove_file(&temporary);
            return Err(e);
        }
        #[cfg(unix)]
        {
            let parent = Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            workspace.dir.open_dir(parent)?.open(".")?.sync_all()?;
        }
    } else {
        workspace.safe_path(path)?;
        workspace.dir.remove_file(path)?;
    }
    Ok(())
}
pub fn publish(
    repo: &Path,
    store: &Store,
    files: &[FileProposal],
    guard: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    publish_in_context(repo, store, files, files, guard)
}
pub fn publish_in_context(
    repo: &Path,
    store: &Store,
    files: &[FileProposal],
    approved: &[FileProposal],
    guard: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let workspace = Workspace::open(repo, &[])?;
    let state = crate::publication::State::open(&workspace)?;
    if state.pending()? {
        return Err(bad("Unfinished publication; run recover first"));
    }
    checked(&workspace, files, false, true, approved)?;
    guard()?;
    let objects = Store::child(
        &state.dir,
        "artifact-objects",
        workspace.root.join(".lyra/artifact-objects"),
    )?;
    for f in files {
        objects.put(&store.get(&f.replacement)?)?;
        if let Some(b) = &f.original {
            objects.put(&store.get(b)?)?;
        }
    }
    let journal = Journal {
        schema_version: 2,
        root: workspace.root.clone(),
        files: files.to_vec(),
    };
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    let mut out = state.dir.open_with("artifact-journal.tmp", &opts)?;
    serde_json::to_writer(&mut out, &journal).map_err(bad)?;
    out.sync_all()?;
    drop(out);
    state
        .dir
        .rename("artifact-journal.tmp", &state.dir, JOURNAL)?;
    sync(&state.dir)?;
    let result = (|| {
        for f in files {
            checked(&workspace, files, true, true, approved)?;
            write(&workspace, &f.path, Some(&objects.get(&f.replacement)?))?;
        }
        checked(&workspace, files, true, true, approved)?;
        guard()?;
        Ok(())
    })();
    if let Err(e) = result {
        if restore(&workspace, &objects, files, false).is_err() {
            return Err(bad(format!(
                "Publication interrupted: {e}; recovery journal retained"
            )));
        }
        state.dir.remove_file(JOURNAL)?;
        sync(&state.dir)?;
        clear_objects(&objects, files)?;
        return Err(e);
    }
    state.dir.remove_file(JOURNAL)?;
    sync(&state.dir)?;
    clear_objects(&objects, files)
}
fn clear_objects(objects: &Store, files: &[FileProposal]) -> io::Result<()> {
    let mut refs = BTreeSet::new();
    for f in files {
        refs.insert(f.replacement.sha256.clone());
        if let Some(b) = &f.original {
            refs.insert(b.sha256.clone());
        }
    }
    for sha in refs {
        let m = objects.dir.symlink_metadata(&sha)?;
        if !ordinary(&m) || !m.is_file() {
            return Err(bad("Applied, but backup cleanup blocked by changed object"));
        }
        objects.dir.remove_file(sha)?;
    }
    Ok(())
}
fn sync(dir: &Dir) -> io::Result<()> {
    #[cfg(unix)]
    {
        dir.open(".")?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}
fn restore(
    workspace: &Workspace,
    store: &Store,
    files: &[FileProposal],
    complete: bool,
) -> io::Result<()> {
    // Validate the complete set before touching any file. External edits are preserved.
    let states = checked(workspace, files, true, false, files)?;
    for (f, applied) in files.iter().zip(states) {
        if applied == complete {
            continue;
        }
        let blob = if complete {
            Some(&f.replacement)
        } else {
            f.original.as_ref()
        };
        let bytes = blob.map(|b| store.get(b)).transpose()?;
        if bytes.is_none() && !workspace.dir.try_exists(&f.path)? {
            continue;
        }
        write(workspace, &f.path, bytes.as_deref())?;
    }
    Ok(())
}
pub(crate) fn recover_locked(
    workspace: &Workspace,
    state: &Dir,
    action: crate::publication::RecoveryAction,
    out: &mut impl Write,
) -> io::Result<bool> {
    if !state.try_exists(JOURNAL)? {
        // Uncommitted staging contains backups only, never published project writes.
        // Prefer an existing legacy journal before cleaning unrelated staging.
        if state.try_exists("apply-journal.json")? {
            return Ok(false);
        }
        let mut staging = Vec::new();
        for (index, entry) in state.entries()?.enumerate() {
            if index > 10000 {
                return Err(bad("Publication state exceeds entry limit"));
            }
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let temporary = name
                .strip_prefix("artifact-write-")
                .and_then(|s| s.strip_suffix(".tmp"))
                .is_some_and(|s| {
                    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'-')
                });
            if temporary || name == "artifact-journal.tmp" {
                let m = state.symlink_metadata(&name)?;
                if !ordinary(&m) || !m.is_file() {
                    return Err(bad("Changed publication staging preserved"));
                }
                staging.push(name);
            }
        }
        let objects = if state.try_exists("artifact-objects")? {
            Some(Store::child(
                state,
                "artifact-objects",
                workspace.root.join(".lyra/artifact-objects"),
            )?)
        } else {
            None
        };
        let mut blobs = Vec::new();
        if let Some(store) = &objects {
            for (index, entry) in store.dir.entries()?.enumerate() {
                if index > 4096 {
                    return Err(bad("Too many staging objects"));
                }
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let m = store.dir.symlink_metadata(&name)?;
                if !hash(&name) || !ordinary(&m) || !m.is_file() {
                    return Err(bad("Unknown staging object preserved"));
                }
                blobs.push(name);
            }
        }
        if staging.is_empty() && blobs.is_empty() {
            return Ok(false);
        }
        let status = if action == crate::publication::RecoveryAction::Status {
            "staging_only_no_project_writes"
        } else {
            for name in staging {
                state.remove_file(name)?;
            }
            if let Some(store) = objects {
                for name in blobs {
                    store.dir.remove_file(name)?;
                }
                sync(&store.dir)?;
            }
            sync(state)?;
            "staging_discarded_no_project_writes"
        };
        writeln!(out, "{}", serde_json::json!({"status":status}))?;
        return Ok(true);
    }
    let m = state.symlink_metadata(JOURNAL)?;
    if !ordinary(&m) || !m.is_file() || m.len() > 1024 * 1024 {
        return Err(bad("Invalid artifact journal"));
    }
    let j: Journal =
        serde_json::from_reader(state.open(JOURNAL)?.take(1024 * 1024)).map_err(bad)?;
    if j.schema_version != 2 || j.root != workspace.root {
        return Err(bad("Artifact journal belongs to another workspace"));
    }
    let states = checked(workspace, &j.files, true, false, &j.files)?;
    use crate::publication::RecoveryAction;
    if action != RecoveryAction::Status {
        let store = Store::child(
            state,
            "artifact-objects",
            workspace.root.join(".lyra/artifact-objects"),
        )?;
        restore(
            workspace,
            &store,
            &j.files,
            action == RecoveryAction::Complete,
        )?;
        state.remove_file(JOURNAL)?;
        sync(state)?;
        clear_objects(&store, &j.files)?;
    }
    writeln!(
        out,
        "{}",
        serde_json::json!({"status":match action{RecoveryAction::Status=>"pending",RecoveryAction::Rollback=>"rolled_back",RecoveryAction::Complete=>"completed"},"files":j.files.iter().zip(states).map(|(f,s)|serde_json::json!({"path":f.path,"state_before":if s{"applied"}else{"original"}})).collect::<Vec<_>>()})
    )?;
    Ok(true)
}
