use crate::{
    inference::{self, GenerateArgs, Session},
    workspace::{Edit, Workspace},
};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

pub const ACTION_GRAMMAR: &str = r#"
root ::= (list | page | range | scan | read | search | docs | edit | create | discard | review | verify | execute | workplan | inspectdoc | artifact | finish) ws
list ::= "{" ws "\"tool\"" ws ":" ws "\"list\"" ws "}"
read ::= "{" ws "\"tool\"" ws ":" ws "\"read_file\"" ws "," ws "\"path\"" ws ":" ws path ws "}"
search ::= "{" ws "\"tool\"" ws ":" ws "\"search\"" ws "," ws "\"query\"" ws ":" ws string ws "}"
edit ::= "{" ws "\"tool\"" ws ":" ws "\"propose_edit\"" ws "," ws "\"path\"" ws ":" ws path ws "," ws "\"old\"" ws ":" ws string ws "," ws "\"new\"" ws ":" ws string ws "}"
finish ::= "{" ws "\"tool\"" ws ":" ws "\"finish\"" ws "," ws "\"summary\"" ws ":" ws string ws "}"
docs ::= "{" ws "\"tool\"" ws ":" ws "\"search_docs\"" ws "," ws "\"query\"" ws ":" ws string ws "}"
create ::= "{" ws "\"tool\"" ws ":" ws "\"propose_file\"" ws "," ws "\"path\"" ws ":" ws string ws "," ws "\"content\"" ws ":" ws string ws "}"
verify ::= "{" ws "\"tool\"" ws ":" ws "\"verify\"" ws "}"
review ::= "{" ws "\"tool\"" ws ":" ws "\"request_changes\"" ws "," ws "\"summary\"" ws ":" ws string ws "}"
discard ::= "{" ws "\"tool\"" ws ":" ws "\"discard_proposal\"" ws "," ws "\"path\"" ws ":" ws string ws "}"
page ::= "{" ws "\"tool\"" ws ":" ws "\"list_files\"" ws "," ws "\"prefix\"" ws ":" ws string ws "," ws "\"offset\"" ws ":" ws number ws "}"
range ::= "{" ws "\"tool\"" ws ":" ws "\"read_range\"" ws "," ws "\"path\"" ws ":" ws path ws "," ws "\"start_line\"" ws ":" ws number ws "," ws "\"count\"" ws ":" ws number ws "}"
scan ::= "{" ws "\"tool\"" ws ":" ws "\"search_page\"" ws "," ws "\"query\"" ws ":" ws string ws "," ws "\"offset\"" ws ":" ws number ws "}"
execute ::= "{" ws "\"tool\"" ws ":" ws "\"run_script\"" ws "," ws "\"environment\"" ws ":" ws string ws "," ws "\"script\"" ws ":" ws string ws "," ws "\"inputs\"" ws ":" ws strings ws "," ws "\"outputs\"" ws ":" ws strings ws "}"
workplan ::= "{" ws "\"tool\"" ws ":" ws "\"set_work_plan\"" ws "," ws "\"plan\"" ws ":" ws object ws "}"
inspectdoc ::= "{" ws "\"tool\"" ws ":" ws "\"inspect_document\"" ws "," ws "\"environment\"" ws ":" ws string ws "," ws "\"path\"" ws ":" ws string ws "," ws "\"start\"" ws ":" ws number ws "," ws "\"count\"" ws ":" ws number ws "}"
artifact ::= "{" ws "\"tool\"" ws ":" ws "\"read_artifact\"" ws "," ws "\"path\"" ws ":" ws string ws "}"
strings ::= "[" ws (string (ws "," ws string)*)? ws "]"
object ::= "{" ws (string ws ":" ws value (ws "," ws string ws ":" ws value)*)? ws "}"
value ::= string | number | object | array | "true" | "false" | "null"
array ::= "[" ws (value (ws "," ws value)*)? ws "]"
number ::= "0" | [1-9] [0-9]{0,6}
string ::= "\"" ([^"\\\x00-\x1F] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F]{4}))* "\""
ws ::= [ \t\n\r]*
path ::= string
"#;

pub fn action_grammar(files: &[String], read_only: bool) -> io::Result<String> {
    if files.len() > 1024 || files.iter().any(|p| p.len() > 1024) {
        return Err(io::Error::other("Индекс превышает лимит схемы."));
    }
    let mut grammar = ACTION_GRAMMAR.to_owned();
    if read_only {
        grammar = grammar
            .replace(" | edit", "")
            .replace(" | create", "")
            .replace(" | discard", "")
            .replace(" | execute", "")
            .replace(" | workplan", "");
    }
    if files.is_empty() {
        grammar = grammar
            .replace(" | read", "")
            .replace(" | range", "")
            .replace(" | edit", "");
    } else {
        let literals = files
            .iter()
            .map(|path| {
                serde_json::to_string(path).and_then(|literal| serde_json::to_string(&literal))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(io::Error::other)?;
        grammar = grammar.replace(
            "path ::= string",
            &format!("path ::= {}", literals.join(" | ")),
        );
    }
    Ok(grammar)
}

#[derive(Debug, Args)]
pub struct TaskArgs {
    #[arg(long)]
    #[cfg_attr(feature = "gateway-http", arg(conflicts_with = "provider_config"))]
    pub model: Option<PathBuf>,
    /// Opt-in HTTP provider registry for remote task inference
    #[cfg(feature = "gateway-http")]
    #[arg(long, conflicts_with = "model")]
    pub provider_config: Option<PathBuf>,
    /// Explicitly permitted HTTP provider (required for cloud tasks)
    #[cfg(feature = "gateway-http")]
    #[arg(long = "allow")]
    pub allowed_providers: Vec<String>,
    /// Optional cloud model selector
    #[cfg(feature = "gateway-http")]
    #[arg(long = "cloud-model")]
    pub cloud_model: Option<String>,
    #[arg(long, alias = "workspace")]
    pub repo: PathBuf,
    /// UTF-8 файл задания (до 16 KiB)
    #[arg(long)]
    pub task_file: PathBuf,
    /// Выбрать файлы контекста явно; можно повторить до 32 раз
    #[arg(long = "file")]
    pub files: Vec<String>,
    /// Локальный document pack; inference не выполняет сетевые запросы
    #[arg(long)]
    pub docs: Option<PathBuf>,
    /// ECC skill pack (JSON or SKILL.md)
    #[arg(long)]
    pub skills: Option<PathBuf>,
    /// Explicit ECC skills for all task roles
    #[arg(long = "skill")]
    pub selected_skills: Vec<String>,
    /// Request-local, lossless pooling of repeated older tool results
    #[arg(long)]
    pub dedup_tool_context: bool,
    /// Explicit trusted Docker check configuration; model cannot change image or argv
    #[arg(long)]
    pub check_config: Option<PathBuf>,
    #[arg(long)]
    pub environments: Option<PathBuf>,
    #[arg(long = "allow-environment", requires = "environments")]
    pub allowed_environments: Vec<String>,
    #[arg(long, requires = "environments")]
    pub artifacts_dir: Option<PathBuf>,
    /// Explicit local SQLite checkpoint store (contains selected source code)
    #[arg(long)]
    pub session_db: Option<PathBuf>,
    /// Resume a snapshot from --session-db; task file must still match
    #[arg(long, requires = "session_db")]
    pub resume: Option<String>,
    /// Два read-only подагента: исследователь до кодера, ревьюер после
    #[arg(long)]
    pub subagents: bool,
    #[arg(long,default_value_t=48,value_parser=clap::value_parser!(u32).range(1..=256))]
    pub max_calls: u32,
    #[arg(long,default_value_t=8192,value_parser=clap::value_parser!(u32).range(128..=32768))]
    pub context: u32,
    #[arg(long,default_value_t=2048,value_parser=clap::value_parser!(u32).range(1..=8192))]
    pub max_tokens: u32,
    #[arg(long,default_value_t=inference::default_threads(),value_parser=clap::value_parser!(u32).range(1..=256))]
    pub threads: u32,
    #[arg(long,default_value_t=0,value_parser=clap::value_parser!(u32).range(0..=1024))]
    pub gpu_layers: u32,
    #[arg(long,default_value_t=1800,value_parser=clap::value_parser!(u64).range(1..=4600))]
    pub timeout_secs: u64,
    /// Только для base/test GGUF, без chat template
    #[arg(long)]
    pub raw: bool,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(long, alias = "workspace")]
    pub repo: PathBuf,
    #[arg(long)]
    pub report: PathBuf,
    /// Индекс предложения в отчёте, начиная с 0
    #[arg(long, conflicts_with = "all", required_unless_present = "all")]
    pub index: Option<usize>,
    /// Применить весь набор после проверки всех конфликтов
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    InspectDocument {
        environment: String,
        path: String,
        start: usize,
        count: usize,
    },
    ReadArtifact {
        path: String,
    },
    RunScript {
        environment: String,
        script: String,
        inputs: Vec<String>,
        outputs: Vec<String>,
    },
    SetWorkPlan {
        plan: crate::universal::WorkPlan,
    },
    Verify,
    List,
    ListFiles {
        prefix: String,
        offset: usize,
    },
    ReadRange {
        path: String,
        start_line: usize,
        count: usize,
    },
    SearchPage {
        query: String,
        offset: usize,
    },
    ReadFile {
        path: String,
    },
    Search {
        query: String,
    },
    SearchDocs {
        query: String,
    },
    RequestChanges {
        summary: String,
    },
    DiscardProposal {
        path: String,
    },
    ProposeFile {
        path: String,
        content: String,
    },
    ProposeEdit {
        path: String,
        old: String,
        new: String,
    },
    Finish {
        summary: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub call: u32,
    pub role: String,
    pub tool: String,
    pub outcome: String,
    pub argument: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    #[default]
    Incomplete,
    AnalysisComplete,
    ChangesProposed,
    ChangesVerified,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEvidence {
    pub proposal_sha256: String,
    #[serde(default)]
    pub check_sha256: String,
    pub result: crate::executor::VerificationResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_output_sha256: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub workspace: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_plan: Option<crate::universal::WorkPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_store: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_proposals: Vec<crate::universal::artifacts::FileProposal>,
    pub status: String,
    #[serde(default)]
    pub outcome: TaskOutcome,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verifications: Vec<VerificationEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_scope: Vec<String>,
    pub summaries: Vec<(String, String)>,
    pub proposals: Vec<Edit>,
    pub events: Vec<Event>,
    pub calls: u32,
    #[serde(default)]
    pub events_omitted: u32,
    #[serde(default)]
    pub verifications_omitted: u32,
    pub subagents: bool,
    pub truncated_index: bool,
    pub model_review_is_verified_test: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documentation_sources: Vec<crate::knowledge::Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_optimization: Vec<crate::context_optimizer::ContextStats>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_budgets: Vec<crate::task_context::BudgetStats>,
}

pub trait Model {
    fn run_script(
        &mut self,
        _workspace: &Workspace,
        _pending: &[Edit],
        _environment: &str,
        _script: &str,
        _inputs: &[String],
        _outputs: &[String],
    ) -> io::Result<(
        serde_json::Value,
        Vec<crate::universal::artifacts::FileProposal>,
    )> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "No execution permission",
        ))
    }
    fn execution(&mut self) -> Option<&mut crate::universal::execution::ExecutionSession> {
        None
    }
    fn cancellation(&self) -> &std::sync::atomic::AtomicBool {
        static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        &NEVER
    }
    fn input_limit(&self) -> usize {
        96 * 1024
    }
    fn count_prompt(&self, prompt: &str) -> io::Result<usize> {
        Ok(prompt.len())
    }
    fn accounting(&self) -> &'static str {
        "utf8_bytes"
    }
    fn respond(&mut self, prompt: &str) -> io::Result<String>;
}
impl Model for Session<'_> {
    fn input_limit(&self) -> usize {
        self.input_token_limit()
    }
    fn count_prompt(&self, prompt: &str) -> io::Result<usize> {
        self.task_prompt_tokens(prompt)
    }
    fn accounting(&self) -> &'static str {
        "model_tokenizer_with_chat_template"
    }
    fn cancellation(&self) -> &std::sync::atomic::AtomicBool {
        Session::cancellation(self)
    }
    fn respond(&mut self, prompt: &str) -> io::Result<String> {
        struct Bounded(Vec<u8>);
        impl Write for Bounded {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0.len() + bytes.len() > 65536 {
                    return Err(io::Error::other("Ответ модели превышает 64 KiB."));
                }
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut output = Bounded(Vec::new());
        let envelope: serde_json::Value = serde_json::from_str(prompt).map_err(io::Error::other)?;
        let mut files: Vec<String> =
            serde_json::from_value(envelope["files"].clone()).map_err(io::Error::other)?;
        if let Some(proposals) = envelope["proposals"].as_array() {
            for proposal in proposals {
                if let Some(path) = proposal["path"].as_str()
                    && !files.iter().any(|file| file == path)
                {
                    files.push(path.to_owned());
                }
            }
        }
        // Paths visible in observations remain addressable even if the ranked list
        // was shortened by the token budget. Runtime Workspace checks remain authoritative.
        let visible = crate::context_optimizer::restore(&envelope)?;
        for item in visible["history"].as_array().into_iter().flatten() {
            if item["result"]["status"] != "success" {
                continue;
            }
            let data = &item["result"]["data"];
            for path in data["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .chain(
                    data["matches"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v["path"].as_str()),
                )
                .chain(
                    data["scoped_instructions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v["path"].as_str()),
                )
                .chain(data["path"].as_str())
            {
                if !files.iter().any(|p| p == path) {
                    files.push(path.to_owned());
                }
            }
        }
        let mut grammar = action_grammar(&files, envelope["read_only"].as_bool().unwrap_or(true))?;
        for (tool, branch) in [
            ("verify", " | verify"),
            ("request_changes", " | review"),
            ("run_script", " | execute"),
            ("set_work_plan", " | workplan"),
            ("inspect_document", " | inspectdoc"),
            ("read_artifact", " | artifact"),
        ] {
            if !envelope["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|entry| entry["tool"] == tool))
            {
                grammar = grammar.replace(branch, "");
            }
        }
        self.complete_with_system(
            prompt,
            Some(crate::agent_runtime::TASK_SYSTEM_PROMPT),
            Some(&grammar),
            &mut output,
        )?;
        String::from_utf8(output.0).map_err(io::Error::other)
    }
}

pub fn execute(
    model: &mut impl Model,
    workspace: &mut Workspace,
    task: &str,
    subagents: bool,
    max_calls: u32,
) -> Report {
    execute_with_docs(
        model,
        workspace,
        task,
        subagents,
        max_calls,
        &crate::knowledge::Pack::empty(),
    )
}

pub fn execute_with_docs(
    model: &mut impl Model,
    workspace: &mut Workspace,
    task: &str,
    subagents: bool,
    max_calls: u32,
    docs: &crate::knowledge::Pack,
) -> Report {
    execute_with_context(model, workspace, task, subagents, max_calls, docs, false)
}

pub fn execute_with_context(
    model: &mut impl Model,
    workspace: &mut Workspace,
    task: &str,
    subagents: bool,
    max_calls: u32,
    docs: &crate::knowledge::Pack,
    dedup_tool_context: bool,
) -> Report {
    execute_workflow(
        model,
        workspace,
        task,
        WorkflowOptions {
            subagents,
            max_calls,
            docs,
            dedup_tool_context,
            check: None,
            deadline: None,
            previous: None,
            checkpoint: None,
        },
    )
}

pub type Checkpoint<'a> = dyn Fn(&Report) -> io::Result<()> + 'a;

pub struct WorkflowOptions<'a> {
    pub subagents: bool,
    pub max_calls: u32,
    pub docs: &'a crate::knowledge::Pack,
    pub dedup_tool_context: bool,
    pub check: Option<&'a crate::executor::ExecutionRequest>,
    pub deadline: Option<std::time::Instant>,
    pub previous: Option<Report>,
    pub checkpoint: Option<&'a Checkpoint<'a>>,
}

pub fn execute_workflow(
    model: &mut impl Model,
    workspace: &mut Workspace,
    task: &str,
    options: WorkflowOptions<'_>,
) -> Report {
    use crate::agent_runtime::AgentRole;
    let WorkflowOptions {
        subagents,
        max_calls,
        docs,
        dedup_tool_context,
        check,
        deadline,
        previous,
        checkpoint,
    } = options;
    let context = crate::project::inspect(workspace).unwrap_or_default();
    let mut report = previous.unwrap_or_else(|| Report {
        schema_version: 4,
        work_plan: None,
        artifact_store: None,
        file_proposals: Vec::new(),
        workspace: workspace.root.clone(),
        status: "incomplete".into(),
        outcome: TaskOutcome::Incomplete,
        verifications: Vec::new(),
        verification_scope: Vec::new(),
        summaries: Vec::new(),
        proposals: Vec::new(),
        events: Vec::new(),
        calls: 0,
        events_omitted: 0,
        verifications_omitted: 0,
        subagents,
        truncated_index: workspace.truncated,
        model_review_is_verified_test: false,
        documentation_sources: Vec::new(),
        context_optimization: Vec::new(),
        prompt_budgets: Vec::new(),
    });
    report.status = "incomplete".into();
    report.outcome = TaskOutcome::Incomplete;
    report.subagents = subagents;
    report.truncated_index = workspace.truncated;
    if report.workspace != workspace.root
        || !matches!(report.schema_version, 4 | 5)
        || report.calls >= max_calls
    {
        report.status = "invalid_resume_or_budget".into();
        return report;
    }
    if task.trim().is_empty()
        || task.len() > 16384
        || max_calls == 0
        || max_calls > 256
        || (subagents && max_calls < 3)
    {
        report.status = "invalid_task".into();
        return report;
    }
    if docs.validate().is_err() {
        report.status = "invalid_documentation".into();
        return report;
    }
    let ranked_files = crate::retrieval::ranked(workspace, task);
    let mut focus_paths = Vec::<String>::new();
    let initial_docs = docs.context(task);
    for snippet in &initial_docs {
        if !report
            .documentation_sources
            .iter()
            .any(|s| s.id == snippet.source.id)
        {
            report.documentation_sources.push(snippet.source.clone());
        }
    }
    if !report.documentation_sources.is_empty() && report.schema_version < 2 {
        report.schema_version = report.schema_version.max(2);
    }
    let instructions = if workspace.files.contains("AGENTS.md") {
        match workspace.read_tool("AGENTS.md") {
            Ok(text) => text,
            Err(error) => {
                report.status = format!("project_instructions_error: {error}");
                return report;
            }
        }
    } else {
        String::new()
    };
    let roles: Vec<(AgentRole, u32)> = if subagents {
        let child = (max_calls / 4).max(1);
        vec![
            (AgentRole::Researcher, child),
            (AgentRole::Coder, max_calls - 2 * child),
            (AgentRole::Reviewer, child),
        ]
    } else {
        vec![(AgentRole::Coder, max_calls)]
    };
    let mut roles: std::collections::VecDeque<_> = roles.into();
    let mut repair_rounds = 0;
    while let Some((role_spec, budget)) = roles.pop_front() {
        let role = role_spec.name();
        let mut history = Vec::<serde_json::Value>::new();
        let mut finished = false;
        let mut changes_requested = false;
        let mut malformed = 0u32;
        let mut last_observation = String::new();
        let mut unchanged_steps = 0u32;
        for step in 0..budget {
            if report.calls >= max_calls {
                break;
            }
            if let Some(reason) = stop_reason(model, deadline) {
                report.status = reason.into();
                return report;
            }
            focus_paths.retain(|p| {
                workspace.files.contains(p) || report.proposals.iter().any(|e| &e.path == p)
            });
            let mut active_instructions = Vec::new();
            let mut instruction_paths = std::collections::BTreeSet::new();
            for path in focus_paths
                .last()
                .into_iter()
                .chain(report.proposals.iter().map(|p| &p.path))
                .chain(report.file_proposals.iter().map(|p| &p.path))
            {
                let scopes = match workspace.scoped_instructions(path) {
                    Ok(scopes) => scopes,
                    Err(error) => {
                        report.status = format!("project_instructions_error: {error}");
                        return report;
                    }
                };
                for scope in scopes {
                    let scope_path = scope["path"].as_str().unwrap_or("").to_owned();
                    if scope_path != "AGENTS.md" && instruction_paths.insert(scope_path) {
                        active_instructions.push(scope);
                    }
                }
            }
            let context_files: Vec<String> = if workspace.files.len() <= 128 {
                workspace.files.iter().cloned().collect()
            } else {
                let mut paths = Vec::new();
                for path in focus_paths.iter().rev().chain(ranked_files.iter()) {
                    if !paths.contains(path) {
                        paths.push(path.clone());
                    }
                    if paths.len() == 128 {
                        break;
                    }
                }
                paths
            };
            let example_path = context_files.first().cloned().unwrap_or_default();
            let mut tools = vec![
                serde_json::json!({"tool":"list"}),
                serde_json::json!({"tool":"list_files","prefix":"","offset":0}),
                serde_json::json!({"tool":"read_range","path":example_path,"start_line":1,"count":80}),
                serde_json::json!({"tool":"search_page","query":"symbol","offset":0}),
                serde_json::json!({"tool":"read_file","path":example_path}),
                serde_json::json!({"tool":"search","query":"symbol"}),
                serde_json::json!({"tool":"propose_edit","path":example_path,"old":"exact existing text","new":"replacement"}),
                serde_json::json!({"tool":"propose_file","path":"src/new_file.rs","content":"complete new file content"}),
                serde_json::json!({"tool":"search_docs","query":"API symbol"}),
                serde_json::json!({"tool":"discard_proposal","path":example_path}),
                serde_json::json!({"tool":"finish","summary":"result and limitations"}),
            ];
            let execution_catalog = model.execution().map(|e| e.catalog());
            if execution_catalog.is_some() {
                tools.push(serde_json::json!({"tool":"inspect_document","environment":"registered-document-id","path":example_path,"start":0,"count":20}));
            }
            if !report.file_proposals.is_empty() {
                tools.push(serde_json::json!({"tool":"read_artifact","path":report.file_proposals[0].path}));
            }
            if execution_catalog.is_some() && !role_spec.read_only() {
                tools.push(serde_json::json!({"tool":"run_script","environment":"registered-id","script":"complete script; inputs at /source and outputs at /output","inputs":[example_path],"outputs":["result.txt"]}));
            }
            if !role_spec.read_only() {
                tools.push(serde_json::json!({"tool":"set_work_plan","plan":{"schema_version":1,"goal":task,"technologies":[],"inputs":[],"outputs":[],"constraints":[],"steps":[],"required_capabilities":[],"acceptance":[]}}));
            }
            if check.is_some() && !role_spec.read_only() {
                tools.push(serde_json::json!({"tool":"verify"}));
            }
            if matches!(role_spec, AgentRole::Reviewer) {
                tools.push(serde_json::json!({"tool":"request_changes","summary":"specific defects to repair"}));
            }
            tools.retain(|tool| {
                !role_spec.read_only()
                    || !matches!(
                        tool["tool"].as_str(),
                        Some("propose_edit" | "propose_file" | "discard_proposal")
                    )
            });
            let verification_current = verification_matches(&report, check, workspace);
            let mut role_contract = role_spec.contract();
            role_contract["allowed_tools"] =
                serde_json::json!(tools.iter().map(|tool| &tool["tool"]).collect::<Vec<_>>());
            let envelope = serde_json::json!({
                "protocol_version":2,"role":role,"read_only":role_spec.read_only(),"role_contract":role_contract,
                "workflow":{"phase":if check.is_some() && !verification_current {"inspect_edit_verify"} else if report.proposals.is_empty(){"inspect_and_propose"}else{"review_pending_and_finish"},
                    "remaining_calls":(budget-step).min(max_calls-report.calls),
                    "pending_paths":report.proposals.iter().map(|p| &p.path).collect::<Vec<_>>(),"writes_applied":false,
                    "completion_rule":"If pending proposals cover the task and any required verification is current and passed, finish. Otherwise inspect remaining files. Read pending content before revising it; old must match the current pending text. Discard a proposal to undo it."},
                "technology_selection_policy":crate::universal::SELECTION_POLICY,"work_plan":report.work_plan,"execution_environments":execution_catalog,"file_proposals":report.file_proposals,
                "task":task,"project_instructions":instructions,"active_project_instructions":active_instructions,"verification_required":check.is_some(),"verification_current":verification_current,
                "verification_evidence":report.verifications.last(),
                "project_context":context,"platform_guidance":context.profile.guidance(),
                "documentation_catalog":docs.catalog(),"documentation_context_untrusted":initial_docs,
                "files":context_files,"file_count":workspace.files.len(),"file_list_partial":workspace.files.len()>128,"index_incomplete":workspace.truncated,
                "prior_role_summaries":report.summaries,
                "proposals":report.proposals.iter().enumerate().map(|(i,p)|serde_json::json!({"index":i,"path":p.path,"create":p.create,"replacement":p.replacement})).collect::<Vec<_>>(),
                "tools":tools,
                "history":history
            });
            let (envelope, mut budget_stats) = match crate::task_context::fit(
                envelope,
                model.input_limit(),
                report.calls + 1,
                model.accounting(),
                |text| model.count_prompt(text),
            ) {
                Ok(result) => result,
                Err(error) => {
                    report.status = format!("context_budget_error: {error}");
                    return report;
                }
            };
            let mut prompt = envelope.to_string();
            if dedup_tool_context {
                let (optimized, stats) =
                    crate::context_optimizer::optimize(&envelope, report.calls + 1, role);
                if model
                    .count_prompt(&optimized)
                    .is_ok_and(|tokens| tokens <= model.input_limit())
                {
                    prompt = optimized;
                    report.context_optimization.push(stats);
                }
            }
            budget_stats.units_after = model.count_prompt(&prompt).unwrap_or(usize::MAX);
            report.prompt_budgets.push(budget_stats);
            report.calls += 1;
            if let Some(save) = checkpoint
                && let Err(error) = save(&report)
            {
                report.status = format!("checkpoint_error: {error}");
                return report;
            }
            let _ = writeln!(
                io::stderr().lock(),
                "LYRA_PROGRESS:activity={}",
                serde_json::json!({"label":format!("{role} · model · {}/{}",report.calls,max_calls)})
            );
            let _ = writeln!(
                io::stderr().lock(),
                "Lyra: {role}, вызов {}/{}",
                report.calls,
                max_calls
            );
            let response = match model.respond(&prompt) {
                Ok(text) => text,
                Err(e) => {
                    report.status = if let Some(reason) = stop_reason(model, deadline) {
                        reason.into()
                    } else {
                        match e.kind() {
                            io::ErrorKind::Interrupted => "cancelled".into(),
                            io::ErrorKind::TimedOut => "timeout".into(),
                            _ => format!(
                                "model_error: {}",
                                crate::task_report::diagnostic(&e.to_string())
                            ),
                        }
                    };
                    report.record_event(Event {
                        call: report.calls,
                        role: role.into(),
                        tool: "model".into(),
                        outcome: e.to_string(),
                        argument: None,
                    });
                    return report;
                }
            };
            if let Some(reason) = stop_reason(model, deadline) {
                report.status = reason.into();
                return report;
            }
            if response.len() > 65536 {
                report.status = "response_limit".into();
                return report;
            }
            let action: Action = match serde_json::from_str(response.trim()) {
                Ok(a) => a,
                Err(e) => {
                    report.status = format!(
                        "invalid_response: {}",
                        crate::task_report::diagnostic(&e.to_string())
                    );
                    report.record_event(Event {
                        call: report.calls,
                        role: role.into(),
                        tool: "invalid_response".into(),
                        outcome: e.to_string(),
                        argument: Some(response.chars().take(512).collect()),
                    });
                    if (e.is_syntax() || e.is_eof()) && malformed < 2 {
                        malformed += 1;
                        history.push(serde_json::json!({"action":{"tool":"invalid_response"},
                            "result":{"status":"error","summary":"Return one valid JSON action matching tools; no Markdown or prose.",
                            "data":{"error":e.to_string()},"next_actions":["read_file","finish"]}}));
                        if history.len() > 4 {
                            history.remove(0);
                        }
                        continue;
                    }
                    return report;
                }
            };
            let tool = match &action {
                Action::InspectDocument { .. } => "inspect_document",
                Action::ReadArtifact { .. } => "read_artifact",
                Action::RunScript { .. } => "run_script",
                Action::SetWorkPlan { .. } => "set_work_plan",
                Action::Verify => "verify",
                Action::List => "list",
                Action::ListFiles { .. } => "list_files",
                Action::ReadRange { .. } => "read_range",
                Action::SearchPage { .. } => "search_page",
                Action::ReadFile { .. } => "read_file",
                Action::Search { .. } => "search",
                Action::ProposeEdit { .. } => "propose_edit",
                Action::DiscardProposal { .. } => "discard_proposal",
                Action::RequestChanges { .. } => "request_changes",
                Action::ProposeFile { .. } => "propose_file",
                Action::SearchDocs { .. } => "search_docs",
                Action::Finish { .. } => "finish",
            };
            if let Some(execution) = model.execution() {
                execution.staged_files = report.file_proposals.clone();
                execution.staged_store = report.artifact_store.clone();
            }
            let outcome = match &action {
                Action::ReadArtifact{path}=>{( || {let file=report.file_proposals.iter().find(|f|&f.path==path).ok_or_else(||io::Error::other("Artifact missing"))?;let store=crate::universal::artifacts::Store::open(report.artifact_store.as_ref().ok_or_else(||io::Error::other("Artifact store missing"))?)?;let bytes=store.get(&file.replacement)?;if bytes.len()>65536{return Ok(serde_json::json!({"path":path,"blob":file.replacement,"note":"Use document inspection for binary/large content"}));}match String::from_utf8(bytes){Ok(text)=>Ok(serde_json::json!({"path":path,"text":text,"untrusted":true})),Err(_)=>Ok(serde_json::json!({"path":path,"blob":file.replacement,"binary":true}))}})()},
                Action::InspectDocument{environment,path,start,count}=>{( || {
                    if *start>100000 || !(1..=200).contains(count){return Err(io::Error::other("Invalid document range"));}
                    let script=format!("import json,lyra_documents as d\nfrom pathlib import Path\nPath('/output/inspection.json').write_text(json.dumps(d.inspect('/source/'+{}, {}, {}),ensure_ascii=False),encoding='utf-8')",serde_json::to_string(path).map_err(io::Error::other)?,start,count);
                    let (_,files)=model.run_script(workspace,&report.proposals,environment,&script,std::slice::from_ref(path),&["inspection.json".into()])?;
                    let result=files.first().ok_or_else(||io::Error::other("Document inspection failed"))?;
                    let bytes=model.execution().ok_or_else(||io::Error::other("Environment unavailable"))?.store.get(&result.replacement)?;
                    if bytes.len()>65536{return Err(io::Error::other("Inspection exceeds context bound"));}
                    serde_json::from_slice(&bytes).map_err(io::Error::other)
                })()},

                Action::SetWorkPlan{plan}=> {if role_spec.read_only(){Err(io::Error::other("Read-only role cannot change work plan"))}else{plan.validate().map(|()|{report.work_plan=Some(plan.clone());serde_json::json!({"plan_accepted":true,"execution_granted":false})})}},
                Action::RunScript{environment,script,inputs,outputs}=> {
                    if role_spec.read_only(){Err(io::Error::other("Read-only role cannot execute scripts"))}
                    else if let Some(root)=model.execution().map(|e|e.store.root.clone()) {
                        model.run_script(workspace,&report.proposals,environment,script,inputs,outputs).and_then(|(value,files)|{
                            if !files.is_empty(){
                                let mut combined=report.file_proposals.clone();
                                for file in files {combined.retain(|p|p.path!=file.path);combined.push(file);}
                                if combined.len()+report.proposals.len()>32{return Err(io::Error::other("Too many proposals"));}
                                if combined.iter().any(|p|report.proposals.iter().any(|t|t.path==p.path)){return Err(io::Error::other("Text and binary proposals overlap"));}
                                report.file_proposals=combined;report.artifact_store=Some(root);report.schema_version=5;
                            }Ok(value)
                        })
                    }else{Err(io::Error::new(io::ErrorKind::PermissionDenied,"No execution environment granted"))}
                },
                Action::Verify => {
                    if role_spec.read_only() {
                        Err(io::Error::new(io::ErrorKind::PermissionDenied, "Verify is a coder tool"))
                    } else if let Some(check) = check {
                        let mut request = check.clone();
                        if let Some(deadline) = deadline {
                            request.timeout_secs = request.timeout_secs.min(deadline.saturating_duration_since(std::time::Instant::now()).as_secs());
                        }
                        crate::executor::verify_changes(&workspace.root, &workspace.files.iter().cloned().collect::<Vec<_>>(), &report.proposals,
                            &request, model.cancellation()).and_then(|result| {
                            let proposal_sha256=proposal_digest(&report.proposals)?;
                            crate::task_report::evidence(&mut report,proposal_sha256,value_digest(check)?,result)
                        })
                    } else {
                        Err(io::Error::new(io::ErrorKind::PermissionDenied, "No check was authorized"))
                    }
                }
                Action::List => crate::retrieval::list(workspace, &report.proposals, "", 0),
                Action::ListFiles { prefix, offset } => crate::retrieval::list(workspace, &report.proposals, prefix, *offset),
                Action::ReadRange { path, start_line, count } => crate::retrieval::range(workspace, &report.proposals, path, *start_line, *count),
                Action::ReadFile { path } => crate::retrieval::read(workspace, &report.proposals, path)
                    .and_then(|content| Ok(serde_json::json!({"path":path,"line_endings":if content.contains("\r\n") { if content.replace("\r\n", "").contains('\n') { "mixed" } else { "crlf" } } else { "lf" },
                        "source_sha256":crate::retrieval::sha256(content.as_bytes()),"content":content,"scoped_instructions":workspace.scoped_instructions(path)?}))),
                Action::Search { query } => crate::retrieval::search(workspace, &report.proposals, query, 0),
                Action::SearchPage { query, offset } => crate::retrieval::search(workspace, &report.proposals, query, *offset),
                Action::SearchDocs { query } => docs.search(query, 2, 2048).and_then(|snippets| {
                    for snippet in &snippets {
                        if !report
                            .documentation_sources
                            .iter()
                            .any(|s| s.id == snippet.source.id)
                        {
                            report.documentation_sources.push(snippet.source.clone());
                        }
                    }
                    if !snippets.is_empty() {
                        report.schema_version = report.schema_version.max(2);
                    }
                    serde_json::to_value(snippets).map_err(io::Error::other)
                }),
                Action::ProposeFile { path, content } => {
                    if role != "coder" {
                        Err(io::Error::other("Подагенту запрещено создавать файлы."))
                    } else if report.proposals.len() >= crate::workspace::MAX_PROPOSALS
                        || report
                            .proposals
                            .iter()
                            .any(|e| e.path.eq_ignore_ascii_case(path))
                    {
                        Err(io::Error::other("Лимит предложений или повтор пути."))
                    } else {
                        workspace.propose_new(path,content).and_then(|edit|{
                        if !crate::task_report::proposal_fits(&report,&edit,None)? {return Err(io::Error::other("Исчерпан сериализованный бюджет предложений (1 MiB)."));}
                        report.schema_version=report.schema_version.max(2);report.proposals.push(edit);Ok(serde_json::json!({"proposal_index":report.proposals.len()-1,"created":false}))
                    })
                    }
                }
                Action::ProposeEdit { path, old, new } => {
                    if role_spec.read_only() {
                        Err(io::Error::other("Подагенту запрещены предложения правок."))
                    } else {
                        let index = report.proposals.iter().position(|e| e.path == *path);
                        let edit = match index {
                            Some(index) => workspace.revise(&report.proposals[index], old, new),
                            None => workspace.propose(path, old, new),
                        };
                        edit.and_then(|edit| {
                            if index.is_none() && report.proposals.len() >= crate::workspace::MAX_PROPOSALS || !crate::task_report::proposal_fits(&report,&edit,index)? {
                                return Err(io::Error::other("Исчерпан сериализованный бюджет предложений (1 MiB)."));
                            }
                            let index = if let Some(index) = index {
                                report.proposals[index] = edit;
                                index
                            } else {
                                report.proposals.push(edit);
                                report.proposals.len() - 1
                            };
                            Ok(serde_json::json!({"proposal_index": index, "applied": false}))
                        })
                    }
                }
                Action::DiscardProposal { path } => {
                    if role_spec.read_only() {
                        Err(io::Error::other("Подагенту запрещено менять предложения."))
                    } else if let Some(index) = report.proposals.iter().position(|p| p.path == *path) {
                        report.proposals.remove(index);
                        Ok(serde_json::json!({"discarded": path, "writes_applied": false}))
                    } else {
                        Err(io::Error::other("Предложение для этого пути отсутствует."))
                    }
                }
                Action::RequestChanges { summary } => {
                    if !matches!(role_spec, AgentRole::Reviewer) || summary.trim().is_empty() {
                        Err(io::Error::other("Только reviewer может запросить исправления с описанием."))
                    } else {
                        crate::task_report::summarize(&mut report,role,summary).map(|()| {
                            changes_requested=true;finished=true;
                            serde_json::json!({"changes_requested":true,"tests_verified":false})
                        })
                    }
                }
                Action::Finish { summary } => {
                    crate::task_report::summarize(&mut report,role,summary).map(|()| {
                        finished=true;serde_json::json!({"finished":true})
                    })
                }
            };
            let executor_unavailable = matches!(action, Action::Verify)
                && outcome.as_ref().err().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        io::ErrorKind::Unsupported
                            | io::ErrorKind::NotFound
                            | io::ErrorKind::PermissionDenied
                    )
                });
            let (result, label) = match outcome {
                Ok(value)
                    if tool == "verify"
                        && (value["exit_code"] != 0
                            || value["timed_out"] != false
                            || value["cancelled"] != false
                            || value["cleanup_confirmed"] != true) =>
                {
                    (value, "check_failed".to_owned())
                }
                Ok(value) => (value, "ok".to_owned()),
                Err(e) => (
                    serde_json::json!({"error":e.to_string(),"error_code": match e.kind() {
                        io::ErrorKind::Unsupported => "unsupported", io::ErrorKind::PermissionDenied => "permission_denied",
                        io::ErrorKind::TimedOut => "timeout", io::ErrorKind::Interrupted => "cancelled",
                        io::ErrorKind::NotFound => "not_found", io::ErrorKind::InvalidInput => "invalid_input", _ => "failed" }}),
                    format!("denied_or_error: {e}"),
                ),
            };
            let success = label == "ok";
            let path = match &action {
                Action::ReadFile { path }
                | Action::ReadRange { path, .. }
                | Action::ProposeEdit { path, .. }
                | Action::ProposeFile { path, .. }
                | Action::DiscardProposal { path } => {
                    format!(" · {}", path.chars().take(128).collect::<String>())
                }
                _ => String::new(),
            };
            let _ = writeln!(
                io::stderr().lock(),
                "LYRA_PROGRESS:activity={}",
                serde_json::json!({"label":format!("{role} · {tool} · {}{path}",if success {"ok"} else {"error"})})
            );
            report.record_event(Event {
                call: report.calls,
                role: role.into(),
                tool: tool.into(),
                outcome: label,
                argument: match &action {
                    Action::ReadFile { path }
                    | Action::ReadRange { path, .. }
                    | Action::ProposeEdit { path, .. }
                    | Action::ProposeFile { path, .. }
                    | Action::DiscardProposal { path } => Some(path.chars().take(256).collect()),
                    Action::Search { query }
                    | Action::SearchPage { query, .. }
                    | Action::SearchDocs { query } => Some(query.chars().take(256).collect()),
                    _ => None,
                },
            });
            let artifact = match &action {
                Action::ReadFile { path }
                | Action::ReadRange { path, .. }
                | Action::ProposeEdit { path, .. }
                | Action::ProposeFile { path, .. }
                | Action::DiscardProposal { path } => Some(path.as_str()),
                _ => None,
            };
            if let Some(path) = artifact
                && (workspace.files.contains(path)
                    || report.proposals.iter().any(|p| p.path == path))
            {
                focus_paths.retain(|p| p != path);
                focus_paths.push(path.to_owned());
            }
            for path in result["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .chain(
                    result["matches"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v["path"].as_str()),
                )
            {
                focus_paths.retain(|p| p != path);
                focus_paths.push(path.to_owned());
            }
            if focus_paths.len() > 64 {
                focus_paths.drain(..focus_paths.len() - 64);
            }
            let proposal_exists =
                artifact.is_some_and(|path| report.proposals.iter().any(|p| p.path == path));
            let observation = crate::agent_runtime::tool_observation(
                role_spec,
                tool,
                success,
                result,
                artifact,
                proposal_exists,
            );
            // Only accepted proposal contents exist above. Keep rejected old/new for recovery.
            let recorded = match &action {
                Action::ProposeEdit { path, .. } | Action::ProposeFile { path, .. } if success => {
                    serde_json::json!({"tool":tool,"path":path,"content_in_proposals":true})
                }
                _ => serde_json::to_value(&action).unwrap_or(serde_json::Value::Null),
            };
            let signature = serde_json::json!({"action":recorded,"result":observation,"proposal_state":proposal_digest(&report.proposals).ok()}).to_string();
            if signature == last_observation {
                unchanged_steps += 1;
            } else {
                unchanged_steps = 1;
            }
            last_observation = signature;
            history.push(serde_json::json!({"action":recorded,"result":observation}));
            if let Some(save) = checkpoint
                && let Err(error) = save(&report)
            {
                report.status = format!("checkpoint_error: {error}");
                return report;
            }
            if let Some(reason) = stop_reason(model, deadline) {
                report.status = reason.into();
                return report;
            }
            if executor_unavailable {
                report.status = "executor_unavailable".into();
                return report;
            }
            if !finished && unchanged_steps >= 4 {
                report.status = format!("no_progress: {role}");
                return report;
            }
            if history.len() > 4 {
                history.remove(0);
            }
            if finished {
                break;
            }
        }
        if !finished {
            report.status = format!("step_limit: {role}");
            return report;
        }
        if changes_requested {
            let remaining = max_calls.saturating_sub(report.calls);
            if remaining < 2 || repair_rounds >= 2 {
                report.status = "changes_requested".into();
                return report;
            }
            repair_rounds += 1;
            roles.push_back((AgentRole::Coder, remaining - 1));
            roles.push_back((AgentRole::Reviewer, 1));
        }
    }
    let verified = verification_matches(&report, check, workspace);
    if let Some(reason) = stop_reason(model, deadline) {
        report.status = reason.into();
        return report;
    }
    if check.is_some() && !verified {
        report.status = "verification_required".into();
        return report;
    }
    report.status = "completed".into();
    report.outcome = if verified {
        TaskOutcome::ChangesVerified
    } else if report.proposals.is_empty() && report.file_proposals.is_empty() {
        TaskOutcome::AnalysisComplete
    } else {
        TaskOutcome::ChangesProposed
    };
    report
}

