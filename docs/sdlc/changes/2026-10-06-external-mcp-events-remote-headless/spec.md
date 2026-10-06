---
id: 2026-10-06-external-mcp-events-remote-headless
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-06
based_on: intent.md
design_approved_by: user
design_approved_at: 2026-10-06
design_approval_source: "User reply 2026-10-06 18:51 to the confirmation list: 同意引入 MCP 网络面; 凭证允许在界面上签发,允许永久,默认 30 天; 第二批工具同意; Approve 范围默认给予授权,方便用为主,权限以后再说; D 类/删除类工具也支持但 MCP 要标注; 隧道功能也支持; 事件策略同意; 实验性草案(MCP Events)作为正式功能支持."
---

# Spec: External MCP, Events, Remote Access, Headless

Design status: confirmed by the user on 2026-10-06 18:51; the confirmed changes below take precedence over the earlier text of this Spec.

## Confirmed decisions (supersede conflicting text below)

1. External MCP network surface: approved. Still default off, explicitly enabled (`CODETWO_EXTERNAL_MCP=1`, `codetwo serve --external-mcp`, or the desktop setting).
2. Credentials may be issued from the desktop UI (and the CLI). TTL options: 30 days (default), custom, or permanent (`expires_at` absent, must be chosen explicitly). The 90-day maximum and the "no UI issuance" rule are dropped. Permanent credentials stay revocable, hash-only and audited.
3. Wave 2 write tools (W/X): approved.
4. Scope `approve` is granted by default to new clients (convenience first; the UI shows it and can untick it). `--allow-approve` is no longer required. `admin` stays an explicit choice per client because it unlocks D tools.
5. D tools are implemented, available only to credentials holding `admin`: `project_delete` (unregisters, never deletes user files), `session_delete`, `worktree_delete`, `automation_create|update|delete`, `scene_run`, `pipeline_run`. Marking: `destructiveHint=true`, description prefixed `[DESTRUCTIVE]`, `_meta["codetwo/risk"]="destructive"`, and every D call requires `confirm:true` plus the target `id` repeated in `expected_id`; otherwise the gate refuses without effect. Execution-policy loosening is still not offered (no escalation rule stays). The `CODETWO_EXTERNAL_MCP_ADMIN` env flag is dropped; the `admin` scope is the control.
6. Tunnel: implemented as designed (external `cloudflared`, named tunnel; quick tunnel behind its own flag), tested with a fake executable, no real tunnel in development.
7. Event content policy approved. MCP Events (draft, `experimental-ext-triggers-events`) and `subscriptions/listen` are supported as regular features of the external MCP when it is enabled, no extra flag, but `codetwo_capabilities` and docs label them with their upstream status (draft or experimental) and spec version so clients can tell. MCP Tasks and webhooks stay out of scope.

## Design

The design is the set of sections below: evidence base, internal/external boundary, tool naming and classification, permission model, event design, remote access, headless entry, and the decisions that need user confirmation. The confirmed decisions above authorize bounded implementation. Network exposure remains explicitly enabled by the operator.

## Evidence base (phase 0, read-only)

- First-party inventory: ~285 operable actions, but most plugin commands (`CoreApp::call`) are reachable only inside the desktop host; remote clients get the `Op` bus on `/ws`, a 30-command allowlist on `/api/web-ui/call`, and the T3 compat layer. Shared mutation entry points: `Engine::submit(Op)`, `deliver_prompt`/`enqueue_prompt`, `answer_permission`, `answer_elicitation`, `create_session`, `rename_session`, `set_archived`, `set_pinned`, `list_sessions`, `transcript_page`, worktree helpers. Subagents have no command surface, only `Event::SubagentUpdated`.
- T3 Code (snapshot `t3code-read-host`): one HTTP `/mcp` with Bearer scoped to environment/parent thread/provider/capability set (`orchestration|worktree|pull-requests|preview|device`), 24 h idle TTL, revoked on session end; names `t3_<domain>_<action>` plus an unprefixed orchestration core; annotations map to MCP hints; destructive operations need a full-access, default-mode caller; `t3_thread_wait`/`task_status` poll with a bounded timeout and a timeout never cancels work; `t3_pending_request_*` handles user-input questions only and is explicitly not a permission-approval channel.
- Cloudflare: T3's remote path is a managed `cloudflared tunnel run` with the connector token only in `TUNNEL_TOKEN`, output redaction, restart with backoff, ingress to `127.0.0.1:<port>` and 404 for everything else; application auth stays pairing, Bearer, WS ticket. `cloudflared` is not installed here and is not bundled by T3.
- herdr (docs, not locally checked out): resident server, Unix-socket NDJSON API, thin CLI, `--json`, `status`, `agent prompt --wait`, `wait` commands, exit code 1 error / 2 timeout.
- MCP events (sources verified reachable by HTTP 200 on 2026-10-06; claims below come from the research notes and must be re-read when implemented):

