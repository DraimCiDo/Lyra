//! `lyra-v2 hub …` commands. Owner commands change the project; `mcp` and `work` run agents.
use super::{AgentKind, Hub, OWNER, Status, mcp, worker};
use crate::{executor, inference, v2::cli::LocalModel};
use clap::{Args, Subcommand};
use serde_json::json;
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Debug, Args)]
pub struct HubArgs {
    /// The hub folder (outside the project).
    #[arg(long, global = true)]
    hub: Option<PathBuf>,
    #[command(subcommand)]
    command: HubCommand,
}

#[derive(Debug, Subcommand)]
enum HubCommand {
    /// Create a hub for a git project, optionally with the owner's check configuration.
    Init {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "main")]
        base: String,
        #[arg(long)]
        check_config: Option<PathBuf>,
    },
    /// Show agents, active tasks and file leases.
    Status,
    /// Add a task to the board.
    Add {
        #[arg(long)]
        title: String,
        #[arg(long)]
        description: String,
        #[arg(long = "depends-on")]
        depends_on: Vec<i64>,
    },
    /// List tasks.
    Tasks {
        #[arg(long, value_enum)]
        status: Option<Status>,
    },
    /// Show a task branch compared with the base branch.
    Diff {
        #[arg(long)]
        task: i64,
    },
    /// Merge a reviewed task into the base branch of the project.
    Land {
        #[arg(long)]
        task: i64,
        /// The commit you reviewed (the `commit` field of `diff`); a moved branch is refused.
        #[arg(long)]
        commit: String,
    },
    /// Send a task under review back to the board, with a note for the next agent.
    Reopen {
        #[arg(long)]
        task: i64,
        #[arg(long)]
        note: String,
    },
    /// Cancel a task.
    Cancel {
        #[arg(long)]
        task: i64,
    },
    /// Drop cancelled tasks from a task's dependencies so it can be claimed.
    Unblock {
        #[arg(long)]
        task: i64,
    },
    /// Send a message to one agent or to everyone.
    Send {
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        task: Option<i64>,
        #[arg(long)]
        body: String,
    },
    /// Read messages addressed to the owner or to everyone.
    Inbox,
    /// Serve the hub to one agent over MCP on stdin/stdout.
    Mcp {
        #[arg(long)]
        agent: String,
        #[arg(long, value_enum, default_value_t = AgentKind::Other)]
        kind: AgentKind,
    },
    /// Retry the commit of a task a Lyra worker applied but could not commit.
    Commit {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        task: i64,
        /// The state folder named in the worker's error message.
        #[arg(long)]
        state: PathBuf,
    },
    /// Run an explicitly configured team until stopped or its task limit is reached.
    Team {
        #[arg(long)]
        config: PathBuf,
    },
    /// Run a local Lyra agent on hub tasks with a GGUF model.
    Work {
        #[arg(long)]
        agent: String,
        /// Work on this task instead of the oldest ready one.
        #[arg(long)]
        task: Option<i64>,
        /// Run a task already claimed under this agent name (managed team only).
        #[arg(long, hide = true, requires = "task", conflicts_with = "repeat")]
        assigned: bool,
        /// Keep taking tasks until none is ready.
        #[arg(long = "loop", conflicts_with = "task")]
        repeat: bool,
        #[arg(long, default_value_t = 32, value_parser = clap::value_parser!(u32).range(1..=64))]
        max_calls: u32,
        #[command(flatten)]
        model: LocalModel,
    },
}

fn print(output: &mut impl Write, value: &impl serde::Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *output, value).map_err(io::Error::other)?;
    writeln!(output)
}

