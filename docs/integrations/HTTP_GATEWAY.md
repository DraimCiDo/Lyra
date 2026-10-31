# HTTP Model Gateway (opt-in)

Build the CLI with `cargo run --features gateway-http -- chat --help`.

A local OpenAI-compatible server can be queried with:

```bash
cargo run --features gateway-http -- chat \
  --provider-config examples/openai-compatible-provider.json \
  --allow local \
  --prompt "Explain this Rust error"
```

Edit the example model name and endpoint to match the server. Cloud endpoints must use HTTPS. To use a bearer token, set `api_key_env` in the JSON configuration to the **name** of an environment variable (for example `LYRA_PROVIDER_KEY`) and set that variable outside Lyra. Never store tokens in JSON or commit them.

The provider allowlist is mandatory. A mismatched provider fails before any HTTP request. Redirects are disabled. Requests time out after 90 seconds. This initial implementation uses non-streaming `/v1/chat/completions`, plain-text assistant responses, and token usage where returned. Tool calling, streaming, image inputs, Desktop chat routing, and provider-specific API dialects remain pending; this is not yet full OmniRoute parity.

## Explicit multi-provider fallback

`--provider-config` also accepts a JSON array of provider objects with the same schema. Entries are tried in array order, but **only** if their provider names appear in one or more `--allow` flags. For example:

```bash
cargo run --features gateway-http -- chat \
  --provider-config providers.json \
  --allow local --allow cloud \
  --max-attempts 2 \
  --prompt "Explain ownership in Rust"
```

The default `--max-attempts 1` disables fallback. Only transient transport failures, HTTP 429, and HTTP 5xx can trigger fallback. Authentication and invalid-request errors stop immediately. Allowlisting cloud providers explicitly permits sending the prompt to them; do not add cloud to the allowlist for confidential projects without authorization.

## Provider registry validation

Provider configuration accepts one JSON object or an array of 1–32 objects, up to
64 KiB total. Each `provider` + `model` pair must be unique. Duplicate identities
are rejected rather than silently skipped, preventing ambiguous fallback order.
Malformed registry files fail before any HTTP request. Credentials remain
environment-variable references, never JSON token values.

The CI matrix runs both the default Desktop tests and explicit
`desktop,gateway-http` Clippy/tests on Ubuntu and Windows.

## Desktop provider registry (feature-gated)

Build with `--features desktop,gateway-http` to show **HTTP-провайдеры**
in Desktop settings. Enter the path to the JSON registry and click
**Проверить реестр**. This validates the file format, provider identity
uniqueness, endpoint scheme and credential environment-variable names
without sending prompts or making network requests.

Only the registry **path** is persisted in Desktop preferences. The
registry is not used by the Desktop conversation worker yet: cloud chat
remains available through the `lyra chat` CLI command with an explicit
provider allowlist. The UI does not store or request API keys.
