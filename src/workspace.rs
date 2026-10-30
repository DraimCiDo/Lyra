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

pub const FILE_LIMIT: usize = 64 * 1024;
pub const READ_BUDGET: usize = 512 * 1024;

pub struct Workspace {
    pub root: PathBuf,
    dir: Dir,
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
        if selected.len() > 32 {
            return Err(error("Не более 32 явно выбранных файлов."));
        }
        if selected.is_empty() {
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
                    if seen > 1024 || workspace.files.len() >= 128 {
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
            .take((FILE_LIMIT + 1) as u64)
            .read_to_string(&mut text)?;
        if text.len() > FILE_LIMIT {
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

    fn safe_path(&self, path: &str) -> io::Result<()> {
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

    pub fn propose(&mut self, path: &str, old: &str, new: &str) -> io::Result<Edit> {
        if old.is_empty() || old == new || new.len() > FILE_LIMIT {
            return Err(error("Некорректная точная замена."));
        }
        let original = self.read(path)?;
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

    pub fn propose_new(&mut self, path: &str, content: &str) -> io::Result<Edit> {
        self.new_path(path)?;
        if content.len() > FILE_LIMIT || content.contains('\0') {
            return Err(error("Новый файл: до 64 KiB текста без NUL."));
        }
        Ok(Edit {
            path: path.into(),
            original: String::new(),
            replacement: content.into(),
            create: true,
        })
    }

    fn new_path(&mut self, path: &str) -> io::Result<()> {
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
                self.dir.hard_link(&temp, &self.dir, &edit.path)
            })();
            let _ = self.dir.remove_file(&temp);
            return result;
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