fn stop_reason(model: &impl Model, deadline: Option<std::time::Instant>) -> Option<&'static str> {
    if model
        .cancellation()
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        Some("cancelled")
    } else if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
        Some("timeout")
    } else {
        None
    }
}

fn verification_matches(
    report: &Report,
    check: Option<&crate::executor::ExecutionRequest>,
    workspace: &Workspace,
) -> bool {
    if !report.file_proposals.is_empty() {
        return false;
    }
    report.verifications.last().is_some_and(|evidence| {
        evidence.result.passed()
            && proposal_digest(&report.proposals)
                .is_ok_and(|digest| digest == evidence.proposal_sha256)
            && check.is_none_or(|check| {
                value_digest(check).is_ok_and(|digest| digest == evidence.check_sha256)
            })
            && evidence
                .result
                .source_sha256
                .as_ref()
                .is_some_and(|expected| {
                    crate::executor::checked_source_hash(
                        &workspace.root,
                        &report.verification_scope,
                        &report.proposals,
                        false,
                    )
                    .is_ok_and(|current| &current == expected)
                })
    })
}

fn proposal_digest(proposals: &[Edit]) -> io::Result<String> {
    value_digest(&proposals)
}

fn value_digest(value: &impl Serialize) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn limited_file(path: &std::path::Path, limit: usize) -> io::Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)?
        .take((limit + 1) as u64)
        .read_to_string(&mut text)?;
    if text.len() > limit {
        return Err(io::Error::other("Файл превышает лимит размера."));
    }
    Ok(text)
}

