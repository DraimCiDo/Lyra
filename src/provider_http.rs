//! OpenAI-compatible Chat Completions backend.
//! Secrets are read from environment variables at request time, never stored.
use crate::model_gateway::{
    Message, ModelBackend, ModelDescriptor, Request, Response, Usage, validate_request,
};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::io;
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub descriptor: ModelDescriptor,
    /// Full HTTPS URL to /v1/chat/completions. Loopback HTTP is allowed for local servers.
    pub endpoint: String,
    /// Name of an environment variable holding a bearer token, not the token itself.
    pub api_key_env: Option<String>,
}

/// Parse a bounded provider registry and reject ambiguous backend identities.
pub fn parse_registry(bytes: &[u8]) -> io::Result<Vec<ProviderConfig>> {
    use std::collections::HashSet;

    if bytes.len() > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Provider configuration exceeds 64 KiB",
        ));
    }
    let configs: Vec<ProviderConfig> = if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(b'[')
    {
        serde_json::from_slice(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid provider configuration")
        })?
    } else {
        vec![serde_json::from_slice(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid provider configuration")
        })?]
    };
    if configs.is_empty() || configs.len() > 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected between 1 and 32 providers",
        ));
    }
    let mut seen = HashSet::new();
    for config in &configs {
        let identity = (
            config.descriptor.provider.as_str(),
            config.descriptor.model.as_str(),
        );
        if !seen.insert(identity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Duplicate provider and model in registry",
            ));
        }
    }
    Ok(configs)
}

pub struct OpenAiCompatibleBackend {
    config: ProviderConfig,
    client: Client,
    deadline: Option<Instant>,
}

impl OpenAiCompatibleBackend {
    pub fn new(config: ProviderConfig) -> io::Result<Self> {
        let url = reqwest::Url::parse(&config.endpoint).map_err(io::Error::other)?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
        });
        if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Only HTTPS or loopback HTTP endpoints are allowed",
            ));
        }
        if url.username() != "" || url.password().is_some() || url.fragment().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Credentials and fragments in endpoint URLs are forbidden",
            ));
        }
        if config.descriptor.provider.trim().is_empty() || config.descriptor.model.trim().is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Provider and model are required",
            ));
        }
        if config.api_key_env.as_ref().is_some_and(|name| {
            name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid credential environment variable name",
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(io::Error::other)?;
        Ok(Self {
            config,
            client,
            deadline: None,
        })
    }

    /// Bound every request by the same task deadline, including response-body reads.
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    max_tokens: u32,
    stream: bool,
}

#[derive(Deserialize)]
struct WireResponse {
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireMessage,
}

#[derive(Deserialize)]
struct WireMessage {
    content: Option<String>,
}

#[derive(Default, Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: Option<WireTokenDetails>,
}

#[derive(Default, Deserialize)]
struct WireTokenDetails {
    #[serde(default)]
    cached_tokens: u64,
}

impl ModelBackend for OpenAiCompatibleBackend {
    fn descriptor(&self) -> &ModelDescriptor {
        &self.config.descriptor
    }

