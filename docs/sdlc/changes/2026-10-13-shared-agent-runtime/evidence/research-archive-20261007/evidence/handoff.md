# Handoff: shared-agent research and runtime records (2026-10-13)

Status: reviewable documentation only. Nothing here is committed, pushed or published, and nothing here claims a runtime result. This page lets a remote reviewer read the latest decisions and blockers without any private local files.

## Latest decisions

| Topic | Current state | Source type |
| --- | --- | --- |
| Private task visibility | Creator only, including the machine owner (the owner is filtered by the same rule) | Confirmed user decision, relayed by the coordinating thread, not independently authenticated |
| Claiming | Nonexclusive following/signature, never exclusive control. Earlier "no claim" wording is superseded | Confirmed user decision, same provenance |
| New task public by default; public never returns to private; legacy private publishes one-way | Simulation proposals only. NOT approved runtime rules | Prototype rules |
| Who may publish a legacy private task; whether a new device of the same person is the same participant; real source of the execution-device label | Open | Open |
| Visibility attaches to Session or to Team `Task` | Technical candidate (Session), not proven; needs frontend and command-table tracing | Open |
| Any-actor fallback for legacy receipts | NOT an approved privacy design; legacy rows must fail closed or be migrated under an explicit owner decision | Correction |

## Verdicts

- Research prototype preview: PARTIAL / blocked. The 11505-byte HTML (SHA-256 `12f3fc86c9c84f24bfbb626461108f4d8ac8f101f7cef628b5e688611996a8dc`) is unchanged, but no PNG and no input manifest were saved and later `html_preview` retries were rejected by the host. The earlier PASS is historical and covers only an in-memory mock.
- Runtime: NOT_IMPLEMENTED. No private filtering, nicknames, claiming, sharing entry or per-actor attribution exists. No server or two-browser test ran.

## Static route facts (read, not run; HEAD `fb3cb82964acd3643ccd81f35f73ec0ab0489863`)

- Ordinary React UI: `apps/desktop/src/coreTransport.ts` pairs at `/api/pair`, gets a ticket at `/api/ws-ticket`, opens `/ws?ticket=` for events, and posts commands to `/api/web-ui/call`.
- Server: `web_ui_call` -> `require_device` (bearer -> `device_id`) -> `WebUiCommandCaller::call` -> `KernelWebUiCommands::call`, which ignores its device id argument (`crates/server/src/lib.rs`). Commands therefore carry no actor today.
- T3/mobile compatibility (`crates/server/src/t3_compat.rs`: `/api/orchestration/dispatch`, `/ws?wsTicket=`, `/oauth/token`) is a different surface the frontend does not call. An earlier candidate changed that surface by mistake and was withdrawn; `t3_compat.rs` equals HEAD again.

## Blockers

- Compile and tests are blocked: the exact Ghostty source (commit `a887df42c56f6de86c0fe6da9c4eeca37931e083`, needs `build.zig`) is absent locally, and the single archive download failed with `Could not resolve host: github.com` (curl exit 6). This is a resolver/connectivity failure, not a quoted permission rejection.
- No actual server was started and no two-browser test ran.
- High-risk design (persisted per-actor receipt keys, migration, private filtering) has no independent approval. None is invented.

## Remaining validation

Real isolated Core plus two isolated browser profiles covering public participation, attribution, stop/continue sync, and server-side private filtering at list, direct read, HTTP command and live event; queue/steer and shared-worktree behavior under two authors remain unverified.

## Scrub notes

Removed from the published files: absolute local paths, references to private ignored `.local` task files, internal task/thread/dispatcher identifiers, request identifiers, and personal executor names. Kept: hashes, code paths with lines, host error text, decisions and limits. Original and preview history is preserved in the private snapshot, not in these files.