struct SkilledModel<'a, M: Model> {
    inner: &'a mut M,
    guidance: &'a str,
}

impl<M: Model> SkilledModel<'_, M> {
    fn augmented(&self, prompt: &str) -> io::Result<String> {
        if self.guidance.is_empty() {
            return Ok(prompt.to_owned());
        }
        let mut envelope: serde_json::Value =
            serde_json::from_str(prompt).map_err(io::Error::other)?;
        envelope["ecc_skill_guidance"] = serde_json::Value::String(self.guidance.into());
        Ok(envelope.to_string())
    }
}
impl<M: Model> Model for SkilledModel<'_, M> {
    fn cancellation(&self) -> &std::sync::atomic::AtomicBool {
        self.inner.cancellation()
    }
    fn input_limit(&self) -> usize {
        self.inner.input_limit()
    }
    fn accounting(&self) -> &'static str {
        self.inner.accounting()
    }
    fn count_prompt(&self, prompt: &str) -> io::Result<usize> {
        self.inner.count_prompt(&self.augmented(prompt)?)
    }
    fn respond(&mut self, prompt: &str) -> io::Result<String> {
        self.inner.respond(&self.augmented(prompt)?)
    }
}

#[cfg(feature = "gateway-http")]
struct CloudTaskModel {
    input_limit: usize,
    backend: crate::provider_http::OpenAiCompatibleBackend,
    max_tokens: u32,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "gateway-http")]
