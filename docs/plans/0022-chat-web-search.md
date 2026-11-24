# Chat web evidence

Scope: add explicit, opt-in DuckDuckGo search to v2 Chat without new dependencies.
Only the current question is sent. No repository, chat history, credentials or source
pages are uploaded/fetched. Search results are untrusted data. The local model selects
relevant evidence IDs through a constrained schema; the controller renders verbatim
excerpts and validated HTTPS source links, never generated factual claims. Empty,
blocked, malformed or irrelevant results fail closed with an insufficient-evidence reply.
Offline chat gets an explicit capability/uncertainty policy. No Work/hub changes.

Checks: cargo fmt, focused web/desktop tests, desktop Clippy/build in a local Linux
Docker development image; native Windows CI/build and UI acceptance when available.
Test source parsing, unsafe citations, unknown evidence IDs, no evidence, cancellation,
and a real search. Preserve PR #7 and its existing known integrity issues.

Implemented opt-in search, constrained evidence selection, source links, deterministic
insufficient-evidence replies and offline capability policy. No additional agents or
new product dependencies.

Local Linux Docker / Rust 1.99.1: fmt clean; desktop all-targets Clippy clean;
library tests 191 passed, 4 ignored. Explicit live acceptance passed: DuckDuckGo
search plus Qwen2.5-Coder 1.5B Q4_K_M selects actual source excerpts with the grammar.
First implementation Windows/Linux and both CUDA CI builds passed. Native RTX 4060
UI displayed source excerpts and clickable links. UI acceptance exposed two defects:
formatting follow-ups searched their own words, and the search engine dropped an
unknown subject while the model selected generic results. Follow-ups now reuse the
last cited sources; visible evidence must contain distinctive subject names/IDs.
Regression coverage and desktop Clippy pass; 193 library tests passed, 4 ignored.
A live negative-query attempt was blocked by DuckDuckGo's challenge page; it did
not reach inference. Earlier positive search + real 1.5B acceptance passed. New
Windows/CUDA artifacts and native acceptance of these two fixes remain pending.
