//! Explicit network operations for the v2 CLI.
use super::{TaskReport, TaskState, TaskStore, Verification, cli};
use crate::{
    github::{self, GithubClient},
    model_gateway::{Message, ModelBackend, Request, Role},
    provider_http::{OpenAiCompatibleBackend, parse_registry},
};
use clap::Args;
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Args)]
pub struct HttpModel {
    #[arg(long)]
    provider_config: PathBuf,
    #[arg(long)]
    provider: String,
    #[arg(long)]
    model: String,
    /// Permit sending the task and selected workspace context to this provider.
    #[arg(long, required = true)]
    allow_context_upload: bool,
    #[arg(long, default_value_t = 1024, value_parser = clap::value_parser!(u32).range(32..=8192))]
    max_tokens: u32,
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout_secs: u64,
}

pub(super) fn run_http(
    args: HttpModel,
    mut report: TaskReport,
    store: &TaskStore,
    output: &mut impl Write,
) -> io::Result<()> {
    if !args.allow_context_upload {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Provider context upload requires explicit consent",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(&args.provider_config)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let config = parse_registry(&bytes)?
        .into_iter()
        .find(|config| {
            config.descriptor.provider == args.provider && config.descriptor.model == args.model
        })
        .ok_or_else(|| io::Error::other("The configured provider and model were not found"))?;
    let mut backend = OpenAiCompatibleBackend::new(config)?
        .with_deadline(Instant::now() + Duration::from_secs(args.timeout_secs));
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancel);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed)).map_err(io::Error::other)?;
    let run = report.run(
        &cancel,
        &mut |prompt| {
            let response = backend.complete(&Request {
                messages: vec![Message {
                    role: Role::User,
                    content: prompt.to_owned(),
                }],
                max_output_tokens: args.max_tokens,
                require_tools: false,
            });
            cancellable(response.map(|response| response.text), &cancel)
        },
        &mut |report| store.save(report),
    );
    // Print the saved state before the error, as the local runner does, so callers that
    // parse stdout see a failed task too.
    cli::write_report(output, &report)?;
    run?;
    cli::exit_status(report)
}

/// A request that fails after the user cancelled is recorded as a cancellation, so the
/// saved proposals stay resumable.
fn cancellable(result: io::Result<String>, cancel: &AtomicBool) -> io::Result<String> {
    match result {
        Err(error) if cancel.load(Ordering::Relaxed) => Err(io::Error::new(
            io::ErrorKind::Interrupted,
            format!("Cancelled: {error}"),
        )),
        result => result,
    }
}

#[derive(Debug, Args)]
pub struct GitHubIssue {
    #[arg(long)]
    repo: String,
    #[arg(long)]
    number: u64,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "GITHUB_TOKEN")]
    token_env: String,
}

pub(super) fn issue(args: GitHubIssue, output: &mut impl Write) -> io::Result<()> {
    let client = GithubClient::new(github::DEFAULT_API, &args.token_env)?;
    let bundle = client.issue(&args.repo, args.number)?;
    let task = github::issue_task(&args.repo, &bundle);
    let task = super::limited(&task, 16 * 1024);
    let parent = args
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(task.as_bytes())?;
    file.persist_noclobber(&args.out)
        .map_err(|error| error.error)?;
    writeln!(output, "Wrote {}", args.out.display())
}

#[derive(Debug, Args)]
pub struct GitHubPublish {
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    repo: String,
    #[arg(long)]
    branch: String,
    #[arg(long, default_value = "main")]
    base: String,
    #[arg(long)]
    title: String,
    #[arg(long, default_value = "")]
    body: String,
    #[arg(long, default_value = "GITHUB_TOKEN")]
    token_env: String,
}