| Item | Status | Version / date | Source |
| --- | --- | --- | --- |
| `notifications/progress`, `notifications/cancelled`, `resources/subscribe` + `notifications/resources/updated`, `*/list_changed` | STABLE | 2025-06-18 / 2025-11-25 | https://modelcontextprotocol.io/specification/2025-11-25/server/resources |
| Streamable HTTP with GET SSE and `Last-Event-ID` resumption | STABLE | 2025-06-18; SEP-1699 | https://modelcontextprotocol.io/specification/2025-06-18/basic/transports |
| `subscriptions/listen` + `notifications/subscriptions/acknowledged`, stateless transport | STABLE in the 2026-07-28 revision (SEP-2575) | 2026-07-28 | https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions |
| Core Tasks + `notifications/tasks/status` | EXPERIMENTAL | 2025-11-25 | https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks |
| Tasks extension `io.modelcontextprotocol/tasks` (SEP-2663) | EXTENSION | 2026-04-27 | https://modelcontextprotocol.io/seps/2663-tasks-extension |
| Events / Triggers (poll, push, webhook) | DRAFT, experimental working group | design sketch 2026-02 | https://github.com/modelcontextprotocol/experimental-ext-triggers-events |
| Webhooks in core (PR 593), LLM re-entry (SEP-2495) | PROPOSAL | open | https://github.com/modelcontextprotocol/modelcontextprotocol/pull/593 |
| Logging and sampling | DEPRECATED (SEP-2577) | 2026 | https://modelcontextprotocol.io/seps/2577-deprecate-roots-sampling-and-logging |

There is no standard MCP event type for "approval needed" or "question needed"; CodeTwo defines its own envelope and maps it to stable primitives only.

## Boundary: internal host MCP vs external MCP

| | Internal host MCP (existing, `host_mcp/`) | External MCP (new, `external_mcp/`) |
| --- | --- | --- |
| Caller | the agent running inside one live session | an external client/agent/CLI the user authorized |
| Principal | session id | client id (name, scopes, project allowlist) |
| Credential | per-session 32-byte Bearer, hash only, in memory, dies with the session | `ctmcp_` + 32 random bytes, SHA-256 hash only, persisted in `<data-dir>/external-mcp-clients.json` (0600, atomic write), expiry, revocable |
| Route | `POST /mcp`, loopback only | `POST /external-mcp` (+ optional GET SSE), different prefix so tokens are never interchangeable |
| Default | off (`CODETWO_HOST_MCP=1`) | off (`CODETWO_EXTERNAL_MCP=1` or `--external-mcp`) |
| Scope of tools | read only, own session | catalog below, filtered per credential |

Decision: parallel module, shared plumbing. The baseline JSON-RPC handler in `host_mcp/protocol.rs` is generalized over a small `McpToolHost` trait (`catalog(&self) -> Vec<Value>`, `call(&self, name, args) -> Result<Value, String>`, server name/capabilities); both surfaces implement it. Credential registries, authorize gates and audit are separate types; neither surface accepts the other's token (prefix check before hashing).

## Tool naming and classification

Convention: `codetwo_<domain>_<verb>`, lower snake case, a single closed verb set: `list`, `read`, `search`, `create`, `update`, `delete`, `send`, `stop`, `wait`, `respond`, `run`, `set`. The singleton `codetwo_capabilities` is kept. Existing names `codetwo_capabilities`, `codetwo_session_list`, `codetwo_session_read` already conform and are not renamed (providers already hold them). Reserved legacy names (`delegate_task`, ...) stay refused.

Classes (one per tool, stored in the registry and emitted as annotations):

| Class | Meaning | Annotations |
| --- | --- | --- |
| R | read only, bounded | `readOnlyHint=true` |
| W | reversible write, no data loss | `readOnlyHint=false, destructiveHint=false`; `idempotentHint` set where true |
| X | starts or steers agent execution, or answers an agent request | like W plus `openWorldHint=true` |
| D | deletes data, loosens execution policy, or has external effect | `destructiveHint=true`; not in the default catalog |

Domains and first catalog (✓ wave 1 read, ✓✓ wave 2 write):