pub fn run(args: HubArgs, output: &mut impl Write) -> io::Result<()> {
    let root = args
        .hub
        .ok_or_else(|| io::Error::other("--hub <folder> is required"))?;
    let owner = || Hub::open(&root, OWNER, None);
    match args.command {
        HubCommand::Init {
            repo,
            base,
            check_config,
        } => {
            let check = check_config
                .as_deref()
                .map(executor::load_config)
                .transpose()?;
            Hub::init(&root, &repo, &base, check)?;
            print(output, &owner()?.status()?)
        }
        HubCommand::Status => print(output, &owner()?.status()?),
        HubCommand::Add {
            title,
            description,
            depends_on,
        } => print(
            output,
            &json!({"task_id": owner()?.create_task(&title, &description, &depends_on)?}),
        ),
        HubCommand::Tasks { status } => print(output, &owner()?.tasks(status)?),
        HubCommand::Diff { task } => print(output, &owner()?.diff(task, 512 * 1024)?),
        HubCommand::Land { task, commit } => print(output, &owner()?.land(task, &commit)?),
        HubCommand::Reopen { task, note } => print(output, &owner()?.reopen(task, &note)?),
        HubCommand::Cancel { task } => print(output, &owner()?.cancel(task)?),
        HubCommand::Unblock { task } => print(output, &owner()?.unblock(task)?),
        HubCommand::Send { to, task, body } => print(
            output,
            &json!({"message_id": owner()?.send(to.as_deref(), task, &body)?}),
        ),
        HubCommand::Inbox => print(output, &owner()?.read(100)?),
        HubCommand::Mcp { agent, kind } => {
            let stdin = io::stdin();
            mcp::serve(&root, &agent, kind, stdin.lock(), output)
        }
        HubCommand::Commit { agent, task, state } => {
            let hub = Hub::open_agent(&root, &agent, AgentKind::Lyra)?;
            let cancel = std::sync::atomic::AtomicBool::new(false);
            print(
                output,
                &worker::commit_applied(&hub, task, &state, &cancel)?,
            )
        }
        HubCommand::Team { config } => {
            let config = super::team::Config::load(&config)?;
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let signal = cancel.clone();
            ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))
                .map_err(io::Error::other)?;
            let mut team = super::team::Team::start(root, config, std::env::current_exe()?)?;
            let mut last = String::new();
            while team.running() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    team.stop();
                }
                let events = serde_json::to_string(&team.events()).map_err(io::Error::other)?;
                if events != last {
                    writeln!(output, "{events}")?;
                    last = events;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            team.join();
            print(output, &team.events())
        }
        HubCommand::Work {
            agent,
            task,
            repeat,
            assigned,
            max_calls,
            model,
        } => {
            let mut hub = Hub::open_agent(&root, &agent, AgentKind::Lyra)?;
            let args = model.inference_args();
            inference::with_session(&args, |session| {
                // Tasks this run released stay on the board for another agent or a later
                // run; the loop must not claim them again and starve the tasks behind them.
                let mut released = Vec::new();
                loop {
                    // Each task gets the full `--timeout-secs`, not what earlier tasks left.
                    session.reset_deadline();
                    let result = if assigned {
                        let id =
                            task.ok_or_else(|| io::Error::other("Assigned worker needs --task"))?;
                        let claimed = hub.owned_task(id)?;
                        worker::run_claimed(
                            &hub,
                            claimed,
                            max_calls,
                            session.cancellation(),
                            &mut |report, save| report.run_local(session, save),
                        )
                    } else {
                        worker::run_one(
                            &mut hub,
                            task,
                            &released,
                            max_calls,
                            session.cancellation(),
                            &mut |report, save| report.run_local(session, save),
                        )
                    };
                    match result {
                        Ok(value) => {
                            if value["status"] == "open"
                                && let Some(id) = value["task"].as_i64()
                            {
                                released.push(id);
                            }
                            print(output, &value)?
                        }
                        Err(error) if repeat && super::is_queue_empty(&error) => {
                            return Ok(());
                        }
                        Err(error) => return Err(error),
                    }
                    if !repeat
                        || session
                            .cancellation()
                            .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        return Ok(());
                    }
                }
            })
        }
    }
}
