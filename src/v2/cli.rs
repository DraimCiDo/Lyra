use super::{TaskReport, TaskRequest, TaskState, TaskStore};
use crate::{executor, inference};
use clap::{Args, Parser, Subcommand};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

#[derive(Debug, Parser)]
#[command(
    name = "lyra-v2",
    version = "2.0.0-dev.1",
    about = "Lyra v2 local coding preview"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Propose changes from a local model and save an independent task checkpoint.
    Task {
        #[command(flatten)]
        input: TaskInput,
        #[command(flatten)]
        model: LocalModel,
    },
    /// Send a task and tool observations to one explicitly chosen HTTP provider.
    #[cfg(feature = "gateway-http")]
    TaskHttp {
        #[command(flatten)]
        input: TaskInput,
        #[command(flatten)]
        provider: super::integrations::HttpModel,
    },
    /// Resume a cancelled or interrupted task from its most recent checkpoint.
    Resume {
        #[arg(long)]
        state: PathBuf,
        #[command(flatten)]
        model: LocalModel,
    },
    /// Read the saved JSON task report.
    Show {
        #[arg(long)]
        state: PathBuf,
    },
    /// Explicitly apply the complete pending proposal.
    Apply {
        #[arg(long)]
        state: PathBuf,
    },
    /// Preview or import supported settings and conversations from v1.
    #[cfg(feature = "desktop")]
    ImportV1 {
        #[arg(long)]
        source: PathBuf,
        /// Write the preview to this new or identical v2 import file.
        #[arg(long)]
        into: Option<PathBuf>,
    },
    /// Run a configured isolated checker against the workspace.
    Verify {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        config: PathBuf,
        #[arg(long = "file")]
        files: Vec<String>,
    },
    /// Inspect or reconcile an interrupted project publication.
    Recover(crate::publication::RecoveryArgs),
    /// Build, import or query a code graph.
    Graph(crate::codegraph::GraphArgs),
    /// Coordinate several agents (Codex, Claude Code, local Lyra) on one project.
    Hub(super::hub::cli::HubArgs),
    /// Read a GitHub issue and export it as a task file.
    #[cfg(feature = "gateway-http")]
    GithubIssue(super::integrations::GitHubIssue),
    /// Publish the reviewed, checked proposal as a GitHub draft PR.
    #[cfg(feature = "gateway-http")]
    GithubPublish(super::integrations::GitHubPublish),
}

#[derive(Debug, Args)]
pub struct TaskInput {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    state: PathBuf,
    #[arg(
        long,
        conflicts_with = "task_file",
        required_unless_present = "task_file"
    )]
    task: Option<String>,
    #[arg(long, conflicts_with = "task")]
    task_file: Option<PathBuf>,
    #[arg(long = "file")]
    files: Vec<String>,
    #[arg(long)]
    check_config: Option<PathBuf>,
    #[arg(long, default_value_t = 32, value_parser = clap::value_parser!(u32).range(1..=64))]
    max_calls: u32,
    /// Ask a read-only reviewer to inspect the proposed change before finishing.
    #[arg(long)]
    reviewer: bool,
}

impl TaskInput {
    fn create(self) -> io::Result<(TaskReport, TaskStore)> {
        let check = self
            .check_config
            .as_deref()
            .map(executor::load_config)
            .transpose()?;
        let task = if let Some(path) = self.task_file {
            let mut task = String::new();
            std::fs::File::open(path)?
                .take(16 * 1024 + 1)
                .read_to_string(&mut task)?;
            task
        } else {
            self.task.unwrap_or_default()
        };
        let report = TaskReport::new(TaskRequest {
            repo: self.repo,
            task,
            files: self.files,
            check,
            max_calls: self.max_calls,
            reviewer: self.reviewer,
        })?;
        let store = TaskStore::create(&self.state, &report)?;
        Ok((report, store))
    }
}

