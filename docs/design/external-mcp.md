# External MCP, events, remote access, and headless CLI

Status: implemented in Core and server for MCP, credentials, events, and Cloudflare remote supervision; the unified `codetwo` headless binary is implemented, with final integration verification in progress. Default for every launch surface: external MCP is **off** until explicitly enabled.

This document matches the confirmed product decisions in [External MCP, Events, Remote Access, Headless](../sdlc/changes/2026-10-06-external-mcp-events-remote-headless/spec.md) (2026-10-06). Verify behavior against `crates/core/src/external_mcp/` and `crates/server/src/{external_mcp.rs,external_mcp_stream.rs,remote.rs}`.

## Internal host MCP vs external MCP

Both surfaces share the generic JSON-RPC handler (`McpToolHost` in `host_mcp/protocol.rs`). Credentials, authorize gates, and audit sinks are separate; tokens are never interchangeable (prefix check before hashing).

| | Host MCP (`host_mcp/`) | External MCP (`external_mcp/`) |
| --- | --- | --- |
| Caller | Provider/agent inside one live session | External client, agent, or automation the user authorized |
| Principal | Session id | Client id (name, scopes, project allowlist) |
| Credential | Per-session bearer, hash only, in memory, dies with session | `ctmcp_` + random secret, SHA-256 hash only, persisted |
| Route | `POST /mcp` (session channel) | `POST /external-mcp`, optional `GET /external-mcp` SSE |
| Enable | `CODETWO_HOST_MCP=1` | `CODETWO_EXTERNAL_MCP=1`, desktop Settings → External MCP, or `codetwo serve --external-mcp` |
| Tools | Read-only, own session | Closed catalog below, filtered per credential |
| Server name | `codetwo` | `codetwo-external-mcp` |

Host MCP remains loopback-oriented for providers. External MCP adds transport guards (peer, `Host`, `Origin`) and optional tunnel hostname allowlists.

## Enabling and endpoint

