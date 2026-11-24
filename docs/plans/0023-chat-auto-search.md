# Automatic Chat web routing

Owner request: enable web capability by default and use it when needed.
Change v2 Chat's toggle into Auto (default), Always, Off. The local model classifies
the current question using constrained JSON before any search. Facts about named
subjects/current information use evidence search; conversation, arithmetic and
provided-text work stay local. Cached-source follow-ups never search again.
Auto never uploads history, files, multiline/fenced code or overlong messages.
One model session handles routing and the answer/evidence selection. Existing
query/citation restrictions and honest insufficient-evidence replies remain.
No new dependencies, saved-state formats, Work/hub changes or additional agents.

Validation: fmt; desktop all-targets Clippy; library tests; explicit offline GGUF
routing acceptance for greeting, arithmetic, named server and current weather.
Native CI/build and UI acceptance follow the code checks. Existing PR blockers
remain outside this task.

Implemented. Linux Docker / Rust 1.99.1: fmt and desktop all-targets Clippy clean;
194 library tests passed, 5 opt-in tests skipped. Explicit real Qwen2.5-Coder 1.5B
routing acceptance passed all four cases: greeting and arithmetic local; named
server and current weather search. No network is used for this routing test.
Native CI/build and UI acceptance of the Auto default remain pending.
