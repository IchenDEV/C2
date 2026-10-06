---
id: 2026-10-06-headless-remote-c2
schema: 5
stage: spec
status: accepted
owner: cursor-agent
created: 2026-10-06
based_on: intent.md
design_approved_by: user
design_approved_at: 2026-10-06
design_approval_source: "Current chat: user replied 批准 to the design summary (CORS boundary, per-environment bearer in renderer storage, pair via SIGUSR1 file, known limits); later replied 好同时批准三件事 to widening the server command allowlist and routing remote files, Git and terminals."
---

# Spec: Headless Remote C2

## Design

**Server (`codetwo-server serve`, `pair`).** `serve` boots one Core, binds `127.0.0.1` unless
`--host`/`CODETWO_HOST` says otherwise, always exposes the existing authenticated
`/api/web-ui/call` allowlist, and serves the React UI only if its assets exist (an explicit
missing `--ui-dir` is an error). It claims `<data-dir>/server.pid` before booting Core and refuses
to start while that pid is alive; a stale pid is replaced and the claim is released on exit.
`SIGTERM`/Ctrl-C stops Core cleanly. New pairing links are requested with `codetwo-server pair`,
which signals the daemon (`SIGUSR1`); the daemon writes the link to a `0600` file the CLI reads and
deletes. No network route mints credentials. `--public-url` / `CODETWO_PUBLIC_URL` overrides the
advertised address for proxies.

**Cross-origin.** A desktop renderer is a different origin from the server, so the router adds a
CORS layer: any origin, methods GET/POST/PUT, headers `Authorization` and `Content-Type`, no
credentials mode. Authority is the bearer header, a single-use ticket, or a one-time pairing token,
never a cookie, so allowing origins adds no ambient authority.

**Desktop federation.** A registry of remote environments (`id`, `name`, `baseUrl`, optional
server workspace) is stored in renderer `localStorage`, with each environment's bearer under its own
key. Pairing redeems the one-time link at `POST /api/pair`; re-pairing the same server replaces its
credential. Each environment gets the existing paired web Core transport. A federated Core wraps the
local Core: `sessions.list`, `sessions.archived` and `sessions.previews` are merged, remote rows
carry `environment_id`, and an unreachable environment is skipped and marked offline; engine events
from every environment reach one listener; a session id seen in a remote list or event is owned by
that environment, and any command naming it is sent there (without a local `project_path`);
`engine.new_session` with `environment_id` is created remotely and the field is stripped. The
Environment popover chooses where new sessions run; Settings lists, pairs, edits the server
workspace of, and removes environments; the session rail marks remote rows and hides local-only
"reveal in Finder".

**Remote folders.** A remote session's `cwd`, `project_path` and `worktree_path` (list rows and
`session_created` events) are rewritten to `c2env://<environment>/<path>`. Any command whose `cwd`
carries that prefix is sent to that environment with the prefix removed; a local path equal to a
remote one is never routed remotely, and a removed environment rejects instead of falling back to
local files. `engine.new_session` for a remote folder is created remotely. The server allowlist
gains the workspace file commands (list, read, write, create, rename, copy, delete, search, rules,
source control) and Git commands (status, diff, stat, diff-since, stage, unstage, commit, push,
revert, checkpoints, message suggestion). `workspace.run_script`, `workspace.save_script`,
`git.create_pr`, `github.*`, `lsp.*` and `terminal.*` stay denied there.

**Remote terminals.** The desktop already calls `terminal.spawn/write/resize/kill` and listens to
`pty-*`. For a remote folder the federated Core hands those to `remoteTerminals.ts`, which opens one
`/ws/terminal` socket per terminal using a single-use ticket from the environment's bearer,
attaches with a server-safe id derived from the renderer id (`c2d-…`), replays the shell's screen
on reattach, forwards output/title/exit under the renderer's id and realm, and on a dropped socket
reattaches and repaints. Shells live on the server and survive this app.

**Known limits.** Remote sessions cannot run project scripts, open pull requests, start language
servers, use the browser dock, create worktrees or parallel tasks, or reveal folders in Finder; these
error or stay empty. A paired device can now read, write and delete files and run a shell on the
server wherever the server user can; pairing is therefore equivalent to a shell login, and the
bearer in `localStorage` is readable by renderer code like the existing web bearer. `terminal.dump`
is unsupported remotely. Plain HTTP is acceptable only on loopback/tailnet or behind a TLS tunnel.

Design approval for this high-risk change (CORS and credential boundary) is recorded in the
frontmatter by the user, who is independent of the implementation owner.

## Acceptance criteria

- [x] AC-1: `codetwo-server serve` runs headless: second owner of a data directory is refused, a stale pid is replaced, pairing + `/api/web-ui/call` work, and `SIGTERM` exits cleanly releasing the claim.
- [x] AC-2: `codetwo-server pair` returns a fresh working one-time link from the running daemon, fails clearly when no daemon owns the directory, and the link file is `0600`.
- [x] AC-3: A cross-origin client can preflight and call with `Authorization`, receives no credentials-mode CORS, and is still rejected (401) without a bearer.
- [x] AC-4: The desktop pairs a server from its link (bad/used links add nothing and explain recovery), stores the bearer per environment, and can remove it.
- [x] AC-5: Session lists/previews/events merge across local and remote, remote rows are tagged, commands for a remote session reach only its owner, offline environments do not hide reachable sessions, and with no environments nothing changes.
- [x] AC-6: New sessions can be created on a chosen environment through the Environment popover and show up tagged and streaming, without the routing field reaching a Core.
- [x] AC-7: Settings, popover and rail render correctly in light and dark themes.
- [x] AC-8: An independent human design decision for the CORS/credential boundary is recorded.
- [x] AC-9: Files, search, Git and terminals of a remote session run on its server: remote-folder commands reach only that environment, local paths and removed environments never fall through, denied commands stay denied, and a terminal attaches, streams, resizes, reattaches after a drop and is killed on the server.