1. **Environment:** set `CODETWO_EXTERNAL_MCP=1` (or `true`) before starting `codetwo-server` / the desktop Core owner. The server prints `external MCP enabled (POST /external-mcp on this server)` when the credential store initializes under the data directory.
2. **Desktop:** Settings → External MCP toggles `external_mcp.set_enabled` and persists `external-mcp-settings.json` under the data directory (via the `external_mcp` app plugin).
3. `codetwo serve --external-mcp` on the headless entry binary (see [Headless CLI](#headless-cli)).

When disabled, `POST` and `GET` on `/external-mcp` return **404**.

**Endpoint:** `http://127.0.0.1:<port>/external-mcp` (Streamable HTTP, protocol version **2025-06-18**). All requests require `Authorization: Bearer <token>` where `<token>` starts with `ctmcp_`. Session host tokens are rejected on this route.

## Credential lifecycle

**Issuance**

- Desktop Settings → External MCP → Create client (name, scopes, projects, TTL).
- Core plugin commands `external_mcp.clients.create` / `list` / `revoke` (same registry).
- **Planned:** `codetwo mcp client create|list|revoke` with local data-directory access.

**Token:** shown once at creation (UI dialog or command result). Never pass on argv; do not log. Desktop snippet uses a paste placeholder; use `<TOKEN>` in docs and configs.

**Storage:** `<data-dir>/external-mcp-clients.json`, mode **0600**, atomic write. Only SHA-256 hashes are stored. The registry reloads on credential resolution and uses a cross-process lock for mutations, so local revoke/create is visible without restart.

**TTL (confirmed):**

| Choice | Behavior |
| --- | --- |
| Default | 30 days (`DEFAULT_TTL` in `clients.rs`) |
| Custom days | Any positive day count (no 90-day cap in code) |
| Permanent | `expires_at` absent; must be chosen explicitly; still revocable |

**Scopes:** `read`, `operate`, `approve`, `admin` (flat, not hierarchical). If the caller omits scopes at create time, the registry defaults to **read + operate + approve** (convenience-first; UI shows checkboxes). **`admin` is never default** — it unlocks class **D** tools only when explicitly granted.

**Projects:** `all` or a list of canonical paths. Every tool resolves its target project and denies out-of-scope access. Lists filter silently; direct reads return **not found**. Paths reject `..` and symlink escape via canonicalization.

**Revoke:** immediate on next resolve; SSE streams for that client are closed within the subscription registry grace window (~5s).

**Limits:** at most **64** active clients; unknown tokens use constant-time hash compare; auth failures per peer are rate-limited on the HTTP layer (20 failures / 60s → 429).

**Audit:** append-only `<data-dir>/external-mcp-audit.jsonl`, rotate at **5 MiB**, keep **3** files. Fields: timestamp, client id, tool, class, allowed, reason code, session/project ids, argument hash, duration. Never tokens, prompts, tool arguments, or file contents. W/X/D denials and successes fail closed if the audit sink errors.

## Tool catalog

Naming: `codetwo_<domain>_<verb>` (singleton `codetwo_capabilities`). Classes: **R** read, **W** reversible write, **X** execution/steer/respond, **D** destructive/administrative.

**Class D (destructive):** listed only when the credential includes **`admin`**. Annotations: `destructiveHint=true`, description prefix `[DESTRUCTIVE]`, `_meta["codetwo/risk"]="destructive"`. Every D call requires `confirm: true` and `expected_id` equal to the target id field for that tool (`path`, `session_id`, `id`, etc. per tool schema); otherwise the gate refuses with no effect.

**Default catalog** (no `admin`): all **R**, **W**, and **X** tools below that are not marked deferred in code (`default_catalog()` excludes class D).

| Tool | Class | Scope | Description |
| --- | --- | --- | --- |
| `codetwo_capabilities` | R | read | External MCP capabilities, scopes, waves, and experimental flags. |
| `codetwo_project_list` | R | read | List projects visible to the caller. |
| `codetwo_project_read` | R | read | Read one project by path or id. |
| `codetwo_project_create` | W | operate | Create a project at a path. |
| `codetwo_project_update` | W | operate | Update project metadata (rename, worktree mode, agent defaults). |
| `codetwo_project_delete` | D | admin | [DESTRUCTIVE] Unregister a project (never deletes user files). |
| `codetwo_session_list` | R | read | List sessions filtered to allowed projects. |
| `codetwo_session_read` | R | read | Read session metadata. |
| `codetwo_session_search` | R | read | Search sessions by title or id fragment. |
| `codetwo_session_create` | W | operate | Create a session in a project. |
| `codetwo_session_update` | W | operate | Update session rename, pin, or archive flags. |
| `codetwo_session_delete` | D | admin | [DESTRUCTIVE] Delete a session record. |
| `codetwo_transcript_read` | R | read | Read a bounded transcript page. |
| `codetwo_turn_send` | X | operate | Send, queue, or steer a turn. |
| `codetwo_turn_stop` | X | operate | Stop the active turn for a session. |
| `codetwo_turn_wait` | R | read | Wait for turn completion with bounded timeout. |
| `codetwo_approval_list` | R | read | List pending approval requests. |
| `codetwo_approval_respond` | X | approve | Respond to a pending approval request. |
| `codetwo_question_list` | R | read | List pending user questions. |
| `codetwo_question_respond` | X | operate | Answer a pending question. |
| `codetwo_model_list` | R | read | List selectable models for a session. |
| `codetwo_model_set` | W | operate | Set the model for a session. |
| `codetwo_policy_read` | R | read | Read execution policy for a session. |
| `codetwo_policy_set` | W | operate | Tighten execution policy (never loosen). |
| `codetwo_worktree_list` | R | read | List worktrees for a project. |
| `codetwo_worktree_delete` | D | admin | [DESTRUCTIVE] Discard an orphan session worktree checkout. |
| `codetwo_git_status` | R | read | Git status for a project worktree. |
| `codetwo_git_diff` | R | read | Bounded git diff for a project worktree. |
| `codetwo_workspace_file_read` | R | read | Read a bounded workspace file within the project. |
| `codetwo_workspace_search` | R | read | Search workspace files within the project. |
| `codetwo_automation_list` | R | read | List automations. |
| `codetwo_automation_run` | X | operate | Run an automation once. |
| `codetwo_automation_create` | D | admin | [DESTRUCTIVE] Create a scheduled automation. |
| `codetwo_automation_update` | D | admin | [DESTRUCTIVE] Update a scheduled automation. |
| `codetwo_automation_delete` | D | admin | [DESTRUCTIVE] Delete a scheduled automation. |
| `codetwo_scene_list` | R | read | List scenes. |
| `codetwo_scene_run` | D | admin | [DESTRUCTIVE] Run a scene against a session. |
| `codetwo_pipeline_list` | R | read | List pipelines. |
| `codetwo_pipeline_run` | D | admin | [DESTRUCTIVE] Start a pipeline instance. |
| `codetwo_events_poll` | R | read | Poll events since a cursor. |
| `codetwo_events_wait` | R | read | Wait for events with bounded timeout. |
| `codetwo_subagent_list` | R | read | List subagent runs for a session. |

`codetwo_turn_send` returns a non-final receipt (`final:false`) with `delivery_id`, `session_id`, `state` and a nullable `turn_id`, and `no_retry:true`. Unknown submission outcomes are returned as receipts with `state:"unknown"`, rather than losing the delivery id in an error. A submitted or queued receipt does not prove execution or completion. Preserve the delivery id and use `codetwo_turn_wait`/events for the final result; never replay an operation whose result is unknown. Wait responses include `delivered`, `no_retry`, `reason` and `meaning`; an evicted or unavailable result is `unknown` with `no_retry:true`. Only a confirmed pre-delivery rejection or cancellation reports `delivered:false`. The policy setter checks both axes against the locked current state at commit time; model changes reread the effective stored result. Scene/pipeline rollback tightens each axis without overwriting concurrent human tightening. If queued prompts cannot be cancelled, stop returns Conflict even though cancellation of the active turn was requested. Workspace search shares a 10-second budget across roots and returns `incomplete:true` if that budget expires. Git status/diff for a project inside a repository are confined to the project, including both sides of a rename.

**Transport limits:** request body **256 KiB**; tool JSON responses capped at **256 KiB** with a `truncated` preview when exceeded; list pages default/limit **50** items; per-client token bucket **120/min** read and **30/min** W/X/D; `*_wait` timeout max **55 s**; at most **8** concurrent SSE streams per client.

**Not on this surface:** desktop-only UI, plugin install, terminal PTY, device sync, canvas, and legacy reserved names (for example `delegate_task`) stay refused.

## Client configuration and HTTP examples

Generic MCP client config (replace `<TOKEN>` after issuance):

```json
{
  "mcpServers": {
    "codetwo": {
      "url": "http://127.0.0.1:4599/external-mcp",
      "headers": {
        "Authorization": "Bearer <TOKEN>"
      }
    }
  }
}
```

**Initialize**

```bash
curl -sS http://127.0.0.1:4599/external-mcp \
  -H 'Authorization: Bearer <TOKEN>' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"example","version":"0"}}}'
```

**Tools list**

```bash
curl -sS http://127.0.0.1:4599/external-mcp \
  -H 'Authorization: Bearer <TOKEN>' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

**Tools call** (read capability metadata)

```bash
curl -sS http://127.0.0.1:4599/external-mcp \
  -H 'Authorization: Bearer <TOKEN>' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"codetwo_capabilities","arguments":{}}}'
