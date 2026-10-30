use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
};
use tempfile::TempDir;

pub const ASSET: &str = "lyra-windows-x86_64.zip";
pub const SUMS: &str = "lyra-windows-x86_64.zip.sha256";
pub const MANIFEST: &str = "lyra-update.json";
pub const MANAGED: [&str; 4] = [
    "lyra-desktop.exe",
    "lyra.exe",
    "THIRD_PARTY_NOTICES.txt",
    "VERSION",
];
pub const ARCHIVE_LIMIT: u64 = 128 * 1024 * 1024;
pub const EXPANDED_LIMIT: u64 = 256 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileInfo {
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub version: Version,
    pub target: String,
    pub flavor: String,
    pub files: BTreeMap<String, FileInfo>,
}
pub struct Package {
    pub directory: TempDir,
    pub manifest: Manifest,
}
impl Package {
    pub fn payload(&self) -> std::path::PathBuf {
        self.directory.path().join("payload")
    }
}
pub fn error(text: impl std::fmt::Display) -> io::Error {
    io::Error::other(text.to_string())
}
pub fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Update cancelled",
        ))
    } else {
        Ok(())
    }
}
pub fn plain(path: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(error("Update files must be regular files"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(error("Update file is a reparse point"));
        }
    }
    Ok(())
}
pub fn hash_file(path: &Path) -> io::Result<String> {
    plain(path)?;
    let mut f = File::open(path)?;
    let mut hash = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        hash.update(&b[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
pub fn small_file(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    plain(path)?;
    let mut b = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut b)?;
    if b.len() as u64 > limit {
        return Err(error("Update metadata exceeds its size limit"));
    }
    Ok(b)
}
impl Manifest {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1
            || self.target != "x86_64-pc-windows-msvc"
            || self.flavor != "cpu"
            || self.version.to_string().len() > 128
            || self.files.len() != MANAGED.len()
        {
            return Err(error("Unsupported update manifest"));
        }
        let mut total = 0u64;
        for name in MANAGED {
            let f = self
                .files
                .get(name)
                .ok_or_else(|| error("Incomplete update manifest"))?;
            let max = if name == "VERSION" {
                128
            } else if name.ends_with(".txt") {
                8 * 1024 * 1024
            } else {
                ARCHIVE_LIMIT
            };
            if f.size == 0
                || f.size > max
                || f.sha256.len() != 64
                || !f.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err(error("Invalid managed-file size or hash"));
            }
            total += f.size;
        }
        if total > EXPANDED_LIMIT {
            return Err(error("Update package is too large"));
        }
        Ok(())
    }
    pub fn read(root: &Path) -> io::Result<Self> {
        let m: Self =
            serde_json::from_slice(&small_file(&root.join(MANIFEST), 65536)?).map_err(error)?;
        m.validate()?;
        Ok(m)
    }
    pub fn verify(&self, root: &Path) -> io::Result<()> {
        self.validate()?;
        for (name, f) in &self.files {
            let path = root.join(name);
            plain(&path)?;
            if fs::metadata(&path)?.len() != f.size
                || !hash_file(&path)?.eq_ignore_ascii_case(&f.sha256)
            {
                return Err(error(format!("Managed file changed: {name}")));
            }
        }
        if String::from_utf8(small_file(&root.join("VERSION"), 128)?)
            .map_err(error)?
            .trim()
            != self.version.to_string()
        {
            return Err(error("VERSION does not match manifest"));
        }
        Ok(())
    }
}
pub fn expected_hash(sums: &[u8]) -> io::Result<String> {
    let text = std::str::from_utf8(sums).map_err(error)?;
    let mut matches = Vec::new();
    for line in text.lines() {
        let p: Vec<_> = line.split_whitespace().collect();
        if p.len() == 2 && p[1].trim_start_matches('*') == ASSET {
            matches.push(p[0]);
        }
    }
    if matches.len() != 1
        || matches[0].len() != 64
        || !matches[0].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(error("Missing or ambiguous archive checksum"));
    }
    Ok(matches[0].to_ascii_lowercase())
}
fn safe_name(name: &str) -> bool {
    let n = name.trim_end_matches('/');
    !n.is_empty()
        && !n.contains(['\\', ':'])
        && !n.chars().any(char::is_control)
        && !Path::new(n).is_absolute()
        && Path::new(n)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
pub fn unpack(
    directory: TempDir,
    expected: &Version,
    checksum: &str,
    cancel: &AtomicBool,
) -> io::Result<Package> {
    let zip = directory.path().join("package.zip");
    if fs::metadata(&zip)?.len() > ARCHIVE_LIMIT || hash_file(&zip)? != checksum {
        return Err(error("Archive checksum mismatch"));
    }
    cancelled(cancel)?;
    let mut archive = zip::ZipArchive::new(File::open(&zip)?).map_err(error)?;
    if archive.len() > 128 {
        return Err(error("Too many ZIP entries"));
    }
    let payload = directory.path().join("payload");
    fs::create_dir(&payload)?;
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        cancelled(cancel)?;
        let mut entry = archive.by_index(i).map_err(error)?;
        let name = entry.name().map_err(error)?.into_owned();
        if !safe_name(&name)
            || !seen.insert(name.to_lowercase())
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err(error("Unsafe ZIP entry"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| error("ZIP size overflow"))?;
        if total > EXPANDED_LIMIT {
            return Err(error("Expanded ZIP is too large"));
        }
        if name != MANIFEST && !MANAGED.contains(&name.as_str()) {
            continue;
        }
        if entry.is_dir() {
            return Err(error("Managed file is a directory"));
        }
        let limit = if name == MANIFEST {
            65536
        } else if name == "VERSION" {
            128
        } else if name.ends_with(".txt") {
            8 * 1024 * 1024
        } else {
            ARCHIVE_LIMIT
        };
        if entry.size() > limit {
            return Err(error("Managed ZIP entry is too large"));
        }
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(payload.join(&name))?;
        let mut bytes = 0u64;
        let mut buffer = [0; 65536];
        loop {
            cancelled(cancel)?;
            let n = entry.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > limit {
                return Err(error("ZIP entry exceeds its limit"));
            }
            output.write_all(&buffer[..n])?;
        }
        output.sync_all()?;
        #[cfg(unix)]
        if name.ends_with(".exe") {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(payload.join(&name), fs::Permissions::from_mode(0o755))?;
        }
    }
    let manifest = Manifest::read(&payload)?;
    if &manifest.version != expected {
        return Err(error("Package version does not match release"));
    }
    manifest.verify(&payload)?;
    Ok(Package {
        directory,
        manifest,
    })
}