pub(super) fn publish(args: GitHubPublish, output: &mut impl Write) -> io::Result<()> {
    let store = TaskStore::open(&args.state)?;
    let report = store.load()?;
    if !matches!(report.state, TaskState::ReadyToApply | TaskState::Applied)
        || !matches!(report.verification, Verification::Passed { .. })
    {
        return Err(io::Error::other(
            "A passing check and a proposal ready for review are required",
        ));
    }
    // One read of the project: the hash proves it is the snapshot the check passed on,
    // and the same bytes are compared with the remote base.
    let (sources, hash) =
        crate::executor::candidate_sources(&report.request.repo, &report.proposals)?;
    match &report.verification {
        Verification::Passed { result }
            if result.passed() && result.source_sha256.as_deref() == Some(&hash) => {}
        _ => {
            return Err(io::Error::other(
                "The workspace no longer matches the checked candidate",
            ));
        }
    }
    let client = GithubClient::new(github::DEFAULT_API, &args.token_env)?;
    let (base, tree, symlinks) = client.base_tree(&args.repo, &args.base)?;
    remote_matches_checked(&report, &sources, &tree, &symlinks)?;
    let url = client.publish_at(
        &args.repo,
        &args.branch,
        &args.base,
        Some(&base),
        &args.title,
        &args.body,
        &report.proposals,
        true,
    )?;
    writeln!(output, "{url}")
}