| Domain | Tools | Class | Scope |
| --- | --- | --- | --- |
| capabilities | `codetwo_capabilities` ✓ | R | read |
| project | `project_list` ✓, `project_read` ✓, `project_create` ✓✓ (add path), `project_update` ✓✓ (rename, defaults), `project_delete` (D, deferred) | R/W/D | read / operate / admin |
| session | `session_list` ✓, `session_read` ✓, `session_search` ✓, `session_create` ✓✓, `session_update` ✓✓ (rename, pin, archive), `session_delete` (D, deferred) | R/W/D | read / operate / admin |
| transcript | `transcript_read` ✓ (paged, bounded) | R | read |
| turn | `turn_send` ✓✓ (modes prompt, queue, steer), `turn_stop` ✓✓, `turn_wait` ✓✓ (bounded wait) | X/X/R | operate / operate / read |
| approval | `approval_list` ✓, `approval_respond` ✓✓ | R/X | read / approve |
| question | `question_list` ✓, `question_respond` ✓✓ | R/X | read / operate |
| model | `model_list` ✓, `model_set` ✓✓ | R/W | read / operate |
| policy | `policy_read` ✓, `policy_set` (tightening only) ✓✓; loosening is D, deferred | R/W/D | read / operate / admin |
| worktree | `worktree_list` ✓, `worktree_delete` (D, deferred) | R/D | read / admin |
| git, workspace | `git_status`, `git_diff`, `workspace_file_read`, `workspace_search` (bounded, project-confined, no secrets paths) | R | read |
| automation | `automation_list` ✓, `automation_run` ✓✓; create/update/delete deferred | R/X | read / operate |
| scene, pipeline | `scene_list`, `pipeline_list` (read), run deferred | R | read |
| events | `events_poll` ✓, `events_wait` ✓✓ | R | read |
| subagent | `subagent_list` ✓ (from `SubagentRun` records) | R | read |

Not exposed (desktop-only, no headless meaning): dialogs, BrowserView, appshot, keymap, voice, window actions. Plugin management, provider install/remove, terminal PTY, device sync and canvas stay off the external surface in this change.

## Permission model

1. Scopes (explicit, not hierarchical): `read`, `operate`, `approve`, `admin`. `admin` does not imply `approve`.
2. Project confinement: each credential has `projects: ["*"] | [canonical path...]`. Every tool resolves its target (session to project, project path, worktree) and denies when outside the set. `project_list` and `session_list` filter silently; direct reads of an out-of-scope id return `not found`. Paths are canonicalized; `..` and symlink escape are rejected.
3. One authorize gate: `Engine::authorize_external_mcp_call(resolved, tool, args)`: credential live and unexpired, tool registered, scope held, class allowed by server flags, project confinement, rate limit, then the Engine execution policy of the target session for X/W tools, then dispatch. Every denial and success writes an audit record before dispatch (fail closed if the audit sink fails for W/X/D).
4. No escalation: an external client cannot raise a session's permission mode or sandbox above the current value; `policy_set` accepts only equal or stricter values. Loosening is class D and deferred.
5. Approvals: `approval_respond` needs scope `approve`, which is never granted by default and can only be added when creating a client with `--allow-approve`. The tool echoes the request's tool name and kind from the live request, refuses stale or already-resolved ids, and records the client id. An external agent approving another agent's tool request is the highest-risk operation here; the user may decide to leave it unimplemented.
6. Destructive (class D) tools: not registered unless the server runs with `CODETWO_EXTERNAL_MCP_ADMIN=1` and the credential holds `admin`; wave 1 and 2 implement none of them.
7. Limits: request body 256 KiB; response 256 KiB with a `truncated` marker; list tools max 50 items per page with opaque cursors; per-client token bucket 120 calls/min read, 30/min W/X/D; `*_wait` timeout capped at 55 s; at most 8 concurrent SSE streams per client; at most 64 clients; text fields are length-bounded and control-character stripped.
8. Lifecycle: tokens shown once at creation (stdout or a 0600 file, never argv, never logged); default TTL 30 days, maximum 90 days, no sliding renewal; `revoke` takes effect on the next call and closes the client's SSE streams; unknown tokens cost a constant-time hash compare and are rate-limited per peer; credential file read errors disable the surface.
9. Audit: append-only `<data-dir>/external-mcp-audit.jsonl` (rotation at 5 MiB, 3 files) with timestamp, client id, tool, class, allowed, reason code, session id, project id, argument hash, duration; never tokens, prompt text, tool arguments or file contents.
10. Transport guards: `Origin` must be absent or loopback unless a configured remote hostname is allowed; `Host` must be loopback or an explicitly configured hostname; Bearer is required in all cases (a loopback peer does not mean local once a tunnel is running).

