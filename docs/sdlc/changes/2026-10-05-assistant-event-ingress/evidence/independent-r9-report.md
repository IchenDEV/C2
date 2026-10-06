# independent r9 (compact)
Verdict: pass (limited review). Core --no-fail-fast rerun: 38 result lines, 780 passed, 0 failed, 1 ignored, EXIT 0 (core-all-rerun.log). UI 64/0 EXIT0 (ui.log); tsc EXIT0; oxlint EXIT0.
358 live hashes match before/after. Isolated copy: only crates/core/Cargo.toml (+ derived Cargo.lock libghostty-vt-sys/pkg-config lines) differs.
First Core attempt (core-all.log) was killed mid canvas_v1 when its launching tool call ended (no EXIT line); not a test failure; superseded by rerun.
