---
id: 2026-10-06-headless-remote-c2
schema: 5
stage: verification
status: passed
owner: cursor-agent
created: 2026-10-06
based_on: plan.md
revision: "Worktree based on 40a2c7c4 with this change uncommitted"
verification_mode: human
verified_by: user
verified_at: 2026-10-06
release_target: none
cleanup_status: complete
---

# Verification: Headless Remote C2

## Verification

- AC-1: PASS — `cargo test -p codetwo-server` daemon tests (second owner refused, stale pid replaced) and a live `serve` smoke on a temp data directory: second `serve` refused naming the owner pid, pairing + `/api/web-ui/call` returned `{"result":[]}`, `SIGTERM` removed `server.pid` and exited.
- AC-2: PASS — live `codetwo-server pair` printed a working link that was redeemed; unit test covers no-owner failure and `0600` atomic file.
- AC-3: PASS — `another_c2_client_origin_may_preflight_and_call_with_a_bearer_header` (preflight `*`, no allow-credentials, 401 without bearer); live curl showed `access-control-allow-origin: *`.
- AC-4: PASS — `bun test tests/remoteEnvironments.test.ts` and `tests/remoteEnvironmentsRendered.test.tsx` (pairing, rejected link, re-pair replaces credential, removal clears bearer) plus a live pairing against `serve`.
- AC-5: PASS — registry/federation unit tests (merge + tag, previews, offline, ownership routing, event merge, late-added environment) and the live run: merged list showed the remote session tagged while the local Core received only `sessions.list`.
- AC-6: PASS — live run created `engine.new_session` through the federated Core with `environment_id`; `session_created`/`models` events returned over the WebSocket, ownership was learned from the event, the local Core never received the call. The full Environment-popover-to-composer click path was not driven in a real Electrobun window.
- AC-7: PASS — `node shot.mjs` (Playwright Chromium against a scratch Vite page, deleted afterwards) produced real screenshots (light and dark) of the Environment popover and the Remote environments settings page after a layout fix; the rail badge and remote folder naming are covered by `bun test --isolate ./tests/sessionRailRendered.test.tsx` (DOM, not Chromium).
- AC-9: PASS — `bun test --isolate ./tests/remoteEnvironments.test.ts ./tests/remoteTerminals.test.ts` (folder routing and no local fallthrough, rewritten session folders and `session_created`, terminal id mapping, attach/ticket, output/title/exit under the renderer id, input/resize/kill, refusal, reattach and repaint after a drop, viewer replacement); `cargo test -p codetwo-server --lib browser_renderer` (panel commands allowed, scripts/GitHub/LSP/terminal denied); and a live run of the real registry, web transport and terminal adapter against `codetwo-server serve --data-dir <tmp>` with `SHELL=/bin/sh`: remote `create_file`+`write_text`+`read_text`, `list_dir`, `git.status`, a denied `workspace.run_script`, a real shell `pwd; echo marker-$((6*7))` echoed back, kill, and zero calls on the local Core. Window resize on a live remote shell and the reattach-after-drop path were checked only against a fake socket.
- AC-8: PASS — spec.md frontmatter records the user's design approval (`批准`, 2026-10-06) in this chat; the user is not the implementation owner.
- Human verification: the user, who is not the implementation owner, replied `好同时批准三件事` on 2026-10-06 to a list that included independent verification of this change, after the evidence above was reported. The user did not re-run the commands; the record shows an accepted verifier, not an independent re-execution.

Verdict: verified.
Residual risk: Electrobun desktop window not launched, so renderer `localStorage` persistence, the `views://` origin against real CORS, and the `c2env://` folder naming in every panel (editor tabs, Source Control, terminal titles) are unproven in the shipped shell; only the file breadcrumb, rail and settings were reasoned about or DOM-rendered. Pairing is now equivalent to a shell and file-write login on the server user (accepted in the design approval); the bearer sits in renderer storage and `serve` has no device-revocation CLI beyond editing `remote-devices.json`. Project scripts, GitHub, LSP, browser dock, worktrees, parallel tasks and `terminal.dump` stay local-only. Regression: `cargo test -p codetwo-server` passes except `terminal_ws::terminal_attach_io_reattach_and_kill`, which fails on unchanged HEAD too and passes with `SHELL=/bin/sh` (this machine's interactive zsh first-run prompt swallows the test's input), so it is an environment issue, not this change; desktop `bun test --isolate` 1076 pass / 0 fail, `tsc --noEmit` and `ultracite check` clean; `cargo fmt --check` differences exist only in untouched `codetwo-agent.rs` and `terminal.rs`. `bun script/verify/docs.ts` fails on an existing broken link in `docs/reference/plugin-protocol.md` that this change does not touch. Not run: full `cargo test --workspace`.

## Cleanup

Removed: temp data directories and git workspace, smoke `serve` processes, scratch Vite page, scratch end-to-end script and entry files, Playwright screenshots and script under `/tmp`.
Retained: `apps/desktop/node_modules` (git-ignored install).
Retention owner: user's worktree.
Cleanup trigger: delete `apps/desktop/node_modules` with the worktree.
Processes: none left running (checked with `pgrep`).
Evidence: `pgrep -fl "codetwo-server|vite --port 5199"` returned nothing after `kill -TERM`; `git status --short` lists no scratch files; test names above.

## Review and release

Approval: design and the remote files/Git/terminal extension approved by user 2026-10-06; verification accepted by user 2026-10-06; merge and release approval pending; PR delivery approved in the same reply.
Rollback: See plan.md.
Release: No release requested; merge and external actions require their own authorization.
Feedback: Link an Incident and regression Eval when a real failure occurs.
