use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

pub(crate) fn private_tempdir(prefix: &str) -> io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir()
}

pub const MAX_PROPOSALS: usize = 32;
pub const INDEX_LIMIT: usize = 8192;
pub const ENTRY_LIMIT: usize = 100_000;
pub const SNAPSHOT_FILES: usize = 2048;
pub const SNAPSHOT_FILE_LIMIT: usize = 2 * 1024 * 1024;
pub const SNAPSHOT_BUDGET: usize = 16 * 1024 * 1024;

const RULE_FILE_LIMIT: usize = 64 * 1024;
pub const FILE_LIMIT: usize = 256 * 1024;
pub const READ_BUDGET: usize = 512 * 1024;

pub struct Workspace {
    pub root: PathBuf,
    pub(crate) dir: Dir,
    pub files: BTreeSet<String>,
    pub truncated: bool,
    pub bytes_read: usize,
    ignores: Vec<(String, ignore::gitignore::Gitignore)>,
    ignore_dirs: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub path: String,
    pub original: String,
    pub replacement: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub create: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn error(message: &str) -> io::Error {
    io::Error::other(message)
}

fn permitted(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.chars().any(char::is_control)
        && !path.contains(['\\', ':', '\0'])
        && Path::new(path).components().all(|part| {
            let Component::Normal(name) = part else {
                return false;
            };
            let name = name.to_string_lossy().to_lowercase();
            !name.starts_with('.')
                && !matches!(
                    name.as_str(),
                    "target"
                        | "node_modules"
                        | "dist"
                        | "vendor"
                        | "logs"
                        | "credentials"
                        | "secrets"
                )
                && ![
                    ".pem", ".key", ".p12", ".pfx", ".gguf", ".exe", ".dll", ".zip", ".log",
                ]
                .iter()
                .any(|ext| name.ends_with(ext))
        })
        && !Path::new(path).is_absolute()
}

impl Workspace {
    pub fn open(root: &Path, selected: &[String]) -> io::Result<Self> {
        if selected.len() > 32 {
            return Err(error("Не более 32 явно выбранных файлов."));
        }
        Self::open_index(root, selected, selected.is_empty())
    }
    pub(crate) fn for_snapshot(root: &Path, selected: &[String]) -> io::Result<Self> {
        if selected.len() > SNAPSHOT_FILES {
            return Err(error(
                "Snapshot exceeds 2048 files; select files explicitly",
            ));
        }
        let workspace = Self::open_index(root, selected, selected.is_empty())?;
        if workspace.truncated || workspace.files.len() > SNAPSHOT_FILES {
            return Err(error(
                "Snapshot index incomplete or exceeds 2048 files; select files explicitly",
            ));
        }
        Ok(workspace)
    }
    pub(crate) fn exact_snapshot_scope(root: &Path, selected: &[String]) -> io::Result<Self> {
        if selected.len() > SNAPSHOT_FILES {
            return Err(error("Snapshot scope exceeds limit"));
        }
        let mut workspace = Self::open_index(root, selected, false)?;
        workspace.files.retain(|p| selected.contains(p));
        Ok(workspace)
    }
    fn open_index(root: &Path, selected: &[String], discover: bool) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let dir = Dir::open_ambient_dir(&root, ambient_authority())?;
        let mut workspace = Self {
            root,
            dir,
            files: BTreeSet::new(),
            truncated: false,
            bytes_read: 0,
            ignores: Vec::new(),
            ignore_dirs: BTreeSet::new(),
        };
        workspace.load_ignore("")?;
        if discover {
            let mut pending = vec![String::new()];
            let mut seen = 0;
            while let Some(folder) = pending.pop() {
                workspace.load_ignore(&folder)?;
                let entries = if folder.is_empty() {
                    workspace.dir.entries()?
                } else {
                    workspace.dir.open_dir(&folder)?.entries()?
                };
                for entry in entries {
                    seen += 1;
                    if seen > ENTRY_LIMIT || workspace.files.len() >= INDEX_LIMIT {
                        workspace.truncated = true;
                        break;
                    }
                    let entry = entry?;
                    let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    let path = if folder.is_empty() {
                        name
                    } else {
                        format!("{folder}/{name}")
                    };
                    if !permitted(&path) || workspace.safe_path(&path).is_err() {
                        continue;
                    }
                    let kind = entry.file_type()?;
                    if workspace.ignored(&path, kind.is_dir()) {
                        continue;
                    }
                    if kind.is_dir() {
                        pending.push(path);
                    } else if kind.is_file() {
                        workspace.files.insert(path);
                    }
                }
                if workspace.truncated {
                    break;
                }
            }
        } else {
            for path in selected {
                workspace.safe_path(path)?;
                let mut folder = Path::new(path).parent();
                while let Some(parent) = folder {
                    let name = parent.to_string_lossy().replace('\\', "/");
                    workspace.load_ignore(&name)?;
                    folder = parent.parent();
                }
                if workspace.ignored(path, false) {
                    return Err(error("Файл исключён gitignore."));
                }
                if !workspace.dir.metadata(path)?.is_file() {
                    return Err(error("Ожидается обычный файл."));
                }
                workspace.files.insert(path.clone());
            }
        }
        // Explicit file selection also carries applicable ancestor guidance.
        for selected_path in selected {
            let mut parent = Path::new(selected_path).parent();
            while let Some(folder) = parent.filter(|p| !p.as_os_str().is_empty()) {
                let scope = format!("{}/AGENTS.md", folder.to_string_lossy().replace('\\', "/"));
                if workspace.safe_path(&scope).is_ok()
                    && !workspace.ignored(&scope, false)
                    && workspace.dir.metadata(&scope).is_ok_and(|m| m.is_file())
                {
                    workspace.files.insert(scope);
                }
                parent = folder.parent();
            }
        }
        if workspace.safe_path("AGENTS.md").is_ok()
            && !workspace.ignored("AGENTS.md", false)
            && workspace
                .dir
                .metadata("AGENTS.md")
                .is_ok_and(|m| m.is_file())
        {
            workspace.files.insert("AGENTS.md".into());
        }
        Ok(workspace)
    }

