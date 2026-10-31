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

The provider allowlist is mandatory. A mismatched provider fails before any HTTP request. Redirects are disabled. Requests time out after 90 seconds. This initial implementation uses non-streaming `/v1/chat/completions`, plain-text assistant responses, and token usage where returned. Tool calling, streaming, image inputs, Desktop selection, and provider-specific API dialects remain pending; this is not yet full OmniRoute parity.
