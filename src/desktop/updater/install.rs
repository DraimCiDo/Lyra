//! Separate helper owns replacement; no shell, registry writes, elevation or model access.
use super::package::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
const HELPER: &str = "lyra-update-helper.exe";
const LOCK: &str = ".lyra-update.lock";
const OWNED: &[u8] = b"Lyra updater 1\n";
fn files() -> impl Iterator<Item = &'static str> {
    MANAGED.into_iter().chain([MANIFEST])
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub root: PathBuf,
    pub stage: PathBuf,
    pub helper_dir: PathBuf,
    pub parent_pid: u32,
    pub parent_started: u64,
    pub parent_exe: PathBuf,
    pub helper_sha256: String,
    pub old_manifest_sha256: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Receipt {
    pub success: bool,
    pub version: String,
    pub message: String,
}
#[derive(Serialize, Deserialize)]
struct Health {
    version: String,
    root: PathBuf,
}
struct Lock {
    path: PathBuf,
    armed: bool,
}
impl Drop for Lock {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut f = File::options().create_new(true).write(true).open(path)?;
    serde_json::to_writer(&mut f, value).map_err(error)?;
    f.sync_all()
}
fn directory(path: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(error("Unexpected update directory"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(error("Update directory is a reparse point"));
        }
    }
    Ok(())
}
pub fn process_identity(pid: u32) -> io::Result<Option<(PathBuf, u64)>> {
    let mut s = System::new().map_err(error)?;
    let id = Pid::from_u32(pid);
    s.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    match s.process(id) {
        Some(p) => Ok(Some((
            p.exe()
                .ok_or_else(|| error("Cannot inspect update parent"))?
                .canonicalize()?,
            p.start_time(),
        ))),
        None => Ok(None),
    }
}
fn wait_parent(p: &Plan) -> io::Result<()> {
    let start = Instant::now();
    loop {
        match process_identity(p.parent_pid)? {
            None => return Ok(()),
            Some((exe, time)) if exe == p.parent_exe && time == p.parent_started => {}
            _ => return Err(error("Update parent identity changed")),
        }
        if start.elapsed() > Duration::from_secs(60) {
            return Err(error("Application did not exit within 60 seconds"));
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn command(path: &Path) -> Command {
    let mut c = Command::new(path);
    c.current_dir(path.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c
}
pub fn probe(path: &Path, version: &semver::Version) -> io::Result<()> {
    let mut cmd = command(path);
    if path.file_name().is_some_and(|n| n == "lyra.exe") {
        cmd.arg("--version");
    } else {
        cmd.args(["--worker", "--version"]);
    }
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error("Missing probe pipe"))?;
    let reader = thread::Builder::new()
        .name("update-probe".into())
        .spawn(move || {
            let mut b = Vec::new();
            stdout.take(4097).read_to_end(&mut b).map(|_| b)
        });
    let reader = match reader {
        Ok(r) => r,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e);
            }
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            break Err(error("Version probe timed out"));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let b = reader.join().map_err(|_| error("Probe reader failed"))??;
    let expected = format!("lyra {version}");
    if !status?.success() || b.len() > 4096 || String::from_utf8_lossy(&b).trim() != expected {
        return Err(error("Executable version does not match update"));
    }
    Ok(())
}
fn safe_copy(from: &Path, to: &Path) -> io::Result<()> {
    plain(from)?;
    if to.exists() {
        return Err(error("Update staging file already exists"));
    }
    fs::copy(from, to)?;
    File::options().write(true).open(to)?.sync_all()?;
    Ok(())
}
fn load_plan(path: &Path) -> io::Result<Plan> {
    serde_json::from_slice(&small_file(path, 65536)?).map_err(error)
}
pub fn prepare_for_current(package: super::package::Package, cache: &Path) -> io::Result<()> {
    if !super::Controller::supported() {
        return Err(error("In-place updates require the Windows x64 CPU build"));
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    let root = exe
        .parent()
        .ok_or_else(|| error("No application directory"))?;
    if exe != root.join("lyra-desktop.exe").canonicalize()?
        || Manifest::read(root)?.version.to_string() != env!("CARGO_PKG_VERSION")
    {
        return Err(error(
            "Unpack the complete portable package before updating",
        ));
    }
    prepare(package, root, std::process::id(), &exe, cache, false)
}
/// Explicit parameters support deterministic native installer tests. UI calls prepare_for_current.
pub fn prepare(
    package: super::package::Package,
    root: &Path,
    parent: u32,
    helper_source: &Path,
    cache: &Path,
    quiet: bool,
) -> io::Result<()> {
    let root = root.canonicalize()?;
    directory(&root)?;
    let old = Manifest::read(&root)?;
    old.verify(&root)?;
    if package.manifest.version.cmp_precedence(&old.version) != std::cmp::Ordering::Greater {
        return Err(error("Update must be newer than installed version"));
    }
    package.manifest.verify(&package.payload())?;
    probe(
        &package.payload().join("lyra-desktop.exe"),
        &package.manifest.version,
    )?;
    probe(
        &package.payload().join("lyra.exe"),
        &package.manifest.version,
    )?;
    let mut lock_file = File::options()
        .create_new(true)
        .write(true)
        .open(root.join(LOCK))?;
    let mut lock = Lock {
        path: root.join(LOCK),
        armed: true,
    };
    let stage = tempfile::Builder::new()
        .prefix(".lyra-update-stage-")
        .tempdir_in(&root)?;
    fs::create_dir(stage.path().join("payload"))?;
    fs::create_dir(stage.path().join("backup"))?;
    for name in files() {
        safe_copy(
            &package.payload().join(name),
            &stage.path().join("payload").join(name),
        )?;
    }
    fs::create_dir_all(cache)?;
    let cache = cache.canonicalize()?;
    directory(&cache)?;
    let helper = tempfile::Builder::new()
        .prefix("helper-")
        .tempdir_in(&cache)?;
    fs::write(helper.path().join(".owned"), OWNED)?;
    safe_copy(helper_source, &helper.path().join(HELPER))?;
    let (parent_exe, parent_started) = process_identity(parent)?
        .ok_or_else(|| error("Parent already exited before update preparation"))?;
    if parent_exe != root.join("lyra-desktop.exe").canonicalize()? {
        return Err(error("Parent is not the installed application"));
    }
    let plan = Plan {
        schema_version: 1,
        root: root.clone(),
        stage: stage.path().canonicalize()?,
        helper_dir: helper.path().canonicalize()?,
        parent_pid: parent,
        parent_started,
        parent_exe,
        helper_sha256: hash_file(&helper.path().join(HELPER))?,
        old_manifest_sha256: hash_file(&root.join(MANIFEST))?,
    };
    lock_file.write_all(plan.stage.as_os_str().to_string_lossy().as_bytes())?;
    lock_file.sync_all()?;
    drop(lock_file);
    write_json(&helper.path().join("plan.json"), &plan)?;
    let mut cmd = command(&helper.path().join(HELPER));
    cmd.arg("--update-helper")
        .arg(helper.path().join("plan.json"));
    if quiet {
        cmd.arg("--quiet");
    }
    let mut child = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    let start = Instant::now();
    loop {
        if helper.path().join("ready").is_file() {
            break;
        }
        let status = match child.try_wait() {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        if status.is_some() {
            return Err(error(
                "Update helper could not start; application files unchanged",
            ));
        }
        if start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error("Update helper handshake timed out"));
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = helper.keep();
    let _ = stage.keep();
    lock.armed = false;
    Ok(())
}
fn validate_plan(p: &Plan, plan_path: &Path) -> io::Result<()> {
    if p.schema_version != 1
        || p.root != p.root.canonicalize()?
        || p.stage != p.stage.canonicalize()?
        || p.helper_dir != p.helper_dir.canonicalize()?
    {
        return Err(error("Invalid update paths"));
    }
    directory(&p.stage.join("payload"))?;
    directory(&p.stage.join("backup"))?;
    for dir in [&p.root, &p.stage, &p.helper_dir] {
        directory(dir)?;
    }
    if p.stage.parent() != Some(p.root.as_path())
        || !p
            .stage
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".lyra-update-stage-"))
        || plan_path.canonicalize()? != p.helper_dir.join("plan.json")
        || small_file(&p.helper_dir.join(".owned"), 32)? != OWNED
    {
        return Err(error("Invalid update staging directory"));
    }
    if p.parent_exe != p.root.join("lyra-desktop.exe").canonicalize()?
        || hash_file(&p.root.join(MANIFEST))? != p.old_manifest_sha256
    {
        return Err(error("Installed application changed during preparation"));
    }
    if std::env::current_exe()?.canonicalize()? != p.helper_dir.join(HELPER).canonicalize()?
        || hash_file(&p.helper_dir.join(HELPER))? != p.helper_sha256
    {
        return Err(error("Wrong update helper"));
    }
    if small_file(&p.root.join(LOCK), 32768)? != p.stage.as_os_str().to_string_lossy().as_bytes() {
        return Err(error("Update lock does not match staging directory"));
    }
    let old = Manifest::read(&p.root)?;
    old.verify(&p.root)?;
    let new = Manifest::read(&p.stage.join("payload"))?;
    new.verify(&p.stage.join("payload"))?;
    if new.version.cmp_precedence(&old.version) != std::cmp::Ordering::Greater {
        return Err(error("Downgrade is not an update"));
    }
    Ok(())
}
fn restore(p: &Plan, replaced: &[&str]) -> io::Result<()> {
    let mut errors = Vec::new();
    for name in replaced.iter().rev() {
        let from = p.stage.join("backup").join(name);
        let to = p.root.join(name);
        let mut last = None;
        for _ in 0..10 {
            match fs::copy(
                &from,
                p.stage.join("payload").join(format!("restore-{name}")),
            ) {
                Ok(_) => {
                    match fs::rename(p.stage.join("payload").join(format!("restore-{name}")), &to) {
                        Ok(()) => {
                            last = None;
                            break;
                        }
                        Err(e) => last = Some(e),
                    }
                }
                Err(e) => last = Some(e),
            }
            thread::sleep(Duration::from_millis(100));
        }
        if let Some(e) = last {
            errors.push(format!("{name}: {e}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(error(format!(
            "Rollback incomplete; backups: {}: {}",
            p.stage.join("backup").display(),
            errors.join("; ")
        )))
    }
}
fn wait_health(child: &mut Child, p: &Plan, new: &Manifest) -> io::Result<()> {
    let start = Instant::now();
    loop {
        if let Ok(bytes) = small_file(&p.helper_dir.join("healthy.json"), 4096)
            && let Ok(h) = serde_json::from_slice::<Health>(&bytes)
            && h.version == new.version.to_string()
            && h.root == p.root
        {
            return Ok(());
        }
        if child.try_wait()?.is_some() {
            return Err(error("New application exited before its first UI frame"));
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error("New application did not confirm a healthy startup"));
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn transaction(p: &Plan) -> io::Result<()> {
    let old = Manifest::read(&p.root)?;
    old.verify(&p.root)?;
    let payload = p.stage.join("payload");
    let new = Manifest::read(&payload)?;
    new.verify(&payload)?;
    for name in files() {
        safe_copy(&p.root.join(name), &p.stage.join("backup").join(name))?;
    }
    old.verify(&p.stage.join("backup"))?;
    let mut replaced = Vec::new();
    let result = (|| {
        for name in files() {
            if name == MANIFEST {
                if hash_file(&p.root.join(name))? != p.old_manifest_sha256 {
                    return Err(error("Manifest changed while installing"));
                }
            } else if !hash_file(&p.root.join(name))?.eq_ignore_ascii_case(&old.files[name].sha256)
            {
                return Err(error("Program files changed while installing"));
            }
            fs::rename(payload.join(name), p.root.join(name))?;
            replaced.push(name);
        }
        probe(&p.root.join("lyra-desktop.exe"), &new.version)?;
        let mut child = command(&p.root.join("lyra-desktop.exe"))
            .arg("--updated")
            .arg(&p.helper_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let healthy = wait_health(&mut child, p, &new);
        if healthy.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        healthy
    })();
    if let Err(e) = result {
        restore(p, &replaced)?;
        return Err(e);
    }
    Ok(())
}
fn save_receipt(p: &Plan, success: bool, message: String) {
    let version = Manifest::read(&p.root)
        .map(|m| m.version.to_string())
        .unwrap_or_default();
    let receipt = Receipt {
        success,
        version,
        message,
    };
    let _ = write_json(&p.helper_dir.join("result.json"), &receipt);
    if let Some(cache) = p.helper_dir.parent() {
        let path = cache.join("last-result.json");
        if let Ok(mut f) = tempfile::NamedTempFile::new_in(cache) {
            let _ = serde_json::to_writer(&mut f, &receipt);
            let _ = f.flush();
            let _ = f.persist(path);
        }
    }
}
pub fn helper(plan_path: &Path) -> io::Result<()> {
    let p = load_plan(plan_path)?;
    validate_plan(&p, plan_path)?;
    write_json(&p.helper_dir.join("pid.json"), &std::process::id())?;
    fs::write(p.helper_dir.join("ready"), b"ready")?;
    let exited = wait_parent(&p);
    let parent_gone = exited.is_ok();
    let result = exited.and_then(|_| transaction(&p));
    if let Err(e) = &result {
        save_receipt(&p, false, e.to_string());
    } else {
        save_receipt(&p, true, "Updated successfully".into());
    }
    // Release only the lock belonging to this installer.
    if fs::read(p.root.join(LOCK)).ok().as_deref()
        == Some(p.stage.as_os_str().to_string_lossy().as_bytes())
    {
        let _ = fs::remove_file(p.root.join(LOCK));
    }
    let safe = Manifest::read(&p.root)
        .and_then(|m| m.verify(&p.root))
        .is_ok();
    if result.is_err() && safe && parent_gone {
        let _ = command(&p.root.join("lyra-desktop.exe"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    if safe {
        let _ = fs::remove_dir_all(&p.stage);
    }
    result
}
pub fn acknowledge(cache: &Path, helper_dir: &Path) -> io::Result<()> {
    let cache = cache.canonicalize()?;
    let dir = helper_dir.canonicalize()?;
    if dir.parent() != Some(cache.as_path())
        || !dir
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("helper-"))
        || small_file(&dir.join(".owned"), 32)? != OWNED
    {
        return Err(error("Invalid startup acknowledgement directory"));
    }
    let p = load_plan(&dir.join("plan.json"))?;
    let exe = std::env::current_exe()?.canonicalize()?;
    if p.root.join("lyra-desktop.exe").canonicalize()? != exe {
        return Err(error("Startup acknowledgement for another application"));
    }
    write_json(
        &dir.join("healthy.json"),
        &Health {
            version: env!("CARGO_PKG_VERSION").into(),
            root: p.root,
        },
    )
}
pub fn cleanup(cache: &Path) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.take(32).flatten() {
        let dir = entry.path();
        if directory(&dir).is_err()
            || !entry.file_name().to_string_lossy().starts_with("helper-")
            || small_file(&dir.join(".owned"), 32).ok().as_deref() != Some(OWNED)
        {
            continue;
        }
        if let Ok(p) = load_plan(&dir.join("plan.json"))
            && p.stage.exists()
        {
            continue;
        }
        if let Ok(b) = small_file(&dir.join("pid.json"), 128)
            && let Ok(pid) = serde_json::from_slice::<u32>(&b)
            && !matches!(process_identity(pid), Ok(None))
        {
            continue;
        }
        if fs::remove_file(dir.join(HELPER)).is_err() {
            continue;
        }
        for name in [
            ".owned",
            "plan.json",
            "pid.json",
            "ready",
            "result.json",
            "healthy.json",
        ] {
            let _ = fs::remove_file(dir.join(name));
        }
        let _ = fs::remove_dir(dir);
    }
}
pub fn read_receipt(cache: &Path) -> Option<Receipt> {
    serde_json::from_slice(&small_file(&cache.join("last-result.json"), 8192).ok()?).ok()
}