impl Model for CloudTaskModel {
    fn input_limit(&self) -> usize {
        self.input_limit
    }
    fn accounting(&self) -> &'static str {
        "utf8_bytes_div3_estimate"
    }
    fn count_prompt(&self, prompt: &str) -> io::Result<usize> {
        Ok((prompt.len() + crate::agent_runtime::TASK_SYSTEM_PROMPT.len()).div_ceil(3) + 16)
    }
    fn cancellation(&self) -> &std::sync::atomic::AtomicBool {
        &self.cancelled
    }
    fn respond(&mut self, prompt: &str) -> io::Result<String> {
        use crate::model_gateway::{Message, Request, Role};
        let request = Request {
            messages: vec![
                Message {
                    role: Role::System,
                    content: crate::agent_runtime::TASK_SYSTEM_PROMPT.into(),
                },
                Message {
                    role: Role::User,
                    content: prompt.to_owned(),
                },
            ],
            max_output_tokens: self.max_tokens,
            require_tools: false,
        };
        crate::agent_runtime::complete(&mut self.backend, &request, None, &[])
            .map(|response| response.text)
    }
}

#[cfg(feature = "gateway-http")]
fn cloud_task_model(
    args: &TaskArgs,
    started: std::time::Instant,
) -> io::Result<Option<CloudTaskModel>> {
    use std::io::Read;
    let Some(path) = &args.provider_config else {
        return Ok(None);
    };
    if args.allowed_providers.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Cloud task requires --allow PROVIDER",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let configs = crate::provider_http::parse_registry(&bytes)?;
    let config = configs
        .into_iter()
        .find(|config| {
            args.allowed_providers.contains(&config.descriptor.provider)
                && args
                    .cloud_model
                    .as_ref()
                    .is_none_or(|model| model == &config.descriptor.model)
                && config.descriptor.capabilities.max_context_tokens >= args.max_tokens
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "No permitted compatible cloud model",
            )
        })?;
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))
        .map_err(io::Error::other)?;
    let input_limit = config
        .descriptor
        .capabilities
        .max_context_tokens
        .min(args.context)
        .saturating_sub(args.max_tokens) as usize;
    Ok(Some(CloudTaskModel {
        input_limit,
        cancelled,
        backend: crate::provider_http::OpenAiCompatibleBackend::new(config)?
            .with_deadline(started + std::time::Duration::from_secs(args.timeout_secs)),
        max_tokens: args.max_tokens,
    }))
}

