//! Provider-neutral contracts for the future Lyra Model Gateway.
//! No network I/O, credential handling, or implicit fallback lives here.
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role { System, Developer, User, Assistant, Tool }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub tool_calls: bool,
    pub vision: bool,
    pub prompt_cache: bool,
    pub max_context_tokens: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDescriptor {
    pub provider: String,
    pub model: String,
    pub capabilities: ModelCapabilities,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub messages: Vec<Message>,
    pub max_output_tokens: u32,
    pub require_tools: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub provider: String,
    pub model: String,
    pub text: String,
    pub usage: Usage,
}

pub trait ModelBackend {
    fn descriptor(&self) -> &ModelDescriptor;
    fn complete(&mut self, request: &Request) -> io::Result<Response>;
}

pub fn validate_request(model: &ModelDescriptor, request: &Request) -> io::Result<()> {
    if request.messages.is_empty() || request.messages.iter().all(|m| m.content.trim().is_empty()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Empty model request"));
    }
    if request.max_output_tokens == 0 || request.max_output_tokens > model.capabilities.max_context_tokens {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid output budget"));
    }
    if request.require_tools && !model.capabilities.tool_calls {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "Model does not support tools"));
    }
    Ok(())
}

/// A route is selected only from explicitly permitted providers.
/// Routing cannot silently opt into sending data to a cloud provider.
pub fn select_priority<'a>(
    models: &'a [ModelDescriptor],
    allowed_providers: &[String],
    request: &Request,
) -> io::Result<&'a ModelDescriptor> {
    models.iter().find(|model| {
        allowed_providers.iter().any(|p| p == &model.provider)
            && validate_request(model, request).is_ok()
    }).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No permitted compatible model"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model(provider: &str, tools: bool) -> ModelDescriptor {
        ModelDescriptor {
            provider: provider.into(), model: "test".into(),
            capabilities: ModelCapabilities { tool_calls: tools, vision: false,
                prompt_cache: false, max_context_tokens: 4096 },
        }
    }
    fn request() -> Request {
        Request { messages: vec![Message { role: Role::User, content: "fix bug".into() }],
            max_output_tokens: 256, require_tools: true }
    }
    #[test]
    fn route_never_uses_unapproved_cloud() {
        let models = vec![model("cloud", true), model("local", true)];
        let chosen = select_priority(&models, &["local".into()], &request()).unwrap();
        assert_eq!(chosen.provider, "local");
        assert!(select_priority(&models, &[], &request()).is_err());
    }
    #[test]
    fn capabilities_and_budgets_are_enforced() {
        assert!(validate_request(&model("local", false), &request()).is_err());
        let mut req = request();
        req.max_output_tokens = 5000;
        assert!(validate_request(&model("local", true), &req).is_err());
        req.max_output_tokens = 256;
        assert!(validate_request(&model("local", true), &req).is_ok());
    }
}
