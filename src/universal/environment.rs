//! Manifests are trusted user configuration, never repository/model permissions.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet, io, path::Path, process::Command, sync::atomic::AtomicBool,
    time::Duration,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub id: String,
    pub description: String,
    pub image: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub script_extension: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub documentation: Vec<String>,
    #[serde(default = "memory")]
    pub memory_mib: u32,
    #[serde(default = "timeout")]
    pub timeout_secs: u64,
}
fn memory() -> u32 {
    2048
}
fn timeout() -> u64 {
    120
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema_version: u32,
    pub environments: Vec<Environment>,
}
impl Registry {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = crate::knowledge::read_text(path, 256 * 1024)?;
        let value: Self = serde_json::from_str(&text).map_err(io::Error::other)?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> io::Result<()> {
        let mut seen = BTreeSet::new();
        if self.schema_version != 1 || self.environments.is_empty() || self.environments.len() > 64
        {
            return Err(io::Error::other(
                "Expected environment registry v1 with 1..64 entries",
            ));
        }
        for e in &self.environments {
            if !valid_id(&e.id)
                || !seen.insert(&e.id)
                || e.description.len() > 2048
                || e.capabilities.len() > 64
                || e.capabilities
                    .iter()
                    .any(|s| s.len() > 128 || s.chars().any(char::is_control))
                || e.script_extension.len() > 16
                || !e
                    .script_extension
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric())
                || e.script_extension.is_empty()
                || e.documentation.len() > 16
                || e.documentation.iter().any(|s| {
                    !s.starts_with("https://") || s.len() > 2048 || s.chars().any(char::is_control)
                })
            {
                return Err(io::Error::other("Invalid environment manifest"));
            }
            e.request().validate()?;
        }
        Ok(())
    }
    pub fn get(&self, id: &str) -> io::Result<&Environment> {
        self.environments
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Environment is not registered"))
    }
}
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
impl Environment {
    pub fn request(&self) -> crate::executor::ExecutionRequest {
        crate::executor::ExecutionRequest {
            image: self.image.clone(),
            program: self.program.clone(),
            args: self.args.clone(),
            memory_mib: self.memory_mib,
            timeout_secs: self.timeout_secs,
        }
    }
    pub fn inspect(&self, cancel: &AtomicBool) -> io::Result<serde_json::Value> {
        let mut c = Command::new("docker");
        c.args(["image", "inspect", "--format", "{{json .Os}}", &self.image]);
        let r = crate::executor::capture(c, Duration::from_secs(10), cancel)?;
        Ok(
            serde_json::json!({"environment":self,"state":if r.exit_code!=Some(0)||r.cancelled||r.timed_out{"preparation_required"}else if r.stdout.trim()!= "\"linux\""{"incompatible"}else{"available"},"image_present":r.exit_code==Some(0),"execution_requires_resource_preflight":true}),
        )
    }
}
#[derive(Debug, clap::Args)]
pub struct EnvironmentArgs {
    #[arg(long)]
    pub registry: std::path::PathBuf,
    #[command(subcommand)]
    pub command: EnvironmentCommand,
}
#[derive(Debug, clap::Subcommand)]
pub enum EnvironmentCommand {
    /// Build the bundled document environment; --registry is the output manifest.
    BuildDocuments {
        #[arg(long)]
        accept: bool,
        #[arg(long)]
        proxy_ca: Option<std::path::PathBuf>,
    },
    List,
    Inspect {
        id: String,
    },
    Prepare {
        id: String,
        #[arg(long)]
        accept: bool,
    },
}
pub fn run(args: EnvironmentArgs, out: &mut impl io::Write) -> io::Result<()> {
    if let EnvironmentCommand::BuildDocuments { accept, proxy_ca } = args.command {
        return build_documents(&args.registry, accept, proxy_ca.as_deref(), out);
    }
    let registry = Registry::load(&args.registry)?;
    let cancel = AtomicBool::new(false);
    let result = match args.command {
        EnvironmentCommand::BuildDocuments { .. } => {
            return Err(io::Error::other("Unexpected build dispatch"));
        }
        EnvironmentCommand::List => serde_json::to_value(&registry).map_err(io::Error::other)?,
        EnvironmentCommand::Inspect { id } => registry.get(&id)?.inspect(&cancel)?,
        EnvironmentCommand::Prepare { id, accept } => {
            let e = registry.get(&id)?;
            if e.image.starts_with("sha256:") {
                let state = e.inspect(&cancel)?;
                if state["image_present"] != true {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Local content-addressed image missing; rebuild/import the environment",
                    ));
                }
                state
            } else if !accept {
                serde_json::json!({"approval_required":true,"action":"docker pull","image":e.image,"environment":e.id,"download_size":"reported by Docker during explicit preparation","host_installation":false})
            } else {
                let mut c = Command::new("docker");
                c.args(["pull", &e.image]);
                let r = crate::executor::capture(c, Duration::from_secs(600), &cancel)?;
                if r.exit_code != Some(0) || r.timed_out || r.cancelled {
                    return Err(io::Error::other("Environment preparation failed"));
                }
                e.inspect(&cancel)?
            }
        }
    };
    serde_json::to_writer_pretty(&mut *out, &result).map_err(io::Error::other)?;
    writeln!(out)
}