fn execution_session(
    args: &TaskArgs,
) -> io::Result<Option<crate::universal::execution::ExecutionSession>> {
    args.environments
        .as_ref()
        .map(|p| {
            let root = args.artifacts_dir.as_ref().ok_or_else(|| {
                io::Error::other("--artifacts-dir is required for isolated execution")
            })?;
            let mut execution = crate::universal::execution::ExecutionSession::new(
                crate::universal::environment::Registry::load(p)?,
                args.allowed_environments.clone(),
                root,
            )?;
            if args
                .repo
                .canonicalize()?
                .join(".lyra/artifact-objects")
                .canonicalize()
                .ok()
                .as_ref()
                == Some(&execution.store.root)
            {
                return Err(io::Error::other(
                    "Choose an artifact store outside reserved publication backup storage",
                ));
            }
            execution.deadline =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(args.timeout_secs));
            Ok(execution)
        })
        .transpose()
}

pub fn task(args: TaskArgs, output: &mut impl Write) -> io::Result<()> {
    let started = std::time::Instant::now();
    let task = limited_file(&args.task_file, 16384)?;
    if task.trim().is_empty() || (args.subagents && args.max_calls < 3) {
        return Err(io::Error::other(
            "Пустое задание или недостаточный бюджет для подагентов.",
        ));
    }
    let original_root = args.repo.canonicalize()?;
    let check = args
        .check_config
        .as_ref()
        .map(|path| crate::executor::load_config(path))
        .transpose()?;
    let mut store = args
        .session_db
        .as_ref()
        .map(|path| crate::session::SessionStore::open(path))
        .transpose()?;
    let session_id = if let Some(store) = &mut store {
        let id = if let Some(id) = &args.resume {
            if !store.exists(id)? {
                return Err(io::Error::other("Session does not exist"));
            }
            id.clone()
        } else {
            store.create(&args.repo, &args.files, &task)?
        };
        eprintln!("Lyra session: {id}");
        Some(id)
    } else {
        None
    };
    let mut snapshot = if let (Some(store), Some(id)) = (&store, &session_id) {
        Some(store.materialize(id)?)
    } else {
        None
    };
    if let Some(snapshot) = &snapshot
        && (snapshot.task != task || snapshot.original_root != original_root)
    {
        return Err(io::Error::other(
            "Resume requires the original project and unchanged task file",
        ));
    }
    let mut workspace = if let Some(snapshot) = &snapshot {
        Workspace::open(snapshot.directory.path(), &[])?
    } else {
        Workspace::open(&args.repo, &args.files)?
    };
    let mut previous = snapshot
        .as_mut()
        .and_then(|snapshot| snapshot.report.take());
    let checkpoint = |report: &Report| -> io::Result<()> {
        if let (Some(store), Some(id)) = (&store, &session_id) {
            store.checkpoint(id, report)?;
        }
        Ok(())
    };
    let docs = match &args.docs {
        Some(path) => crate::knowledge::Pack::load(path)?,
        None => crate::knowledge::Pack::empty(),
    };
    let skill_pack = args
        .skills
        .as_ref()
        .map(|path| crate::skills::SkillPack::load_source(path))
        .transpose()?;
    let skill_guidance =
        crate::agent_runtime::skill_instructions(skill_pack.as_ref(), &args.selected_skills)?;
    #[cfg(feature = "gateway-http")]
    if let Some(mut cloud) = cloud_task_model(&args, started)? {
        let mut model = SkilledModel {
            inner: &mut cloud,
            guidance: &skill_guidance,
        };
        let mut model = crate::universal::execution::ExecutionModel {
            inner: &mut model,
            execution: execution_session(&args)?,
        };
        let mut report = execute_workflow(
            &mut model,
            &mut workspace,
            &task,
            WorkflowOptions {
                subagents: args.subagents,
                max_calls: args.max_calls,
                docs: &docs,
                dedup_tool_context: args.dedup_tool_context,
                check: check.as_ref(),
                deadline: Some(started + std::time::Duration::from_secs(args.timeout_secs)),
                previous: previous.take(),
                checkpoint: Some(&checkpoint),
            },
        );
        checkpoint(&report)?;
        if let Some(snapshot) = &snapshot {
            report.workspace = snapshot.original_root.clone();
        }
        serde_json::to_writer_pretty(&mut *output, &report).map_err(io::Error::other)?;
        writeln!(output)?;
        output.flush()?;
        if report.status != "completed" {
            let kind = match report.status.as_str() {
                "cancelled" => io::ErrorKind::Interrupted,
                "timeout" => io::ErrorKind::TimedOut,
                _ => io::ErrorKind::Other,
            };
            return Err(io::Error::new(kind, report.status));
        }
        return Ok(());
    }
    let model_path = args.model.clone().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Specify --model or --provider-config",
        )
    })?;
    let config = GenerateArgs {
        model: model_path,
        prompt: "task session".into(),
        skills: None,
        selected_skills: Vec::new(),
        context: args.context,
        max_tokens: args.max_tokens,
        threads: args.threads,
        gpu_layers: args.gpu_layers,
        timeout_secs: args.timeout_secs,
        raw: args.raw,
    };
    inference::with_session(&config, |session| {
        let mut session = session.clone();
        let mut model = SkilledModel {
            inner: &mut session,
            guidance: &skill_guidance,
        };
        let mut model = crate::universal::execution::ExecutionModel {
            inner: &mut model,
            execution: execution_session(&args)?,
        };
        let mut report = execute_workflow(
            &mut model,
            &mut workspace,
            &task,
            WorkflowOptions {
                subagents: args.subagents,
                max_calls: args.max_calls,
                docs: &docs,
                dedup_tool_context: args.dedup_tool_context,
                check: check.as_ref(),
                deadline: Some(started + std::time::Duration::from_secs(args.timeout_secs)),
                previous: previous.take(),
                checkpoint: Some(&checkpoint),
            },
        );
        checkpoint(&report)?;
        if let Some(snapshot) = &snapshot {
            report.workspace = snapshot.original_root.clone();
        }
        serde_json::to_writer_pretty(&mut *output, &report).map_err(io::Error::other)?;
        writeln!(output)?;
        output.flush()?;
        if report.status != "completed" {
            let kind = match report.status.as_str() {
                "cancelled" => io::ErrorKind::Interrupted,
                "timeout" => io::ErrorKind::TimedOut,
                _ => io::ErrorKind::Other,
            };
            return Err(io::Error::new(kind, report.status));
        }
        Ok(())
    })
}

