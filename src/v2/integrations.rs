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
    report.run(
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
    )?;
    cli::write_report(output, &report)?;
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
    report.check_candidate()?;
    let client = GithubClient::new(github::DEFAULT_API, &args.token_env)?;
    let (base, tree) = client.base_tree(&args.repo, &args.base)?;
    remote_matches_checked(&report, &tree)?;
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

/// The check ran on the local snapshot plus the proposals. Publishing applies the same
/// proposals to the remote base, so every file the check saw must be identical there,
/// and the base must not hold indexed files the check did not see.
fn remote_matches_checked(
    report: &TaskReport,
    remote: &std::collections::BTreeMap<String, String>,
) -> io::Result<()> {
    let mut workspace = crate::workspace::Workspace::for_snapshot(&report.request.repo, &[])?;
    let created: std::collections::BTreeSet<_> = report
        .proposals
        .iter()
        .filter(|edit| edit.create)
        .map(|edit| edit.path.as_str())
        .collect();
    let mut differences = Vec::new();
    for path in workspace.files.clone() {
        if created.contains(path.as_str()) {
            continue;
        }
        let local = workspace.read_snapshot_bytes(&path)?;
        // A text checkout with CRLF conversion still matches a blob stored with LF.
        let lf = std::str::from_utf8(&local)
            .ok()
            .filter(|text| text.contains("\r\n"))
            .map(|text| text.replace("\r\n", "\n"));
        let matches = remote.get(&path).is_some_and(|blob| {
            *blob == github::git_blob_id(&local)
                || lf
                    .as_ref()
                    .is_some_and(|text| *blob == github::git_blob_id(text.as_bytes()))
        });
        if !matches {
            differences.push(path);
        }
    }
    for path in remote.keys() {
        if !workspace.files.contains(path) && workspace.would_index(path)? {
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
        remote_matches_checked(&report, &remote).unwrap();

        let mut drifted = remote.clone();
        drifted.insert(
            "src/util.rs".into(),
            github::git_blob_id(b"pub fn other() {}\n"),
        );
        let error = remote_matches_checked(&report, &drifted).unwrap_err();
        assert!(error.to_string().contains("src/util.rs"), "{error}");

        let mut extra = remote;
        extra.insert(
            "src/extra.rs".into(),
            github::git_blob_id(b"pub fn x() {}\n"),
        );
        let error = remote_matches_checked(&report, &extra).unwrap_err();
        assert!(error.to_string().contains("src/extra.rs"), "{error}");
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
