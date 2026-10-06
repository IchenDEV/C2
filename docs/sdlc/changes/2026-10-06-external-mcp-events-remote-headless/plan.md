---
id: 2026-10-06-external-mcp-events-remote-headless
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-06
based_on: spec.md
scope: docs/sdlc/changes/2026-10-06-external-mcp-events-remote-headless/, docs/design/external-mcp.md, docs/catalog.json, Cargo.toml, Cargo.lock, crates/core/Cargo.toml, crates/core/src/engine.rs, crates/core/src/store.rs, crates/core/src/host_mcp/, crates/core/src/external_mcp/, crates/core/src/plugins/app/plugins/external_mcp.rs, crates/core/src/plugins/app/plugins/mod.rs, crates/core/tests/external_mcp.rs, crates/core/tests/external_mcp_write.rs, crates/core/tests/external_mcp_admin.rs, crates/core/tests/external_mcp_events.rs, crates/server/Cargo.toml, crates/server/src/lib.rs, crates/server/src/main.rs, crates/server/src/serve.rs, crates/server/src/auth.rs, crates/server/src/external_mcp.rs, crates/server/src/external_mcp_stream.rs, crates/server/src/remote.rs, crates/server/src/bin/codetwo.rs, crates/server/src/bin/codetwo-agent.rs, crates/server/src/cli/, crates/server/tests/external_mcp_http.rs, crates/server/tests/external_mcp_stream.rs, crates/server/tests/remote_supervisor.rs, crates/server/tests/codetwo_cli.rs, crates/server/tests/cli_token_hardening.rs, crates/server/tests/fixtures/, apps/desktop/src/settings/, apps/desktop/src/bridge.ts, apps/desktop/src/i18n/strings.ts, apps/desktop/tests/externalMcpSettings.test.tsx
---

# Plan: External MCP, Events, Remote Access, Headless

## Plan

Design is confirmed (see Spec "Confirmed decisions"). Foundations (credentials, catalog, audit, event ring, generic handler, `ctx`/`state`/`ops_*` skeletons, Engine accessor, route stubs) exist from change `2026-10-06-external-mcp-foundations`.

Wave A, parallel lanes with disjoint files (shared files are touched only by the named owner):

- G (gate + read + route): `external_mcp/{gate,dispatch,ops_read,clients(TTL change)}.rs`, `Engine::authorize_external_mcp_call`, event ingestion hook in `engine.rs`, `crates/server/src/external_mcp.rs`, `main.rs`/`codetwo-agent.rs` enable and data-dir wiring, tests `crates/core/tests/external_mcp.rs`, `crates/server/tests/external_mcp_http.rs`.
- W (write + admin tools): `external_mcp/{ops_write,ops_admin}.rs`, `crates/core/tests/external_mcp_write.rs`, `external_mcp_admin.rs`. Calls only public `Engine` APIs; no `engine.rs` edit.
- E (events): `external_mcp/{ops_events,mcp_events,subscriptions}.rs`, `external_mcp/events.rs`, `crates/server/src/external_mcp_stream.rs`, tests `crates/core/tests/external_mcp_events.rs`, `crates/server/tests/external_mcp_stream.rs`.
- R (remote): `crates/server/src/remote.rs`, `auth.rs`, fixtures and `remote_supervisor.rs`; route allowlist layer as one additive edit in `crates/server/src/lib.rs`.
- U (UI credential issuance): `plugins/app/plugins/external_mcp.rs` + one registration line in `plugins/mod.rs`, desktop settings panel, `bridge.ts`, `strings.ts`, `externalMcpSettings.test.tsx`.

Wave B after G is merged: C (`codetwo` CLI: `bin/codetwo.rs`, `cli/`, `codetwo_cli.rs`, Cargo bin entry) and D (docs: `docs/design/external-mcp.md`, `docs/catalog.json`). Wave C: independent review subagents (security/authorization, protocol/events, UI/CLI/remote) and fixes.

Checks: `cargo check --workspace --tests`; `cargo test -p codetwo-core --lib external_mcp host_mcp` and the new core integration tests; `cargo test -p codetwo-server` new tests on temporary ports; existing `host_mcp`, `host_mcp_http`, `native_provider_engine` unchanged and green; negative tests for scope, project confinement, non-loopback, oversize, rate limit, token leakage, D-tool confirmation; desktop `tsc` and `bun test` for the new test file; `bun script/verify/docs.ts`; `bun script/verify/sdlc.ts --worktree`. UI: a jsdom rendered test (light/dark/narrow where applicable); no native window is launched because profile-isolated desktop launches are not supported and the user's instance must not be touched, recorded as residual risk. No real tunnel, no real T3, no user data directory.

Continuation on 2026-10-06: the owner resumes integration and verification after the provider failure. Three supervised Cursor Sonnet 5.5 / 300k / high tasks handle CLI completion, read-only security review, and read-only protocol/events review. The CLI worker exclusively owns `bin/codetwo.rs`, `cli/`, `serve.rs`, and `codetwo_cli.rs`; the owner integrates findings in other files. Existing design authorization is unchanged.

Temporary resources: reuse the existing `/tmp/c2-target-check` Cargo output for lock-serialized checks; the owner disposes it at final handoff. Research notes `/tmp/c2-research/` are inherited inputs. Tests remove fixtures under `std::env::temp_dir()`. Do not dispose other threads' `/tmp/c2-target-t3v2*` resources.

Rollback: revert this diff; all surfaces are default off and no data migration is introduced (credential and audit files are new, removable files under the data directory).
