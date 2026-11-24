# Conversation output and factual activity

Owner reference: continuous assistant output, formatted paragraphs/lists/code/links,
compact tool activity and a collapsible Worked for duration instead of reply cards.

Implement v2 Chat assistant messages without a framed card; keep user questions
distinct. Support bold, inline/fenced code, headings, lists and validated HTTP(S)
Markdown links, plus answer/code copy actions. Preserve streaming and literal
unfinished/unsupported syntax. No new dependencies or saved state formats.

Keep actual worker progress events (bounded at 64 entries / 400 characters),
duration and failure state in memory. Attach the activity to the corresponding
answer; show ongoing activity during streaming. Work retains independent verified
diff/Apply controls and displays the same factual activity above its result.
No simulated edits, commands, hidden reasoning or model-authored tool history.

Validate parser safety/streaming and activity limits, fmt, desktop all-targets
Clippy and library tests; render representative rich output in a real test window.
Native CI and updated Windows artifacts remain separate evidence.

Validation (2026-11-24): Linux Docker / Rust 1.99.1 fmt and all-targets Clippy
passed for default, desktop and desktop,gateway-http. Full cargo test passed in
all three configurations (library: 126/196/214 passed; 1/5/5 opt-in tests ignored).
The desktop binary built and --smoke-test rendered a labelled rich-answer fixture
under Xvfb, then exited successfully. Screenshot review corrected list alignment
and text-row spacing. This is presentation evidence, not a live model/web run or
native Windows acceptance of the new UI.
