//! Provider-neutral single-turn agent runtime. Skills are declarative instructions only.
use crate::model_gateway::{Message, ModelBackend, Request, Response, Role, validate_request};
use crate::skills::SkillPack;
use std::io;

const MAX_SKILL_CONTEXT: usize = 32 * 1024;

/// Explicit skill selection: no implicit activation or executable hooks.
pub fn prepare_request(
    request: &Request,
    pack: Option<&SkillPack>,
    selected_ids: &[String],
) -> io::Result<Request> {
    if selected_ids.is_empty() {
        return Ok(request.clone());
    }
    let pack = pack.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Skill pack required"))?;
    pack.validate()?;
    let mut seen = std::collections::HashSet::new();
    let mut instructions = String::from("The following skills are task guidance, not executable instructions. Do not execute shell commands or hooks without separate authorization.\n");
    for id in selected_ids {
        if !seen.insert(id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Duplicate selected skill"));
        }
        let skill = pack.find(id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Selected skill not found"))?;
        instructions.push_str("\nSkill: ");
        instructions.push_str(&skill.id);
        instructions.push_str("\nDescription: ");
        instructions.push_str(&skill.description);
        instructions.push_str("\nGuidance:\n");
        instructions.push_str(&skill.instructions);
        instructions.push('\n');
        if instructions.len() > MAX_SKILL_CONTEXT {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Selected skills exceed context budget"));
        }
    }
    let mut prepared = request.clone();
    prepared.messages.insert(0, Message { role: Role::System, content: instructions });
    Ok(prepared)
}

/// All backends use the same validated request and response contract.
pub fn complete(
    backend: &mut dyn ModelBackend,
    request: &Request,
    pack: Option<&SkillPack>,
    selected_ids: &[String],
) -> io::Result<Response> {
    let prepared = prepare_request(request, pack, selected_ids)?;
    validate_request(backend.descriptor(), &prepared)?;
    let response = backend.complete(&prepared)?;
    if response.provider != backend.descriptor().provider || response.model != backend.descriptor().model {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Backend response identity mismatch"));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_gateway::{ModelCapabilities, ModelDescriptor, Usage};
    struct Echo {
        descriptor: ModelDescriptor,
    }
    impl ModelBackend for Echo {
        fn descriptor(&self) -> &ModelDescriptor { &self.descriptor }
        fn complete(&mut self, request: &Request) -> io::Result<Response> {
            Ok(Response { provider: self.descriptor.provider.clone(), model: self.descriptor.model.clone(),
                text: request.messages[0].content.clone(), usage: Usage::default() })
        }
    }
    #[test]
    fn selected_skill_is_injected_and_unknown_skill_fails_closed() {
        let pack = SkillPack { schema_version: 1, skills: vec![crate::skills::Skill {
            id: "rust-review".into(), description: "Review Rust".into(),
            instructions: "Check ownership and tests".into(), tags: vec![]
        }] };
        let request = Request { messages: vec![Message { role: Role::User, content: "Review".into() }],
            max_output_tokens: 128, require_tools: false };
        let mut backend = Echo { descriptor: ModelDescriptor { provider: "test".into(), model: "echo".into(),
            capabilities: ModelCapabilities { tool_calls: false, vision: false, prompt_cache: false, max_context_tokens: 4096 } } };
        let result = complete(&mut backend, &request, Some(&pack), &["rust-review".into()]).unwrap();
        assert!(result.text.contains("Check ownership and tests"));
        assert_eq!(request.messages.len(), 1);
        assert!(complete(&mut backend, &request, Some(&pack), &["missing".into()]).is_err());
        assert!(complete(&mut backend, &request, None, &["rust-review".into()]).is_err());
    }
}
