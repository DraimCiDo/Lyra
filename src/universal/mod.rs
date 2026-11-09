//! Open technology choices and explicit execution capabilities.
pub mod artifacts;
pub mod environment;
pub mod execution;
use serde::{Deserialize, Serialize};
use std::io;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Technology {
    pub name: String,
    pub purpose: String,
    pub rationale: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkPlan {
    pub schema_version: u32,
    pub goal: String,
    #[serde(default)]
    pub technologies: Vec<Technology>,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
}
impl WorkPlan {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1
            || self.goal.trim().is_empty()
            || self.goal.len() > 16384
            || self.technologies.len() > 16
            || serde_json::to_vec(self).map_err(io::Error::other)?.len() > 64 * 1024
        {
            return Err(io::Error::other("Invalid or oversized WorkPlan"));
        }
        for t in &self.technologies {
            if t.name.trim().is_empty()
                || t.name.len() > 128
                || t.purpose.len() > 1024
                || t.rationale.len() > 2048
                || t.name.chars().any(char::is_control)
            {
                return Err(io::Error::other("Invalid technology description"));
            }
        }
        for list in [
            &self.inputs,
            &self.outputs,
            &self.constraints,
            &self.steps,
            &self.required_capabilities,
            &self.acceptance,
        ] {
            if list.len() > 64 || list.iter().any(|s| s.len() > 4096) {
                return Err(io::Error::other("WorkPlan list exceeds limits"));
            }
        }
        for p in self.inputs.iter().chain(&self.outputs) {
            crate::project::validate_path(p, false)?;
        }
        Ok(())
    }
}
pub const SELECTION_POLICY: &str = "Choose technologies from explicit user requirements, existing compatible stack, desired artifact, deployment/platform, library quality, performance and available environments, in that order. Preserve existing stacks unless migration is requested. Any language, multiple languages, a document operation or no-code solution is permitted. Presets are examples, not a language allowlist. Explain choices and material alternatives. Missing runtimes require explicit environment preparation; never silently change requirements or permissions.";