## Event design

Envelope (CodeTwo-owned, stable, versioned `v1`):

```json
{"v":1,"cursor":"<epoch>:<seq>","ts":"RFC3339","type":"turn.completed","session_id":"..","project_id":"..","turn_id":"..","data":{}}
```

Types: `turn.started`, `turn.completed`, `turn.cancelled`, `turn.failed`, `approval.requested`, `approval.resolved`, `question.requested`, `question.resolved`, `session.created`, `session.updated`, `session.broken` (from `ThreadDisposition::Broken`), `subagent.updated`. Queue and automation events are not advertised until a real producer is integrated. Mapping from `Event`: `TurnEnded` with its reason to completed/cancelled/failed, terminal `Error` to `turn.failed`, `PermissionRequest` to `approval.requested`, `ElicitationRequest` to `question.requested`, `ThreadDisposition` to `session.broken`, `SubagentUpdated` to `subagent.updated`.

Content policy: ids, enum reasons and counters only, plus a title or tool name truncated to 120 characters. No prompts, assistant text, tool arguments, diffs, paths outside the project id, or secrets. Details are fetched with an authorized read tool (`approval_list`).

Delivery: bounded in-memory ring (2048 events, monotonically increasing `seq`, random `epoch` per process so a restart is detected rather than silently replayed). Consumers filter by the credential's project set before any event leaves the process. `events_poll {cursor, types?, session_id?, limit}` returns events and `next_cursor`, or `reset:true` plus the oldest cursor when the cursor is older than the ring or from another epoch (no replay guessing). `events_wait` is the same with a bounded wait. Delivery is at-least-once with stable cursors; consumers dedupe by cursor.

MCP mapping: tools `events_poll`/`events_wait`/`turn_wait` are the baseline. The 2025-06-18 compatibility path supports GET SSE resource-update notifications and resource reads. The 2026-07-28 subscription pattern uses POST `subscriptions/listen` SSE with an acknowledgment before notifications. The experimental Events extension uses poll and POST push streams. These are separate adapters over the same envelope; this does not claim full conformance to every 2026-07-28 protocol feature. Tasks and webhook delivery remain excluded. Approval and question prompts are not mapped to client-user elicitation.

## Remote access

- Built in as an optional supervisor, not as a bundled binary: `codetwo serve --remote cloudflared` starts the user's own `cloudflared` (explicit `--cloudflared-path` or PATH lookup; no download, no install). Mode `named` only: `cloudflared tunnel --no-autoupdate run` with the connector token read from `CODETWO_TUNNEL_TOKEN_FILE` (or the existing `TUNNEL_TOKEN` env), passed to the child via its environment only. Quick tunnels are refused unless `--allow-quick-tunnel` is also given (experimental, prints a warning, URL printed locally only).
- Refuses to start unless: a public hostname is configured (`--remote-host`), the server has authentication enabled (it always does), and the user has explicitly passed the remote flag. Default is off in every launch surface and in saved settings.
- Authentication: unchanged pairing token to device Bearer to WS ticket. Remote mode additionally (a) shortens pairing token TTL to 10 minutes and single use, (b) rate-limits and locks out pairing and Bearer failures per source (the tunnel peer is loopback, so the limiter keys on `CF-Connecting-IP` only when the request came through the supervised tunnel, otherwise on peer), (c) applies a public-route allowlist by `Host`: for a non-loopback `Host`, only `/health`, `/api/pair`, `/api/ws-ticket`, `/ws`, `/api/web-ui/call`, `/external-mcp` and Web UI static assets are reachable; `/mcp` (session channel), `/terminal`, `/api/terminals*`, `/ws/terminal` and everything else return 404.
- Child management: restart with exponential backoff, stop on server shutdown, stdout/stderr passed through a redactor that removes the token and any `cloudflared` connector JWT shape; the token never appears in argv, logs, diagnostics or the saved settings; the credential file must be 0600.
- The user must create and own the Cloudflare tunnel and its access policy; Cloudflare Access in front of the hostname is recommended in the docs but not required by code.

## Headless