/// Publishing applies the proposals to the remote base. The check validated `sources`
/// (the project plus the proposals), so every file the proposals do not touch must match
/// the remote base, and every proposed file must match its pre-proposal content there,
/// whether or not the report was already applied locally. The remote base must not hold
/// indexed files the check did not see.
fn remote_matches_checked(
    report: &TaskReport,
    sources: &std::collections::BTreeMap<String, Vec<u8>>,
    remote: &std::collections::BTreeMap<String, String>,
    symlinks: &std::collections::BTreeSet<String>,
) -> io::Result<()> {
    let proposed: std::collections::BTreeMap<_, _> = report
        .proposals
        .iter()
        .map(|edit| (edit.path.as_str(), edit))
        .collect();
    let mut differences = Vec::new();
    for (path, checked) in sources {
        let expected: &[u8] = match proposed.get(path.as_str()) {
            Some(edit) if edit.create => continue,
            Some(edit) => edit.original.as_bytes(),
            None => checked,
        };
        // A text checkout whose line endings were converted still matches the remote blob
        // in either direction (CRLF checkout of an LF blob, or LF checkout of a CRLF blob).
        let converted: Vec<String> = std::str::from_utf8(expected)
            .ok()
            .map(|text| {
                let lf = text.replace("\r\n", "\n");
                vec![lf.clone(), lf.replace('\n', "\r\n")]
            })
            .unwrap_or_default();
        let matches = remote.get(path).is_some_and(|blob| {
            *blob == github::git_blob_id(expected)
                || converted
                    .iter()
                    .any(|text| *blob == github::git_blob_id(text.as_bytes()))
        });
        if !matches {
            differences.push(path.clone());
        }
    }
    let mut workspace = crate::workspace::Workspace::for_snapshot(&report.request.repo, &[])?;
    for path in remote.keys() {
        // A real symlink is never part of a local snapshot; a placeholder file is, and was
        // compared above.
        if !sources.contains_key(path) && !symlinks.contains(path) && workspace.would_index(path)? {
            differences.push(path.clone());
        }
    }
    if differences.is_empty() {
        return Ok(());
    }
    differences.truncate(8);
    Err(io::Error::other(format!(
        "The remote base differs from the checked snapshot: {}. Update the project and run the task check again.",
        differences.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{v2::TaskRequest, workspace::Edit};
    use std::collections::BTreeMap;

    #[test]
    fn publication_requires_the_remote_base_to_match_the_checked_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("project");
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("src/lib.rs"), "pub fn add() {}\r\n").unwrap();
        fs::write(repo.join("src/util.rs"), "pub fn helper() {}\n").unwrap();
        fs::write(repo.join(".gitignore"), "build/\n").unwrap();
        fs::write(repo.join("logo.png"), [0x89, b'P', 0, 0xff]).unwrap();
        let mut report = TaskReport::new(TaskRequest {
            repo: repo.clone(),
            task: "task".into(),
            files: Vec::new(),
            check: None,
            max_calls: 4,
            reviewer: false,
        })
        .unwrap();
        report.proposals.push(Edit {
            path: "src/new.rs".into(),
            original: String::new(),
            replacement: "new".into(),
            create: true,
        });
        let mut remote = BTreeMap::new();
        for path in ["src/lib.rs", "src/util.rs"] {
            let text = fs::read_to_string(repo.join(path))
                .unwrap()
                .replace("\r\n", "\n");
            remote.insert(path.to_owned(), github::git_blob_id(text.as_bytes()));
        }
        remote.insert(".gitignore".into(), github::git_blob_id(b"build/\n"));
        remote.insert(
            "logo.png".into(),
            github::git_blob_id(&[0x89, b'P', 0, 0xff]),
        );
        remote.insert("build/out.rs".into(), github::git_blob_id(b"ignored"));
        remote.insert(".github/ci.yml".into(), "0".repeat(40));
        // A real symlink exists only remotely; a Windows checkout holds its target text as
        // a regular file. Neither is a drift.
        fs::create_dir_all(repo.join("assets")).unwrap();
        fs::write(repo.join("assets/current.txt"), "version.txt").unwrap();
        remote.insert(
            "assets/current.txt".into(),
            github::git_blob_id(b"version.txt"),
        );
        remote.insert("docs/latest".into(), github::git_blob_id(b"v2"));
        let symlinks: std::collections::BTreeSet<String> =
            ["assets/current.txt".to_owned(), "docs/latest".to_owned()].into();
        let compare = |report: &TaskReport, remote: &BTreeMap<String, String>| {
            let (sources, _) =
                crate::executor::candidate_sources(&report.request.repo, &report.proposals)?;
            remote_matches_checked(report, &sources, remote, &symlinks)
        };
        compare(&report, &remote).unwrap();

        let mut drifted = remote.clone();
        drifted.insert(
            "src/util.rs".into(),
            github::git_blob_id(b"pub fn other() {}\n"),
        );
        let error = compare(&report, &drifted).unwrap_err();
        assert!(error.to_string().contains("src/util.rs"), "{error}");

        let mut extra = remote;
        extra.insert(
            "src/extra.rs".into(),
            github::git_blob_id(b"pub fn x() {}\n"),
        );
        let error = compare(&report, &extra).unwrap_err();
        assert!(error.to_string().contains("src/extra.rs"), "{error}");

        // After Apply the workspace holds the replacement while the remote base still
        // holds the original: that is the expected state, not a drift.
        let mut applied = report;
        applied.proposals.push(Edit {
            path: "src/util.rs".into(),
            original: "pub fn helper() {}\n".into(),
            replacement: "pub fn helper() { 1; }\n".into(),
            create: false,
        });
        fs::write(repo.join("src/util.rs"), "pub fn helper() { 1; }\n").unwrap();
        fs::write(repo.join("src/new.rs"), "new").unwrap();
        let mut remote = extra;
        remote.remove("src/extra.rs");
        compare(&applied, &remote).unwrap();
        // An LF checkout of a CRLF blob is the same file.
        remote.insert(
            "src/lib.rs".into(),
            github::git_blob_id(b"pub fn add() {}\r\n"),
        );
        fs::write(repo.join("src/lib.rs"), "pub fn add() {}\n").unwrap();
        compare(&applied, &remote).unwrap();
        // The proposed file must still equal its original on the remote base.
        remote.insert(
            "src/util.rs".into(),
            github::git_blob_id(b"pub fn other() {}\n"),
        );
        let error = compare(&applied, &remote).unwrap_err();
        assert!(error.to_string().contains("src/util.rs"), "{error}");
    }

    #[test]
    fn a_failed_request_after_cancellation_is_reported_as_cancelled() {
        let cancel = AtomicBool::new(false);
        let timeout = || Err(io::Error::new(io::ErrorKind::TimedOut, "deadline"));
        assert_eq!(
            cancellable(timeout(), &cancel).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            cancellable(timeout(), &cancel).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(cancellable(Ok("ok".into()), &cancel).unwrap(), "ok");
    }
}