    fn load_ignore(&mut self, folder: &str) -> io::Result<()> {
        if !self.ignore_dirs.insert(folder.into()) {
            return Ok(());
        }
        let path = if folder.is_empty() {
            ".gitignore".into()
        } else {
            format!("{folder}/.gitignore")
        };
        let metadata = match self.dir.symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(error("Небезопасный gitignore."));
        }
        #[cfg(windows)]
        {
            use cap_std::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(error("Reparse gitignore запрещён."));
            }
        }
        let mut text = String::new();
        self.dir
            .open(&path)?
            .take((RULE_FILE_LIMIT + 1) as u64)
            .read_to_string(&mut text)?;
        if text.len() > RULE_FILE_LIMIT {
            return Err(error("gitignore слишком большой."));
        }
        self.bytes_read = self.bytes_read.saturating_add(text.len());
        if self.bytes_read > READ_BUDGET {
            return Err(error("Исчерпан бюджет чтения правил проекта."));
        }
        let mut builder = ignore::gitignore::GitignoreBuilder::new(self.root.join(folder));
        for line in text.lines() {
            builder.add_line(None, line).map_err(io::Error::other)?;
        }
        self.ignores
            .push((folder.into(), builder.build().map_err(io::Error::other)?));
        self.ignores.sort_by_key(|(folder, _)| folder.len());
        Ok(())
    }

    fn ignored(&self, path: &str, is_dir: bool) -> bool {
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            && self.ignored(&parent.to_string_lossy().replace('\\', "/"), true)
        {
            return true;
        }
        let mut ignored = false;
        for (folder, matcher) in &self.ignores {
            if !folder.is_empty() && !path.starts_with(&format!("{folder}/")) {
                continue;
            }
            match matcher.matched_path_or_any_parents(self.root.join(path), is_dir) {
                ignore::Match::Ignore(_) => ignored = true,
                ignore::Match::Whitelist(_) => ignored = false,
                ignore::Match::None => {}
            }
        }
        ignored
    }

    pub(crate) fn safe_path(&self, path: &str) -> io::Result<()> {
        if !permitted(path) {
            return Err(error("Путь запрещён политикой проекта."));
        }
        let mut prefix = PathBuf::new();
        for component in Path::new(path).components() {
            prefix.push(component);
            let meta = self.dir.symlink_metadata(&prefix)?;
            if meta.file_type().is_symlink() {
                return Err(error("Symlink запрещён."));
            }
            #[cfg(windows)]
            {
                use cap_std::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err(error("Reparse point/junction запрещён."));
                }
            }
        }
        Ok(())
    }

    pub fn read(&mut self, path: &str) -> io::Result<String> {
        if !self.files.contains(path) {
            return Err(error("Файл не входит в выбранный контекст."));
        }
        self.safe_path(path)?;
        let file = self.dir.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(error("Ожидается обычный файл."));
        }
        if self.bytes_read >= READ_BUDGET {
            return Err(error("Исчерпан бюджет чтения проекта."));
        }
        let limit = FILE_LIMIT.min(READ_BUDGET - self.bytes_read);
        let mut bytes = Vec::new();
        file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
        self.bytes_read = self.bytes_read.saturating_add(bytes.len());
        if bytes.len() > limit {
            return Err(error("Файл превышает лимит чтения."));
        }
        if bytes.contains(&0) {
            return Err(error("Двоичные файлы не читаются."));
        }
        String::from_utf8(bytes).map_err(|_| error("Файл не является UTF-8."))
    }

    /// Interactive bounded read; bulk snapshots continue to use `read`'s total budget.
    pub fn read_tool(&self, path: &str) -> io::Result<String> {
        self.read_bounded(path, FILE_LIMIT)
    }
    pub(crate) fn read_snapshot(&self, path: &str) -> io::Result<String> {
        self.read_bounded(path, SNAPSHOT_FILE_LIMIT)
    }
    fn read_bounded(&self, path: &str, limit: usize) -> io::Result<String> {
        if !self.files.contains(path) {
            return Err(error("Файл не входит в выбранный контекст."));
        }
        self.safe_path(path)?;
        if !self.dir.symlink_metadata(path)?.is_file() {
            return Err(error("Ожидается обычный файл."));
        }
        let file = self.dir.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(error("Ожидается обычный файл."));
        }
        let mut bytes = Vec::new();
        file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > limit || bytes.contains(&0) {
            return Err(error("Файл превышает лимит чтения или содержит NUL."));
        }
        String::from_utf8(bytes).map_err(|_| error("Файл не является UTF-8."))
    }

    pub fn scoped_instructions(&self, path: &str) -> io::Result<Vec<serde_json::Value>> {
        if !permitted(path) {
            return Err(error("Путь вне выбранного контекста."));
        }
        let mut scopes = vec!["AGENTS.md".to_owned()];
        let mut prefix = PathBuf::new();
        if let Some(parent) = Path::new(path).parent() {
            for part in parent.components() {
                prefix.push(part);
                scopes.push(format!(
                    "{}/AGENTS.md",
                    prefix.to_string_lossy().replace('\\', "/")
                ));
            }
        }
        if scopes.len() > 16 {
            return Err(error("Слишком глубокая вложенность инструкций."));
        }
        let mut out = Vec::new();
        let mut total = 0;
        for scope in scopes {
            if !self.files.contains(&scope) {
                continue;
            }
            let content = self.read_tool(&scope)?;
            total += content.len();
            if total > 16384 {
                return Err(error("Scoped instructions превышают 16 KiB."));
            }
            out.push(serde_json::json!({"path":scope,"sha256":crate::retrieval::sha256(content.as_bytes()),"content":content,"untrusted":true}));
        }
        Ok(out)
    }

    pub fn propose(&mut self, path: &str, old: &str, new: &str) -> io::Result<Edit> {
        if old.is_empty() || old == new || new.len() > FILE_LIMIT || new.contains('\0') {
            return Err(error("Некорректная точная замена."));
        }
        let original = self.read_tool(path)?;
        if original.matches(old).count() != 1 {
            return Err(error(
                "Исходный фрагмент должен встречаться ровно один раз.",
            ));
        }
        let replacement = original.replacen(old, new, 1);
        if replacement.len() > FILE_LIMIT {
            return Err(error("Изменённый файл превышает лимит."));
        }
        Ok(Edit {
            path: path.into(),
            original,
            replacement,
            create: false,
        })
    }

    /// Read the overlay while retaining the on-disk original as the apply precondition.
    pub fn read_pending(&mut self, pending: &Edit) -> io::Result<String> {
        if pending.create {
            self.new_path(&pending.path)?;
        } else if self.read_tool(&pending.path)? != pending.original {
            return Err(error(
                "Конфликт: исходный файл изменился после предложения.",
            ));
        }
        Ok(pending.replacement.clone())
    }

    pub fn revise(&mut self, pending: &Edit, old: &str, new: &str) -> io::Result<Edit> {
        if old.is_empty() || old == new || new.len() > FILE_LIMIT || new.contains('\0') {
            return Err(error("Некорректная точная замена."));
        }
        let current = self.read_pending(pending)?;
        if current.matches(old).count() != 1 {
            return Err(error(
                "Фрагмент должен встречаться ровно один раз в текущем предложении.",
            ));
        }
        let replacement = current.replacen(old, new, 1);
        if replacement.len() > FILE_LIMIT || replacement.contains('\0') {
            return Err(error("Изменённый файл превышает лимит или содержит NUL."));
        }
        if !pending.create && replacement == pending.original {
            return Err(error(
                "Правка возвращает исходный файл; используй discard_proposal.",
            ));
        }
        Ok(Edit {
            replacement,
            ..pending.clone()
        })
    }

    pub fn propose_new(&mut self, path: &str, content: &str) -> io::Result<Edit> {
        self.new_path(path)?;
        if content.len() > FILE_LIMIT || content.contains('\0') {
            return Err(error("Новый файл: до 256 KiB текста без NUL."));
        }
        Ok(Edit {
            path: path.into(),
            original: String::new(),
            replacement: content.into(),
            create: true,
        })
    }

    pub(crate) fn new_path(&mut self, path: &str) -> io::Result<()> {
        crate::project::validate_path(path, false)?;
        if !permitted(path) {
            return Err(error("Путь запрещён политикой проекта."));
        }
        let mut prefix = PathBuf::new();
        for part in Path::new(path).components() {
            prefix.push(part);
            match self.dir.symlink_metadata(&prefix) {
                Ok(m) => {
                    if m.file_type().is_symlink() {
                        return Err(error("Symlink запрещён."));
                    }
                    #[cfg(windows)]
                    {
                        use cap_std::fs::MetadataExt;
                        if m.file_attributes() & 0x400 != 0 {
                            return Err(error("Reparse point запрещён."));
                        }
                    }
                    if prefix == Path::new(path) {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "Файл уже существует; создание отменено.",
                        ));
                    }
                    if !m.is_dir() {
                        return Err(error("Родитель файла не является каталогом."));
                    }
                    self.load_ignore(&prefix.to_string_lossy().replace('\\', "/"))?;
                    let scope =
                        format!("{}/AGENTS.md", prefix.to_string_lossy().replace('\\', "/"));
                    if self.safe_path(&scope).is_ok()
                        && !self.ignored(&scope, false)
                        && self.dir.metadata(&scope).is_ok_and(|m| m.is_file())
                    {
                        self.files.insert(scope);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        if self.ignored(path, false) {
            return Err(error("Новый файл исключён gitignore."));
        }
        Ok(())
    }

    /// Validate without writing. Batch publication checks every edit first.
    pub(crate) fn validate_edit(&mut self, edit: &Edit) -> io::Result<()> {
        crate::project::validate_path(&edit.path, false)?;
        if edit.original.len() > FILE_LIMIT
            || edit.replacement.len() > FILE_LIMIT
            || edit.original.contains('\0')
            || edit.replacement.contains('\0')
        {
            return Err(error("Некорректный размер или NUL в предложении."));
        }
        if edit.create {
            if !edit.original.is_empty() {
                return Err(error("Новый файл не может иметь исходное содержимое."));
            }
            return self.new_path(&edit.path);
        }
        if !self.files.contains(&edit.path) {
            return Err(error("Файл вне выбранного контекста."));
        }
        if edit.original.len() > FILE_LIMIT
            || edit.replacement.len() > FILE_LIMIT
            || edit.original == edit.replacement
        {
            return Err(error("Некорректное предложение изменения."));
        }
        self.safe_path(&edit.path)?;
        if !self.dir.symlink_metadata(&edit.path)?.is_file() {
            return Err(error("Ожидается обычный файл."));
        }
        let current = self.dir.open(&edit.path)?;
        let permissions = current.metadata()?.permissions();
        if permissions.readonly() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Файл помечен read-only; замена запрещена.",
            ));
        }
        let mut text = String::new();
        current
            .take((FILE_LIMIT + 1) as u64)
            .read_to_string(&mut text)?;
        if text != edit.original {
            return Err(error(
                "Конфликт: исходный файл изменился. Применение отменено.",
            ));
        }
        Ok(())
    }

    pub fn apply(&mut self, edit: &Edit) -> io::Result<()> {
        if edit.create {
            if !edit.original.is_empty() {
                return Err(error("Новый файл не может иметь исходное содержимое."));
            }
            self.propose_new(&edit.path, &edit.replacement)?;
            let mut prefix = PathBuf::new();
            if let Some(parent) = Path::new(&edit.path).parent() {
                for component in parent.components() {
                    prefix.push(component);
                    match self.dir.create_dir(&prefix) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e),
                    };
                    self.safe_path(&prefix.to_string_lossy().replace('\\', "/"))?;
                }
            }
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let temp = format!(".lyra-new-{}-{nonce}.tmp", std::process::id());
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut staged = self.dir.open_with(&temp, &options)?;
            let result = (|| {
                staged.write_all(edit.replacement.as_bytes())?;
                staged.sync_all()?;
                drop(staged);
                self.new_path(&edit.path)?;
                // Atomic no-clobber publication on NTFS/Unix. Fail safely on filesystems without hard links.
                #[cfg(windows)]
                {
                    // Windows can deny hard-link creation through directory handles.
                    // CREATE_NEW retains no-clobber semantics for the destination.
                    let mut destination = OpenOptions::new();
                    destination.write(true).create_new(true);
                    let mut published = self.dir.open_with(&edit.path, &destination)?;
                    if let Err(error) = published
                        .write_all(edit.replacement.as_bytes())
                        .and_then(|()| published.sync_all())
                    {
                        drop(published);
                        let _ = self.dir.remove_file(&edit.path);
                        return Err(error);
                    }
                    Ok(())
                }
                #[cfg(not(windows))]
                {
                    self.dir.hard_link(&temp, &self.dir, &edit.path)
                }
            })();
            let _ = self.dir.remove_file(&temp);
            return result;
        }
        self.validate_edit(edit)?;
        let permissions = self.dir.open(&edit.path)?.metadata()?.permissions();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let temp = format!(".lyra-apply-{}-{nonce}.tmp", std::process::id());
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut staged = self.dir.open_with(&temp, &options)?;
            staged.write_all(edit.replacement.as_bytes())?;
            staged.set_permissions(permissions)?;
            staged.sync_all()?;
            drop(staged);
            self.safe_path(&edit.path)?;
            let mut current = String::new();
            self.dir
                .open(&edit.path)?
                .take((FILE_LIMIT + 1) as u64)
                .read_to_string(&mut current)?;
            if current != edit.original {
                return Err(error("Конфликт перед заменой файла."));
            }
            self.dir.rename(&temp, &self.dir, &edit.path)
        })();
        if result.is_err() {
            let _ = self.dir.remove_file(&temp);
        }
        result
    }
}