pub fn apply(args: ApplyArgs, output: &mut impl Write) -> io::Result<()> {
    let report: Report = serde_json::from_str(&limited_file(&args.report, 2 * 1024 * 1024)?)
        .map_err(io::Error::other)?;
    if report.schema_version == 5 && !report.file_proposals.is_empty() {
        if report.workspace != args.repo.canonicalize()?
            || report.status != "completed"
            || !matches!(
                report.outcome,
                TaskOutcome::ChangesProposed | TaskOutcome::ChangesVerified
            )
        {
            return Err(io::Error::other("Incomplete report or wrong workspace"));
        }
        let store = crate::universal::artifacts::Store::open(
            report
                .artifact_store
                .as_ref()
                .ok_or_else(|| io::Error::other("Artifact store missing"))?,
        )?;
        let mut files = report.file_proposals.clone();
        for text in &report.proposals {
            files.push(crate::universal::artifacts::FileProposal {
                path: text.path.clone(),
                original: if text.create {
                    None
                } else {
                    Some(store.put(text.original.as_bytes())?)
                },
                replacement: store.put(text.replacement.as_bytes())?,
                inputs: Vec::new(),
            });
        }
        // Index order is text proposals first in CLI and Desktop.
        files.rotate_right(report.proposals.len());
        let selected = if args.all {
            files.as_slice()
        } else {
            std::slice::from_ref(
                files
                    .get(
                        args.index
                            .ok_or_else(|| io::Error::other("Index required"))?,
                    )
                    .ok_or_else(|| io::Error::other("Index missing"))?,
            )
        };
        crate::universal::artifacts::publish(&args.repo, &store, selected, || Ok(()))?;
        return writeln!(output, "Применено предложений: {}", selected.len());
    }
    let edits = if args.all {
        report.proposals.as_slice()
    } else {
        let index = args
            .index
            .ok_or_else(|| io::Error::other("Укажите --index или --all."))?;
        std::slice::from_ref(
            report
                .proposals
                .get(index)
                .ok_or_else(|| io::Error::other("Предложение не найдено."))?,
        )
    };
    if !matches!(report.schema_version, 1..=4)
        || report.schema_version == 1 && edits.iter().any(|edit| edit.create)
        || report.status != "completed"
        || report.schema_version == 4
            && !matches!(
                report.outcome,
                TaskOutcome::ChangesProposed | TaskOutcome::ChangesVerified
            )
        || report.workspace != args.repo.canonicalize()?
    {
        return Err(io::Error::other("Неполный отчёт или другой проект."));
    }
    crate::publication::apply_checked(&args.repo, edits, || {
        if report.outcome == TaskOutcome::ChangesVerified
            && let Some(expected) = report
                .verifications
                .last()
                .and_then(|v| v.result.source_sha256.as_ref())
            && !crate::executor::checked_source_hash(
                &args.repo,
                &report.verification_scope,
                &report.proposals,
                true,
            )
            .is_ok_and(|actual| &actual == expected)
        {
            return Err(io::Error::other(
                "Исходники изменились после проверки. Повторите задачу/проверку; публикация остановлена.",
            ));
        }

        Ok(())
    })?;
    writeln!(output, "Применено предложений: {}", edits.len())
}

