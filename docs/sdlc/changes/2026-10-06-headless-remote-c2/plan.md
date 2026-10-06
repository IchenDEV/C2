---
id: 2026-10-06-headless-remote-c2
schema: 5
stage: plan
status: accepted
owner: cursor-agent
created: 2026-10-06
based_on: spec.md
scope: crates/server/Cargo.toml, crates/server/src/daemon.rs, crates/server/src/lib.rs, crates/server/src/main.rs, crates/server/tests/web_ui_commands.rs, apps/desktop/src/remoteEnvironments.ts, apps/desktop/src/remoteTerminals.ts, apps/desktop/src/coreTransport.ts, apps/desktop/src/bridge.ts, apps/desktop/src/environment/EnvironmentPopover.tsx, apps/desktop/src/environment/useEnvironments.ts, apps/desktop/src/settings/RemoteEnvironmentsSettings.tsx, apps/desktop/src/settings/SettingsPage.tsx, apps/desktop/src/sidebar/SessionRail.tsx, apps/desktop/src/i18n/strings.ts, apps/desktop/tests/remoteEnvironments.test.ts, apps/desktop/tests/remoteEnvironmentsRendered.test.tsx, apps/desktop/tests/remoteTerminals.test.ts, apps/desktop/tests/sessionRailRendered.test.tsx, website/guide/remote.md, website/reference/server.md, .agents/skills/codetwo-operations/references/remote-agent.md, docs/sdlc/changes/2026-10-06-headless-remote-c2
---

# Plan: Headless Remote C2

## Plan

Smallest path that reuses what exists: the server already has pairing, `/api/web-ui/call`, the
event WebSocket and the paired web transport; this adds a daemon entry point (`daemon.rs`,
`serve`/`pair` in `main.rs`), a CORS layer in `lib.rs`, and in the desktop a pure registry +
federated Core (`remoteEnvironments.ts`) installed in `coreTransport.ts`, with `bridge.ts` routing
new sessions. Remote folders reuse the existing panels unchanged: the owning environment travels inside the
session's folder path (`c2env://…`), `remoteEnvironments.ts` routes by it, and `remoteTerminals.ts`
adapts `terminal.*`/`pty-*` to the server's `/ws/terminal`. The server only widens its allowlist.
UI is limited to one settings page, one popover section and a rail badge.

Checks by risk: Rust unit tests for the daemon and CLI parsing, an integration test for CORS, and a
real-process smoke (`serve`, second owner, `pair`, pair + call + CORS headers, `SIGTERM`); Bun unit
tests for the registry/federation; DOM-rendered tests plus real Chromium screenshots in light and
dark for the three UI surfaces; unit tests for folder routing and the terminal adapter against a
fake socket; one real end-to-end run of the federated Core against a live
`serve` (pair, list, events, remote new session, then file write/read/list, Git status, a denied command
and a real shell attach/echo/kill with `SHELL=/bin/sh`). The Electrobun desktop window was not launched:
that needs the instance preflight and would risk the user's live Core, and the surfaces are
renderer-only.

Temporary resources: temp data directories and the smoke `serve` process (stopped by `SIGTERM`
and removed), a scratch Vite page and Playwright screenshots under `/tmp` (deleted), and
`apps/desktop/node_modules` from `bun install` (git-ignored, retained for later checks).

Rollback: revert the commit. Remote environments live only in renderer `localStorage`; the daemon
adds `server.pid` and a transient `pairing.url` in its own data directory, both safe to delete when
no daemon runs.
