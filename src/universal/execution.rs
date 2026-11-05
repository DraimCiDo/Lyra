//! Model scripts run only in explicitly granted digest-pinned containers.
use super::{
    artifacts::{self, FileProposal, Input, Store},
    environment::Registry,
};
use crate::{
    executor,
    workspace::{Edit, Workspace},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{self, Read},
    path::Path,
    process::Command,
    sync::atomic::AtomicBool,
    time::Duration,
};
pub struct ExecutionSession {
    pub registry: Registry,
    pub allowed: Vec<String>,
    pub store: Store,
    pub deadline: Option<std::time::Instant>,
    pub staged_files: Vec<FileProposal>,
    pub staged_store: Option<std::path::PathBuf>,
}
impl ExecutionSession {
    pub fn new(registry: Registry, allowed: Vec<String>, store: &Path) -> io::Result<Self> {
        registry.validate()?;
        for id in &allowed {
            registry.get(id)?;
        }
        if allowed.is_empty() {
            return Err(io::Error::other("Explicit --allow-environment required"));
        }
        Ok(Self {
            registry,
            allowed,
            store: Store::open(store)?,
            deadline: None,
            staged_files: Vec::new(),
            staged_store: None,
        })
    }
    pub fn catalog(&self) -> Value {
        json!(
            self.registry
                .environments
                .iter()
                .filter(|e| self.allowed.contains(&e.id))
                .collect::<Vec<_>>()
        )
    }
    #[allow(clippy::too_many_arguments)] // Explicit sandbox inputs and cancellation boundary.
    pub fn run(
        &mut self,
        workspace: &Workspace,
        pending: &[Edit],
        environment: &str,
        script: &str,
        inputs: &[String],
        outputs: &[String],
        cancel: &AtomicBool,
    ) -> io::Result<(Value, Vec<FileProposal>)> {
        if !self.allowed.iter().any(|s| s == environment) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Environment was not granted for this task",
            ));
        }
        let mut env = self.registry.get(environment)?.clone();
        if let Some(deadline) = self.deadline {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Task deadline reached",
                ));
            }
            env.timeout_secs = env.timeout_secs.min(left.as_secs().max(1));
        }
        if script.len() > 65536
            || script.contains('\0')
            || inputs.len() > 64
            || outputs.is_empty()
            || outputs.len() > 32
        {
            return Err(io::Error::other("Script/input/output limits exceeded"));
        }
        let mut names = BTreeSet::new();
        for p in outputs {
            crate::project::validate_path(p, false)?;
            if !names.insert(p.to_lowercase()) {
                return Err(io::Error::other("Duplicate output"));
            }
        }
        for path in outputs {
            if workspace.dir.try_exists(path)?
                && !workspace.files.contains(path)
                && !self.staged_files.iter().any(|p| &p.path == path)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Existing output is outside selected context",
                ));
            }
        }
        executor::preflight(cancel)?;
        executor::preflight_image(&env.image, cancel)?;
        let temp = crate::workspace::private_tempdir("lyra-run-")?;
        let source = temp.path().join("source");
        std::fs::create_dir(&source)?;
        let mut fingerprints = Vec::new();
        let mut total = 0;
        for path in inputs {
            crate::project::validate_path(path, false)?;
            let bytes = if let Some(staged) = self.staged_files.iter().find(|f| &f.path == path) {
                let store = Store::open(
                    self.staged_store
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Pending artifact store missing"))?,
                )?;
                fingerprints.extend(staged.inputs.clone());
                if let Some(blob) = &staged.original {
                    fingerprints.push(Input {
                        path: path.clone(),
                        blob: blob.clone(),
                    });
                }
                store.get(&staged.replacement)?
            } else if let Some(edit) = pending.iter().find(|p| &p.path == path) {
                if !edit.create {
                    let original = artifacts::read(workspace, path)?;
                    if original != edit.original.as_bytes() {
                        return Err(io::Error::other("Pending input source changed"));
                    }
                    fingerprints.push(Input {
                        path: path.clone(),
                        blob: self.store.put(&original)?,
                    });
                }
                edit.replacement.as_bytes().to_vec()
            } else {
                if !workspace.files.contains(path) {
                    return Err(io::Error::other("Input is outside selected context"));
                }
                let original = artifacts::read(workspace, path)?;
                fingerprints.push(Input {
                    path: path.clone(),
                    blob: self.store.put(&original)?,
                });
                original
            };
            total += bytes.len();
            if total > artifacts::TOTAL_LIMIT {
                return Err(io::Error::other("Inputs exceed 256 MiB"));
            }
            let destination = source.join(path);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(destination, bytes)?;
        }
        let script_name = format!("lyra_task.{}", env.script_extension);
        if inputs.iter().any(|p| p == &script_name) {
            return Err(io::Error::other("Input collides with internal script"));
        }
        std::fs::write(source.join(&script_name), script)?;
        make_readable(&source)?;
        let mut request = env.request();
        // Runtime argv are positional parameters to a host-owned shell wrapper.
        // Script stdout becomes bounded diagnostics; only the tar stream is binary stdout.
        let program = request.program.clone();
        let mut args = request.args.clone();
        args.push(format!("/source/{script_name}"));
        request.program = "/bin/sh".into();
        request.args=vec!["-c".into(),"\"$@\" >&2; status=$?; [ \"$status\" = 0 ] || exit \"$status\"; exec tar -C /output -cf - .".into(),"lyra-run".into(),program];
        request.args.extend(args);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let name = format!("lyra-run-{}-{nonce}", std::process::id());
        let command = request.command(&source, &name)?;
        let mut argv: Vec<std::ffi::OsString> =
            command.get_args().map(|s| s.to_os_string()).collect();
        // Insert before the image/entrypoint arguments, never into model argv.
        let pos = argv
            .iter()
            .position(|s| s == "--entrypoint")
            .ok_or_else(|| io::Error::other("Missing entrypoint"))?;
        argv.insert(
            pos,
            "--tmpfs=/output:rw,noexec,nosuid,nodev,size=256m,mode=1777".into(),
        );
        let mut run = Command::new("docker");
        run.args(argv);
        let result = executor::capture_binary(
            run,
            Duration::from_secs(env.timeout_secs),
            cancel,
            artifacts::TOTAL_LIMIT + 1024 * 1024,
        );
        let mut cleanup = Command::new("docker");
        cleanup.args(["rm", "--force", "--volumes", &name]);
        let clean = executor::capture(cleanup, Duration::from_secs(10), &AtomicBool::new(false));
        let (mut result, archive) = result?;
        result.cleanup_confirmed = clean.is_ok_and(|r| r.exit_code == Some(0) && !r.timed_out);
        if !result.passed() || result.output_truncated {
            return Ok((
                json!({"environment":env.id,"image":env.image,"script_sha256":crate::retrieval::sha256(script.as_bytes()),"execution":result,"outputs":[],"artifact_checks_passed":false}),
                vec![],
            ));
        }
        let extracted = extract(&archive, outputs)?;
        let mut proposals = Vec::new();
        for (path, bytes) in extracted {
            let original = match artifacts::read(workspace, &path) {
                Ok(b) => Some(self.store.put(&b)?),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    let mut w = Workspace::open(&workspace.root, &[])?;
                    w.new_path(&path)?;
                    None
                }
                Err(e) => return Err(e),
            };
            proposals.push(FileProposal {
                path,
                original,
                replacement: self.store.put(&bytes)?,
                inputs: fingerprints.clone(),
            });
        }
        let descriptors:Vec<_>=proposals.iter().map(|p|json!({"path":p.path,"replacement":p.replacement,"create":p.original.is_none()})).collect();
        Ok((
            json!({"environment":env.id,"image":env.image,"script_sha256":crate::retrieval::sha256(script.as_bytes()),"execution":result,"outputs":descriptors,"artifact_checks_passed":false,"visual_review":"pending","note":"Execution and SHA256 do not establish semantic or visual correctness. Review outputs before applying."}),
            proposals,
        ))
    }
}
pub fn extract(archive: &[u8], outputs: &[String]) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut reader = tar::Archive::new(archive);
    let mut found = std::collections::BTreeMap::new();
    let mut total = 0;
    let mut count = 0;
    for entry in reader.entries()? {
        let e = entry?;
        count += 1;
        if count > 4096 {
            return Err(io::Error::other("Too many output entries"));
        }
        let path = e
            .path()?
            .to_str()
            .ok_or_else(|| io::Error::other("Non-UTF8 output path"))?
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_owned();
        if path.is_empty() && e.header().entry_type().is_dir() {
            continue;
        }
        crate::project::validate_path(&path, false)?;
        if e.header().entry_type().is_dir() {
            continue;
        }
        if !e.header().entry_type().is_file() {
            return Err(io::Error::other("Output links/devices are forbidden"));
        }
        if !outputs.contains(&path) {
            return Err(io::Error::other(format!("Undeclared output: {path}")));
        }
        let size = e.size() as usize;
        total += size;
        if size > artifacts::FILE_LIMIT || total > artifacts::TOTAL_LIMIT {
            return Err(io::Error::other("Output limit exceeded"));
        }
        let mut bytes = Vec::new();
        e.take(artifacts::FILE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != size || found.insert(path, bytes).is_some() {
            return Err(io::Error::other("Duplicate or incomplete output"));
        }
    }
    if found.len() != outputs.len() {
        return Err(io::Error::other("Requested output missing"));
    }
    Ok(found.into_iter().collect())
}
fn make_readable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
        for e in std::fs::read_dir(path)? {
            let e = e?;
            if e.file_type()?.is_dir() {
                make_readable(&e.path())?;
            } else {
                std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o644))?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub struct ExecutionModel<'a, M: crate::agent::Model> {
    pub inner: &'a mut M,
    pub execution: Option<ExecutionSession>,
}
impl<M: crate::agent::Model> crate::agent::Model for ExecutionModel<'_, M> {
    fn run_script(
        &mut self,
        w: &Workspace,
        p: &[Edit],
        e: &str,
        s: &str,
        i: &[String],
        o: &[String],
    ) -> io::Result<(Value, Vec<FileProposal>)> {
        self.execution
            .as_mut()
            .ok_or_else(|| io::Error::other("No environment"))?
            .run(w, p, e, s, i, o, self.inner.cancellation())
    }
    fn cancellation(&self) -> &AtomicBool {
        self.inner.cancellation()
    }
    fn input_limit(&self) -> usize {
        self.inner.input_limit()
    }
    fn count_prompt(&self, p: &str) -> io::Result<usize> {
        self.inner.count_prompt(p)
    }
    fn accounting(&self) -> &'static str {
        self.inner.accounting()
    }
    fn respond(&mut self, p: &str) -> io::Result<String> {
        self.inner.respond(p)
    }
    fn execution(&mut self) -> Option<&mut ExecutionSession> {
        self.execution.as_mut()
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecord {
    pub environment: String,
    pub image: String,
    pub script_sha256: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub cleanup_confirmed: bool,
    pub outputs: Vec<(String, String)>,
}
pub fn record(report: &mut crate::agent::Report, value: &Value) {
    let Some(environment) = value["environment"].as_str() else {
        return;
    };
    let result = &value["execution"];
    report.schema_version = 5;
    report.executions.push(ExecutionRecord {
        environment: environment.into(),
        image: value["image"].as_str().unwrap_or_default().into(),
        script_sha256: value["script_sha256"].as_str().unwrap_or_default().into(),
        exit_code: result["exit_code"]
            .as_i64()
            .and_then(|x| i32::try_from(x).ok()),
        timed_out: result["timed_out"] == true,
        cancelled: result["cancelled"] == true,
        cleanup_confirmed: result["cleanup_confirmed"] == true,
        outputs: value["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| {
                Some((
                    x["path"].as_str()?.into(),
                    x["replacement"]["sha256"].as_str()?.into(),
                ))
            })
            .take(32)
            .collect(),
    });
    if report.executions.len() > 8 {
        report.executions.remove(0);
    }
}