#[cfg(test)]
mod skill_adapter_tests {
    use super::*;

    struct Capture {
        prompts: Vec<String>,
    }

    impl Model for Capture {
        fn respond(&mut self, prompt: &str) -> io::Result<String> {
            self.prompts.push(prompt.to_owned());
            Ok(r#"{"tool":"finish","summary":"reviewed"}"#.into())
        }
    }

    #[test]
    fn ecc_guidance_reaches_researcher_coder_and_reviewer_without_changing_roles() {
        let mut capture = Capture {
            prompts: Vec::new(),
        };
        let guidance = "Check Rust ownership and propose precise edits";
        {
            let mut model = SkilledModel {
                inner: &mut capture,
                guidance,
            };
            for role in ["researcher", "coder", "reviewer"] {
                let prompt = serde_json::json!({
                    "role": role,
                    "read_only": role != "coder",
                    "files": ["src/lib.rs"]
                })
                .to_string();
                model.respond(&prompt).unwrap();
            }
        }
        assert_eq!(capture.prompts.len(), 3);
        for (prompt, role) in capture
            .prompts
            .iter()
            .zip(["researcher", "coder", "reviewer"])
        {
            let value: serde_json::Value = serde_json::from_str(prompt).unwrap();
            assert_eq!(value["role"], role);
            assert_eq!(value["read_only"], role != "coder");
            assert_eq!(value["ecc_skill_guidance"], guidance);
        }
    }

