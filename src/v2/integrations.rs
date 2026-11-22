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
            })?;
            Ok(response.text)
        },
        &mut |report| store.save(report),
    )?;
    cli::write_report(output, &report)?;
    cli::exit_status(report)
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
    let url = client.publish(
        &args.repo,
        &args.branch,
        &args.base,
        &args.title,
        &args.body,
        &report.proposals,
        true,
    )?;
    writeln!(output, "{url}")
}