```

## Events

### CodeTwo envelope (stable, version `v1`)

```json
{
  "v": 1,
  "cursor": "<epoch>:<seq>",
  "ts": "2026-10-06T12:00:00Z",
  "type": "turn.completed",
  "session_id": "...",
  "project_id": "...",
  "turn_id": "...",
  "data": {}
}
```

**Kinds:** `turn.started`, `turn.completed`, `turn.cancelled`, `turn.failed`, `approval.requested`, `approval.resolved`, `question.requested`, `question.resolved`, `session.created`, `session.updated`, `session.broken`, `subagent.updated`.

**Content policy:** ids, enum reasons, counters, and titles/tool names truncated to **120** characters. No prompts, assistant text, tool arguments, diffs, out-of-project paths, or secrets. Use `codetwo_approval_list` / read tools for detail.

**Ring:** in-memory, capacity **2048**, monotonic `seq`, random **epoch** per process. Project filter follows the credential before any event leaves the process.

**Cursors:** `epoch:seq`. If the cursor epoch differs or seq fell off the ring, poll returns `reset: true` and `oldest_cursor` — consumers must not guess replay. Delivery is at-least-once; dedupe by cursor.

**Tools:** `codetwo_events_poll` and `codetwo_events_wait` (bounded wait, same cap as turn wait). There is no standard MCP notification for “approval needed”; CodeTwo does not map approvals to MCP `elicitation` on this surface.

### MCP mapping and upstream protocol status

Implemented adapters and relevant upstream mechanisms:

| Mechanism | Label | Version / source |
| --- | --- | --- |
| `codetwo_events_poll` / `codetwo_events_wait` / `codetwo_turn_wait` | CodeTwo stable | Envelope `v1` |
| Streamable HTTP **GET** SSE on `/external-mcp`, `Last-Event-ID` | **STABLE** | [Transports 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports); [SEP-1699](https://modelcontextprotocol.io/seps/1699-support-sse-polling-via-server-side-disconnect) |
| `resources/subscribe` + `notifications/resources/updated` for `codetwo://events` | **STABLE** | [Resources 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/server/resources) |
| `notifications/tools/list_changed`, progress, cancelled, list_changed variants | Upstream **STABLE**, not advertised or emitted by this surface | 2025-06-18 / 2025-11-25 family |
| `subscriptions/listen` + `notifications/subscriptions/acknowledged` | **STABLE** @ 2026-07-28 | [Subscriptions 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions) (no extra env flag; labelled in `codetwo_capabilities` / `mcp_events::capabilities`) |
| MCP Events extension `io.modelcontextprotocol/events` (`events/list`, `events/poll`, push via `events/stream`) | **DRAFT** (non-standard) | [design sketch 2026-02-19](https://github.com/modelcontextprotocol/experimental-ext-triggers-events/blob/main/docs/design-sketch-proposal.md); poll/push implemented; **webhook** (`events/subscribe`) **not implemented** |
| MCP Tasks (`notifications/tasks/status`, SEP-2663 extension) | **EXPERIMENTAL / extension** | [Tasks 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks), [SEP-2663](https://modelcontextprotocol.io/seps/2663-tasks-extension) — **not implemented** |
| Core webhooks (PR 593), LLM re-entry (SEP-2495) | **PROPOSAL** | Not implemented |

**2026-07-28 note:** the current MCP *Current* revision deprecates standalone GET SSE + `Last-Event-ID` in favor of `subscriptions/listen` on the POST stream ([streamable-http 2026-07-28](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx)). CodeTwo intentionally implements **both** the 2025-06-18 GET resumption path and 2026-07-28 listen for client compatibility; capabilities advertise each with its spec id.

**Resources:** `resources/list` exposes `codetwo://events`. Subscribe via `resources/subscribe`, or `subscriptions/listen` with `resourceSubscriptions` for `codetwo://events`. The acknowledgment reports only this supported resource subset; tools-list filters and draft event-name filters are not honored by listen. Draft event-name subscriptions use `events/stream`. Each client has at most 8 simultaneous streams; there is no separate 32-subscription limit.

## Remote access (Cloudflare)

Optional supervisor in `remote.rs`; **default off**. CodeTwo does not bundle `cloudflared`.

**Named tunnel (production path)**

1. In Cloudflare Zero Trust / dashboard: create a **named tunnel**, copy the connector token, configure a **public hostname** → `http://127.0.0.1:<local-port>` (ingress); restrict other paths at the edge if desired.
2. Store the token in a file with mode **0600** (or set `TUNNEL_TOKEN` in the process environment only — never argv or logs).
3. Start the server with remote flags, for example on `codetwo-server` argv after the subcommand:  
   `--remote cloudflared --remote-host remote.example.com --tunnel-token-file /path/to/token`  
   Optional: `--cloudflared-path /usr/local/bin/cloudflared`.
4. When remote mode starts, the server sets tunnel hostname allowlists for external MCP and installs `remote_access_guard`. Pairing uses hardened TTL (**10 minutes**) and stricter auth rate limits; limiter keys prefer `CF-Connecting-IP` when the request came through the tunnel.

**Quick tunnel:** requires **`--allow-quick-tunnel`** (experimental); refused otherwise. URL is printed locally only.

**Public route allowlist** (non-loopback `Host` must match `--remote-host`):  
`/health`, `POST /api/pair`, `POST /api/ws-ticket`, `/ws`, `/api/web-ui/call`, `/external-mcp`, Web UI static (`/`, `/pair`, recognized static extensions outside API namespaces; traversal, hidden segments and percent-encoded paths are refused).  
**Blocked examples:** `/mcp` (host session MCP), `/terminal`, `/api/terminals*`, `/ws/terminal`, `/api/canvas`, `/api/team`, handoffs, and similar admin paths → **404**.

**Recommendations:** place [Cloudflare Access](https://developers.cloudflare.com/cloudflare-one/policies/access/) (or equivalent) in front of the public hostname; keep external MCP off until needed; prefer named tunnels over quick tunnels; rotate tunnel tokens if leaked. The supervisor redacts connector JWT-shaped output and restarts `cloudflared` with backoff.

Quick tunnel requests with a public Host are refused until the hostname is discovered from stdout or stderr. A Cloudflare request that rewrites Host to loopback is still limited to the public route set when Cloudflare edge headers are present. This is a Host/header boundary, not cryptographic proof of the transport source.

**Risks:** a tunnel exposes pairing and `/external-mcp` to the Internet if Access is absent; long-lived `ctmcp_` tokens with default **approve** scope can answer permission prompts; permanent credentials require explicit user choice.

## Headless CLI

Workspace search requires `ripgrep` (`rg`) on the server process PATH. Install it on headless hosts as well as desktop hosts; unavailable search returns an operation error. CI installs and checks this dependency before Rust integration tests.

The `codetwo` binary is registered in `crates/server/Cargo.toml` and shares the server entry implementation. Client commands use the external MCP gate.

| Command | Purpose |
| --- | --- |
| `codetwo serve [webui] [--data-dir] [--bind] [--external-mcp] [--remote cloudflared …]` | One Core + server owner; `server.json` metadata without secrets |
| `codetwo status` | Server reachability |
| `codetwo session list\|read`, `send`, `wait`, `stop` | Session operations via external MCP |
| `codetwo approve`, `answer` | Pending approval / question helpers |
| `codetwo events` | Event poll/wait wrapper |
| `codetwo pair` | Import and validate an external MCP credential; separate from Web UI device pairing |
| `codetwo mcp client create\|list\|revoke` | Local credential file edits (server reloads by mtime) |

`codetwo-server serve` and `codetwo serve` use one shared Core startup path and hold the data-directory instance lock. `codetwo-server pair --data-dir <path>` requests a fresh Web UI device-pairing link from either running owner; it does not mint an external MCP credential. The daemon binds loopback by default and can serve remote C2 commands without a bundled Web UI. Its `--public-url` controls the advertised pairing address.

`codetwo pair --url https://host (--token-file FILE | --stdin)` validates one external credential through the read-only capabilities tool before saving `cli.token` (0600) and `cli.url`. Plain HTTP is permitted only for loopback. stdin must be redirected rather than an echoing terminal. No redirects or automatic retries are used. Pairing does not mint a new credential or exchange a Web UI device Bearer. URL discovery order: `--url`, `CODETWO_URL`, saved `cli.url`, then local `server.json`.

**Credentials for CLI:** `CODETWO_TOKEN`, `--token-file`, or `<data-dir>/cli.token` (0600), never argv. **`--json` on all commands.**

**Exit codes:** `0` success, `1` error, `2` timeout, `3` needs attention (approval or question pending).

Alternatively, use `CODETWO_EXTERNAL_MCP=1` with `codetwo-server`, issue tokens from desktop Settings or `external_mcp.clients.create`, and call `/external-mcp` directly.

## Security notes and known limits

- **Default approve scope** on new clients is convenient but high impact; untick in UI or pass explicit scopes without `approve` when creating credentials.
- **Permanent credentials** stay revocable but never expire; use only when necessary.
- **Admin scope** unlocks D tools with confirm/expected_id — still no permission-mode escalation via `policy_set` (tighten only).
- **Loopback is not implicit trust** once a tunnel is active: Bearer is always required; `Host`/`Origin` must match loopback or configured tunnel hostnames.
- **Unsupported by design:** MCP Tasks, webhook event delivery, plugin/terminal/device surfaces, and treating MCP Events draft as a normative standard.
- **Host vs external:** never send `ctmcp_` tokens to `/mcp` or session host tokens to `/external-mcp`.