    #[cfg(feature = "gateway-http")]
    #[test]
    fn cloud_task_sends_role_envelope_and_rejects_invalid_provider_response() {
        use crate::model_gateway::{ModelCapabilities, ModelDescriptor};
        use crate::provider_http::{OpenAiCompatibleBackend, ProviderConfig};
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        for (status, body, expected_error) in [
            (
                "200 OK",
                r#"{"choices":[{"message":{"content":"{\"tool\":\"finish\",\"summary\":\"ok\"}"}}]}"#,
                None,
            ),
            (
                "401 Unauthorized",
                "{}",
                Some(io::ErrorKind::PermissionDenied),
            ),
            (
                "503 Service Unavailable",
                "{}",
                Some(io::ErrorKind::TimedOut),
            ),
            (
                "200 OK",
                r#"{"choices":[]}"#,
                Some(io::ErrorKind::InvalidData),
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0u8; 8192];
                let size = stream.read(&mut request).unwrap();
                let request_text = String::from_utf8_lossy(&request[..size]);
                assert!(request_text.contains("POST /v1/chat/completions"));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let config = ProviderConfig {
                descriptor: ModelDescriptor {
                    provider: "mock".into(),
                    model: "mock-model".into(),
                    capabilities: ModelCapabilities {
                        tool_calls: false,
                        vision: false,
                        prompt_cache: false,
                        max_context_tokens: 4096,
                    },
                },
                endpoint: format!("http://{address}/v1/chat/completions"),
                api_key_env: None,
            };
            let mut model = CloudTaskModel {
                input_limit: 4096,
                cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                backend: OpenAiCompatibleBackend::new(config).unwrap(),
                max_tokens: 128,
            };
            let result = model.respond(r#"{"role":"coder","task":"inspect"}"#);
            match expected_error {
                Some(kind) => assert_eq!(result.unwrap_err().kind(), kind),
                None => assert!(result.unwrap().contains("finish")),
            }
            thread.join().unwrap();
        }
    }

    #[test]
    fn unselected_skill_does_not_modify_task_envelope() {
        let mut capture = Capture {
            prompts: Vec::new(),
        };
        let mut model = SkilledModel {
            inner: &mut capture,
            guidance: "",
        };
        let prompt = r#"{"role":"coder","files":[]}"#;
        model.respond(prompt).unwrap();
        assert_eq!(capture.prompts[0], prompt);
    }
}
