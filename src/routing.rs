//! Provider routing and deterministic failover, without implicit cloud access.
use crate::model_gateway::{
    ModelBackend, ModelDescriptor, Request, Response, select_priority, validate_request,
};
use std::collections::HashSet;
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutingPolicy {
    /// Explicitly approved providers; empty means no requests are allowed.
    pub allowed_providers: Vec<String>,
    /// Maximum distinct backends attempted for a single request.
    pub max_attempts: usize,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            allowed_providers: Vec::new(),
            max_attempts: 1,
        }
    }
}

/// Executes the first eligible backend, then fails over only for transient I/O
/// errors. Invalid requests, permission errors and malformed responses fail
/// closed. The selected backend's identity must match its declared descriptor.
pub fn complete_with_fallback(
    backends: &mut [&mut dyn ModelBackend],
    policy: &RoutingPolicy,
    request: &Request,
) -> io::Result<Response> {
    if policy.max_attempts == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Routing attempts must be positive",
        ));
    }
    let descriptors: Vec<ModelDescriptor> = backends
        .iter()
        .map(|backend| backend.descriptor().clone())
        .collect();
    select_priority(&descriptors, &policy.allowed_providers, request)?;
    let mut seen = HashSet::new();
    let mut attempts = 0;
    let mut last_error = None;
    for backend in backends.iter_mut() {
        let descriptor = backend.descriptor().clone();
        if !policy
            .allowed_providers
            .iter()
            .any(|provider| provider == &descriptor.provider)
            || validate_request(&descriptor, request).is_err()
            || !seen.insert((descriptor.provider.clone(), descriptor.model.clone()))
        {
            continue;
        }
        if attempts >= policy.max_attempts {
            break;
        }
        attempts += 1;
        match backend.complete(request) {
            Ok(response) => {
                if response.provider != descriptor.provider || response.model != descriptor.model {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Backend returned a different provider or model",
                    ));
                }
                return Ok(response);
            }
            Err(error) if retryable(error.kind()) => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "No compatible permitted backend")
    }))
}

fn retryable(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::TimedOut
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_gateway::{Message, ModelCapabilities, Role, Usage};

    struct Stub {
        descriptor: ModelDescriptor,
        error: Option<io::ErrorKind>,
        calls: usize,
    }

    impl Stub {
        fn new(provider: &str, error: Option<io::ErrorKind>) -> Self {
            Self {
                descriptor: ModelDescriptor {
                    provider: provider.into(),
                    model: "stub".into(),
                    capabilities: ModelCapabilities {
                        tool_calls: false,
                        vision: false,
                        prompt_cache: false,
                        max_context_tokens: 4096,
                    },
                },
                error,
                calls: 0,
            }
        }
    }

    impl ModelBackend for Stub {
        fn descriptor(&self) -> &ModelDescriptor {
            &self.descriptor
        }

        fn complete(&mut self, _request: &Request) -> io::Result<Response> {
            self.calls += 1;
            if let Some(kind) = self.error {
                return Err(io::Error::from(kind));
            }
            Ok(Response {
                provider: self.descriptor.provider.clone(),
                model: self.descriptor.model.clone(),
                text: "ok".into(),
                usage: Usage::default(),
            })
        }
    }

    fn request() -> Request {
        Request {
            messages: vec![Message {
                role: Role::User,
                content: "test".into(),
            }],
            max_output_tokens: 128,
            require_tools: false,
        }
    }

    #[test]
    fn falls_back_only_to_explicitly_approved_provider() {
        let mut primary = Stub::new("local", Some(io::ErrorKind::TimedOut));
        let mut secondary = Stub::new("cloud", None);
        let policy = RoutingPolicy {
            allowed_providers: vec!["local".into()],
            max_attempts: 2,
        };
        let result =
            complete_with_fallback(&mut [&mut primary, &mut secondary], &policy, &request());
        assert!(result.is_err());
        assert_eq!(primary.calls, 1);
        assert_eq!(secondary.calls, 0);
        let policy = RoutingPolicy {
            allowed_providers: vec!["local".into(), "cloud".into()],
            max_attempts: 2,
        };
        let response =
            complete_with_fallback(&mut [&mut primary, &mut secondary], &policy, &request())
                .unwrap();
        assert_eq!(response.provider, "cloud");
        assert_eq!(secondary.calls, 1);
    }

    #[test]
    fn non_transient_errors_do_not_fallback() {
        let mut primary = Stub::new("local", Some(io::ErrorKind::PermissionDenied));
        let mut secondary = Stub::new("cloud", None);
        let policy = RoutingPolicy {
            allowed_providers: vec!["local".into(), "cloud".into()],
            max_attempts: 2,
        };
        assert!(
            complete_with_fallback(&mut [&mut primary, &mut secondary], &policy, &request(),)
                .is_err()
        );
        assert_eq!(secondary.calls, 0);
    }

    #[test]
    fn empty_allowlist_never_invokes_backend() {
        let mut backend = Stub::new("cloud", None);
        assert!(
            complete_with_fallback(&mut [&mut backend], &RoutingPolicy::default(), &request(),)
                .is_err()
        );
        assert_eq!(backend.calls, 0);
    }
}
