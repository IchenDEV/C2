# independent-r8 result: changes-required (P2 flaky tests); no source edits
- live 358 hashes match source-sha256.json before/after (0 mismatch); build-source copies: Rust all match except isolated Cargo.toml (+libghostty pkg-config, intended); 179 UI-test/docs files not copied.
- core all (outside harness sandbox): run1 601... see core-all.log (fail-fast stopped at assistant_observation 9/1), core-all-nofailfast.log (1 lib fail under CPU load: slow_observation_a... assistant.rs:6712 left invalidated right running; 8/8 pass unloaded).
- poison_duplicate_scope... tests/assistant_observation.rs:819: failed 4/8 isolated reruns (flake-poison-dup.log).
- UI 64 pass/0 fail; tsc --noEmit exit 0.