- One new binary `codetwo` in `crates/server` (existing `codetwo-server` and `codetwo-agent` keep working unchanged).
- `codetwo serve [--data-dir P] [--bind 127.0.0.1:PORT] [--webui] [--external-mcp] [--remote cloudflared ...]`: starts Core + server exactly as `codetwo-server` does, one data-directory owner enforced by the existing lock; prints a pairing link only to local stdout; writes `<data-dir>/server.json` (pid, port, version; no secrets).
- Client commands talk to the running server over the external MCP endpoint (same operations, same gate, same audit), never start a second Core: `status`, `session list|read`, `send`, `wait`, `stop`, `approve|answer`, `events`, `pair`, `mcp client create|list|revoke`. `--json` everywhere; exit codes 0 ok, 1 error, 2 timeout, 3 needs attention (approval or question pending). Credentials come from `CODETWO_TOKEN`, `--token-file`, or `<data-dir>/cli.token` (0600), never from argv. `mcp client create|revoke` edit the credential file directly (requires data-dir access = local owner) and the running server re-reads it by mtime.
- `pair` imports an existing external MCP credential from protected file or redirected stdin, verifies it with capabilities, then saves `cli.token` and `cli.url`. It is distinct from Web UI device pairing and does not create any network issuance endpoint.
- Shared core operations: a typed `ops` layer in `crates/core/src/external_mcp/ops.rs` implemented once over `Engine`; MCP tools and the CLI (through MCP) both use it. The T3-compat and WS handlers are not rewritten in this change.

## Acceptance criteria

- [x] AC-1: Internal and external MCP surfaces are parallel with separate credentials; the shared JSON-RPC handler is generic; internal behavior and tests are unchanged and a session token is rejected on the external route and vice versa.
- [x] AC-2: The external tool registry defines every tool once with name, class, scope and annotations; a test enforces the naming convention, unique names, class/annotation consistency, and that every tool has exactly one authorize path.
- [x] AC-3: Client credentials are hash-only, expiring, revocable, project-scoped, persisted 0600, never logged; tests cover expiry, revoke, scope denial, project denial, unknown token, and token absence from audit, logs and diagnostics.
- [x] AC-4: Read tools work for an authorized client through `POST /external-mcp` on a temporary port; non-loopback peer/Host/Origin, oversize bodies, missing Bearer, and over-rate calls are rejected.
- [x] AC-5: Write tools (create project/session, rename/pin/archive, send/queue/steer, stop, question respond, approval respond, tighten policy, set model) go through the single gate, honor project confinement and execution policy, and write an audit record before dispatch; D-class tools are listed only for admin credentials and require confirm:true plus matching expected_id.
- [x] AC-6: Event envelope, bounded ring with epoch cursors, project filtering, `events_poll`/`events_wait`/`turn_wait`, and the stable MCP mapping exist; content policy test proves no prompt/tool-argument text leaks; upstream draft protocols are labelled with their status and version, supported without an additional flag as confirmed above.
- [x] AC-7: Remote supervisor is default off, refuses unsafe configuration, never exposes tokens in argv/logs, enforces the public-route allowlist and rate limits; tested with a fake `cloudflared` executable (no real tunnel, no network).
- [x] AC-8: `codetwo` binary provides serve/status/session/send/wait/stop/approve/answer/events/pair/mcp client commands with JSON output and the documented exit codes, tested against an in-process temporary server.
- [x] AC-9: Documentation for the tool catalog, scopes, event envelope and remote setup exists and passes `bun script/verify/docs.ts`; `bun script/verify/sdlc.ts --worktree` passes.

## Original proposal (historical; superseded by Confirmed decisions)

1. Enable the external MCP network surface at all (default off; loopback plus optional tunnel). Recommended: yes, default off.
2. Long-lived client credentials: 30 d default, 90 d max, file-backed hash store, no sliding renewal, issued only by a local CLI command. Recommended: approve.
3. Write tools of classes W/X in wave 2, project-confined, audited. Recommended: approve, limited to the table above.
4. Scope `approve` (an external agent answers tool-permission requests). Recommended: implement but never grant by default; or leave unimplemented. Needs an explicit decision.
5. Class D tools (delete project/session/worktree, loosen policy, automation create/update/delete): recommended not to implement in this change; they need a human-confirmation channel designed separately.
6. Remote access: optional external `cloudflared`, named tunnel only, quick tunnel experimental behind a second flag; Host-based public route allowlist. Recommended: approve default off; no real tunnel is started during development.
7. Event content policy (ids, reasons and truncated titles only) and in-memory ring without persistence. Recommended: approve.
8. Experimental `subscriptions/listen` support behind a flag; no Tasks, no Events draft. Recommended: approve.