fn build_documents(
    destination: &Path,
    accept: bool,
    proxy_ca: Option<&Path>,
    out: &mut impl io::Write,
) -> io::Result<()> {
    const RECIPE: &str = include_str!("../../tools/documents/Dockerfile");
    if !accept {
        return writeln!(
            out,
            "{}",
            serde_json::json!({"approval_required":true,"action":"build document environment","base_image":"python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1","recipe":RECIPE,"recipe_sha256":crate::retrieval::sha256(RECIPE.as_bytes()),"estimated_disk":"several GiB for Office/OCR tools; Docker reports actual usage","registry_destination":destination})
        );
    }
    if destination.exists() {
        return Err(io::Error::other(
            "Choose a new registry path; existing manifests are preserved",
        ));
    }
    let temp = crate::workspace::private_tempdir("lyra-document-build-")?;
    for (name, content) in [
        ("Dockerfile", RECIPE),
        (
            "requirements.lock",
            include_str!("../../tools/documents/requirements.lock"),
        ),
        (
            "lyra_documents.py",
            include_str!("../../tools/documents/lyra_documents.py"),
        ),
    ] {
        std::fs::write(temp.path().join(name), content)?;
    }
    let iid = temp.path().join("image-id");
    let mut c = Command::new("docker");
    c.args(["build", "--iidfile"]).arg(&iid);
    if let Some(ca) = proxy_ca {
        if !ca.is_file() {
            return Err(io::Error::other("CA bundle missing"));
        }
        c.arg("--secret")
            .arg(format!("id=proxy_ca,src={}", ca.display()));
    }
    c.arg(temp.path());
    let r = crate::executor::capture(c, Duration::from_secs(1800), &AtomicBool::new(false))?;
    if r.exit_code != Some(0) || r.timed_out {
        return Err(io::Error::other(format!(
            "Document image build failed: {}",
            r.stderr
        )));
    }
    let image = std::fs::read_to_string(iid)?.trim().to_owned();
    let registry=Registry{schema_version:1,environments:vec![Environment{id:"documents".into(),description:"Office/PDF: import lyra_documents; inspect(path,start,count), create(path,spec), edit(source,destination,operations), validate(path), render(source,destination), ocr(path,start,count). Inputs /source; deliverables /output. Inspect and declare all outputs, including previews. Structure/render checks do not establish semantic correctness.".into(),image,program:"python".into(),args:vec![],script_extension:"py".into(),capabilities:vec!["python".into(),"docx".into(),"xlsx".into(),"pptx".into(),"pdf".into(),"ocr-rus-eng".into()],documentation:vec![],memory_mib:2048,timeout_secs:180}]};
    registry.validate()?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    serde_json::to_writer_pretty(&mut f, &registry).map_err(io::Error::other)?;
    f.sync_all()?;
    serde_json::to_writer_pretty(&mut *out, &registry).map_err(io::Error::other)?;
    writeln!(out)
}