    fn complete(&mut self, request: &Request) -> io::Result<Response> {
        validate_request(&self.config.descriptor, request)?;
        if request.require_tools {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Tool invocation payloads are not yet supported by this backend",
            ));
        }
        let mut http = self
            .client
            .post(&self.config.endpoint)
            .header(CONTENT_TYPE, "application/json");
        if let Some(deadline) = self.deadline {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "Cloud task deadline exceeded")
                })?;
            http = http.timeout(remaining.min(Duration::from_secs(90)));
        }
        if let Some(name) = &self.config.api_key_env {
            let secret = std::env::var(name).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "API credential is not configured",
                )
            })?;
            if secret.is_empty() || secret.contains('\r') || secret.contains('\n') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid API credential",
                ));
            }
            http = http.header(AUTHORIZATION, format!("Bearer {secret}"));
        }
        let body = WireRequest {
            model: &self.config.descriptor.model,
            messages: &request.messages,
            max_tokens: request.max_output_tokens,
            stream: false,
        };
        let wire = serde_json::to_vec(&body).map_err(io::Error::other)?;
        eprintln!("Lyra HTTP: request_body_bytes={}", wire.len());
        let response = http.body(wire).send().map_err(map_transport_error)?;
        let status = response.status();
        if !status.is_success() {
            let kind = if status.as_u16() == 429 || status.is_server_error() {
                io::ErrorKind::TimedOut
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                io::ErrorKind::PermissionDenied
            } else {
                io::ErrorKind::InvalidInput
            };
            return Err(io::Error::new(
                kind,
                format!("Model provider returned HTTP {}", status.as_u16()),
            ));
        }
        // Cap response size before JSON parsing to avoid unbounded allocation.
        use std::io::Read;
        let mut bytes = Vec::new();
        response
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                if self
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    io::Error::new(io::ErrorKind::TimedOut, "Cloud task deadline exceeded")
                } else {
                    error
                }
            })?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Model response exceeds 8 MiB",
            ));
        }
        let parsed: WireResponse = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        let text = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing assistant text"))?;
        let usage = parsed.usage.unwrap_or_default();
        Ok(Response {
            provider: self.config.descriptor.provider.clone(),
            model: self.config.descriptor.model.clone(),
            text,
            usage: Usage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
                cached_input_tokens: usage
                    .prompt_tokens_details
                    .map_or(0, |details| details.cached_tokens),
            },
        })
    }
}