#[derive(Debug, Args)]
pub struct LocalModel {
    #[arg(long)]
    pub model: PathBuf,
    #[arg(long, default_value_t = 8192, value_parser = clap::value_parser!(u32).range(512..=32768))]
    pub context: u32,
    #[arg(long, default_value_t = 1024, value_parser = clap::value_parser!(u32).range(32..=8192))]
    pub max_tokens: u32,
    #[arg(long, default_value_t = inference::default_threads(), value_parser = clap::value_parser!(u32).range(1..=256))]
    pub threads: u32,
    /// CPU (0), manual CUDA layers (1..1022), NVIDIA Balanced (1023), or Auto (1024).
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=1024))]
    pub gpu_layers: u32,
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub timeout_secs: u64,
}

impl LocalModel {
    pub fn inference_args(self) -> inference::GenerateArgs {
        inference::GenerateArgs {
            model: self.model,
            prompt: "Lyra v2 session".into(),
            skills: None,
            selected_skills: Vec::new(),
            context: self.context,
            max_tokens: self.max_tokens,
            threads: self.threads,
            gpu_layers: self.gpu_layers,
            timeout_secs: self.timeout_secs,
            raw: false,
        }
    }
}

pub(super) fn write_report(output: &mut impl Write, report: &TaskReport) -> io::Result<()> {
    serde_json::to_writer(&mut *output, report).map_err(io::Error::other)?;
    writeln!(output)
}

fn run_model(
    mut report: TaskReport,
    store: &TaskStore,
    model: LocalModel,
    output: &mut impl Write,
) -> io::Result<()> {
    if !matches!(report.state, TaskState::Running | TaskState::Cancelled) {
        return Err(io::Error::other("Task is not resumable"));
    }
    let args = model.inference_args();
    let result = inference::with_session(&args, |session| {
        report.run_local(session, &mut |report| store.save(report))
    });
    write_report(output, &report)?;
    result?;
    exit_status(report)
}

pub(super) fn exit_status(report: TaskReport) -> io::Result<()> {
    match report.state {
        TaskState::Failed => Err(io::Error::other(report.summary)),
        TaskState::Cancelled => Err(io::Error::new(io::ErrorKind::Interrupted, report.summary)),
        _ => Ok(()),
    }
}

pub fn run(cli: Cli, output: &mut impl Write) -> io::Result<()> {
    match cli.command {
        Command::Task { input, model } => {
            let (report, store) = input.create()?;
            run_model(report, &store, model, output)
        }
        #[cfg(feature = "gateway-http")]
        Command::TaskHttp { input, provider } => {
            let (report, store) = input.create()?;
            super::integrations::run_http(provider, report, &store, output)
        }
        Command::Resume { state, model } => {
            let store = TaskStore::open(&state)?;
            run_model(store.load()?, &store, model, output)
        }
        Command::Show { state } => write_report(output, &TaskStore::open(&state)?.load()?),
        Command::Apply { state } => write_report(output, &TaskStore::open(&state)?.apply()?),
        #[cfg(feature = "desktop")]
        Command::ImportV1 { source, into } => {
            let bundle = super::migration::ImportBundle::preview(&source)?;
            if let Some(path) = into {
                bundle.install(&path)?;
            }
            serde_json::to_writer(&mut *output, &bundle).map_err(io::Error::other)?;
            writeln!(output)
        }
        Command::Verify {
            repo,
            config,
            files,
        } => executor::run(&repo, &config, &files, output),
        Command::Recover(args) => crate::publication::recover(args, output),
        Command::Graph(args) => crate::codegraph::run(args, output),
        Command::Hub(args) => super::hub::cli::run(args, output),
        #[cfg(feature = "gateway-http")]
        Command::GithubIssue(args) => super::integrations::issue(args, output),
        #[cfg(feature = "gateway-http")]
        Command::GithubPublish(args) => super::integrations::publish(args, output),
    }
}