fn map_transport_error(error: reqwest::Error) -> io::Error {
    let kind = if error.is_timeout() {
        io::ErrorKind::TimedOut
    } else if error.is_connect() {
        io::ErrorKind::ConnectionRefused
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, "Model provider transport error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_gateway::ModelCapabilities;

    #[test]
    fn expired_task_deadline_does_not_connect() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let mut backend = OpenAiCompatibleBackend::new(config(&endpoint))
            .unwrap()
            .with_deadline(Instant::now());
        let error = backend
            .complete(&Request {
                messages: vec![Message {
                    role: crate::model_gateway::Role::User,
                    content: "private".into(),
                }],
                max_output_tokens: 128,
                require_tools: false,
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    fn config(endpoint: &str) -> ProviderConfig {
        ProviderConfig {
            descriptor: ModelDescriptor {
                provider: "local".into(),
                model: "test".into(),
                capabilities: ModelCapabilities {
                    tool_calls: false,
                    vision: false,
                    prompt_cache: false,
                    max_context_tokens: 4096,
                },
            },
            endpoint: endpoint.into(),
            api_key_env: None,
        }
    }

    #[test]
    fn bundled_qwen3_coder_480b_profile_is_valid_and_secretless() {
        let config = include_bytes!("../examples/qwen3-coder-openrouter.json");
        let providers = parse_registry(config).unwrap();
        assert_eq!(providers.len(), 1);
        let provider = &providers[0];
        assert_eq!(provider.descriptor.provider, "openrouter");
        assert_eq!(provider.descriptor.model, "qwen/qwen3-coder");
        assert_eq!(provider.api_key_env.as_deref(), Some("OPENROUTER_API_KEY"));
        assert!(provider.endpoint.starts_with("https://"));
        assert!(OpenAiCompatibleBackend::new(provider.clone()).is_ok());
    }

    #[test]
    fn rejects_insecure_remote_endpoints_and_embedded_credentials() {
        assert!(
            OpenAiCompatibleBackend::new(config("http://example.com/v1/chat/completions")).is_err()
        );
        assert!(
            OpenAiCompatibleBackend::new(config("https://u:p@example.com/v1/chat/completions"))
                .is_err()
        );
        assert!(
            OpenAiCompatibleBackend::new(config("http://127.0.0.1:1234/v1/chat/completions"))
                .is_ok()
        );
    }

    #[test]
    fn localhost_chat_parses_text_and_usage() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0u8; 4096];
            let size = stream.read(&mut request).unwrap();
            let head = String::from_utf8_lossy(&request[..size]);
            assert!(head.starts_with("POST /v1/chat/completions HTTP/1.1"));
            let body = r#"{"choices":[{"message":{"content":"Hello from local model"}}],"usage":{"prompt_tokens":12,"completion_tokens":4,"prompt_tokens_details":{"cached_tokens":2}}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let mut backend =
            OpenAiCompatibleBackend::new(config(&format!("http://{address}/v1/chat/completions")))
                .unwrap();
        let response = backend
            .complete(&Request {
                messages: vec![Message {
                    role: crate::model_gateway::Role::User,
                    content: "Hi".into(),
                }],
                max_output_tokens: 128,
                require_tools: false,
            })
            .unwrap();
        assert_eq!(response.text, "Hello from local model");
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.usage.output_tokens, 4);
        assert_eq!(response.usage.cached_input_tokens, 2);
        server.join().unwrap();
    }

    #[test]
    fn http_503_falls_back_to_explicitly_approved_provider() {
        use crate::model_gateway::Role;
        use crate::routing::{RoutingPolicy, complete_with_fallback};
        use std::io::{Read, Write};
        use std::net::TcpListener;

        fn serve_once(status: &str, body: &'static str) -> (String, std::thread::JoinHandle<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!(
                "http://{}/v1/chat/completions",
                listener.local_addr().unwrap()
            );
            let status = status.to_owned();
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0u8; 4096];
                let size = stream.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..size]).starts_with("POST "));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            (endpoint, thread)
        }

        let (primary_url, primary_server) = serve_once("503 Service Unavailable", "{}");
        let (secondary_url, secondary_server) = serve_once(
            "200 OK",
            r#"{"choices":[{"message":{"content":"fallback succeeded"}}]}"#,
        );
        let mut primary = OpenAiCompatibleBackend::new(config(&primary_url)).unwrap();
        let mut secondary_config = config(&secondary_url);
        secondary_config.descriptor.provider = "backup".into();
        let mut secondary = OpenAiCompatibleBackend::new(secondary_config).unwrap();
        let response = complete_with_fallback(
            &mut [&mut primary, &mut secondary],
            &RoutingPolicy {
                allowed_providers: vec!["local".into(), "backup".into()],
                max_attempts: 2,
            },
            &Request {
                messages: vec![Message {
                    role: Role::User,
                    content: "Hello".into(),
                }],
                max_output_tokens: 128,
                require_tools: false,
            },
        )
        .unwrap();
        assert_eq!(response.provider, "backup");
        assert_eq!(response.text, "fallback succeeded");
        primary_server.join().unwrap();
        secondary_server.join().unwrap();
    }

    #[test]
    fn registry_accepts_single_and_array_configs() {
        let one = serde_json::to_vec(&serde_json::json!({
            "descriptor": {
                "provider": "local",
                "model": "test",
                "capabilities": {
                    "tool_calls": false,
                    "vision": false,
                    "prompt_cache": false,
                    "max_context_tokens": 4096
                }
            },
            "endpoint": "http://127.0.0.1:1234/v1/chat/completions",
            "api_key_env": null
        }))
        .unwrap();
        assert_eq!(parse_registry(&one).unwrap().len(), 1);
        let mut backup: serde_json::Value = serde_json::from_slice(&one).unwrap();
        backup["descriptor"]["provider"] = "backup".into();
        let array = serde_json::to_vec(&serde_json::json!([
            serde_json::from_slice::<serde_json::Value>(&one).unwrap(),
            backup
        ]))
        .unwrap();
        assert_eq!(parse_registry(&array).unwrap().len(), 2);
        let duplicate = serde_json::to_vec(&serde_json::json!([
            serde_json::from_slice::<serde_json::Value>(&one).unwrap(),
            serde_json::from_slice::<serde_json::Value>(&one).unwrap()
        ]))
        .unwrap();
        assert_eq!(
            parse_registry(&duplicate).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn registry_rejects_invalid_and_oversized_inputs() {
        for bytes in [&b"[]"[..], &b"null"[..], &b"{not json}"[..], &b"[{}]"[..]] {
            assert!(parse_registry(bytes).is_err());
        }
        assert!(parse_registry(&vec![b' '; 64 * 1024 + 1]).is_err());
    }

    #[test]
    fn rejects_invalid_credential_env_name() {
        let mut cfg = config("https://example.com/v1/chat/completions");
        cfg.api_key_env = Some("BAD-NAME".into());
        assert!(OpenAiCompatibleBackend::new(cfg).is_err());
    }
}
