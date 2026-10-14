---
id: 2026-10-13-shared-agent-runtime
schema: 5
stage: spec
status: accepted
owner: research-shared-agent r5 executor
created: 2026-10-13
based_on: intent.md
design_approved_by: chenli (user; existing decisions, recorded as claims, not authenticated)
design_approved_at: 2026-10-13
design_approval_source: Original user messages in the parent thread mcp:0a3b2a07-7688-4836-bd49-c63b4bb70156, verified by the parent via t3_thread_read (private selection record parent-user-decisions-v1.json SHA256 3b44684f...c408, 2755 B). Position 172 "进入功能开发 / 你可以派出 subagent" (2026-10-13T06:34:54Z); 183 "private 任务应对谁可见？机器主人也要通过服务端可见性过滤。仅创建者可见（推荐）" (06:36:28Z); 340 "开始实现完整分享功能" (08:58:49Z); 348 answer "没有创建者信息的历史任务...保留本地、不自动分享（推荐）" (09:04:49Z; its default-private choice is superseded); 349 "默认 public" (09:04:54Z). Scope: product/privacy semantics only (default public, private creator-only incl. host, legacy local/unshared, full UI, public control). Not a review of the storage format or wire details, and not storage-migration, OS-isolation, rollout, paid, publication, push or release approval.
---

# Spec: Shared Agent Runtime

## Design

Surfaces: the ordinary React UI posts commands to `/api/web-ui/call` with a device bearer, pairs at
`/api/pair`, and opens `/ws?ticket=`. The T3 compatibility surface (`crates/server/src/t3_compat.rs`) is
separate; its only change here is privacy gating covered by `t3_share_privacy`.

Data: one persisted `ShareStore` (`crates/core/src/share.rs`, `<data_dir>/session-shares.json`, one
instance per data directory via `share::shared_store`) keyed by session: immutable `creator`,
`visibility` (`public` default | `private`), nonexclusive `followers`, bounded `journal`, durable
`message_authors` keyed by transcript seq. Every mutation persists a copy before swapping memory; a
failed write changes nothing and emits no event.

Policy (`codetwo_core::share_policy`, `ShareStore::can_view`):

- public: everyone; private: creator only (host local UI is actor `local-owner` and is filtered);
  no record (legacy): only `local-owner`, never a paired device, never listed or returned by `share.get`.
- Denied, unknown and legacy targets all answer `session not found`.
- Actor comes from the paired device (`device:<id>`), never from arguments.

Entry points covered by the same policy: `/api/web-ui/call`, raw `/ws` ops (prompt/cancel/new_session
with optional `visibility`/transcript), `/ws` welcome and session-bearing broadcast events, the T3
compat routes, native/NAPI `call_local*` and the desktop host event pump. Creation registers creator and
visibility before dispatch (`Engine` right after `Session::new`; a failed registration aborts the
create), so a private task cannot reopen as legacy.

Commands: `share.whoami`, `share.set_nickname`, `share.get`, `share.set_visibility` (creator only,
grants no control), `share.follow`; `engine.new_session` / `engine.new_parallel_task` accept `visibility`;
`remote.pairing_link` produces a one-time pairing link.

Authorship and request ids: for shared sessions a paired device's request id becomes
`device:<id>/<client id>`; same actor+id retries are one operation; a different actor using the same client id is an
independent operation in its own namespace (`ws_share_parity`: "same client id, independent actor namespaces"). User rows in `sessions.transcript` carry `author {actor, nickname,
request_id}`. `session_share_changed {session, kind, revision}` (kinds `message_author|follow|visibility|revoked`)
is metadata-free; `revoked` (public to private) reaches every participant so open views drop the task.

UI: header "New task visibility" selector, `ShareControls` (visibility, creator toggle, follow with
names, nickname, share link), rail badge with creator, per-message author labels, shared-turn
reconciliation (`turns.ts`, `sessionEvents.ts`, `App.tsx`): foreign running turns show one Working row,
queued rows clear on cancel, a Cancelled turn keeps its duration when the transcript is re-read.

Reported limitations (stated by the executor; no user or reviewer waived them): application visibility, not OS isolation; sharing-enabled cross-host handoff without creator provenance
stays local/unshared and remote activation is unsupported; native steer unverifiable with the fixture;
workspace/git/search-by-cwd commands are only audited where listed in the evidence.

## Acceptance criteria

- [x] AC-1: Defaults and privacy: new tasks public, private creator-only including host, legacy local-only (Core/server tests, `evidence/r26..r30` privacy audits).
- [x] AC-2: One policy for HTTP, WS, T3 compat, native/NAPI and host events; failed store writes fail closed (targeted tests).
- [x] AC-3: Server-derived durable authorship with actor-scoped request ids (`share_authorship`, `ws_share_parity`).
- [x] AC-4: Real paired-device UI: pairing link, public visibility, restart, revoke, continued A/C control (r14, r15, r20, r31, r34/r35 manifests; fixture ACP).
- [x] AC-5: (UI evidence R37; literal manual `share.whoami` NOT run, equivalent store correlation plus route/native tests) Current-bundle UI check on the R36 bundle with two isolated browser clients: creator-private (creator Working/Stop, other client's pane and rail cleared, other public task still listed) and 390 px dark header, actors identified from the fixture store (see `evidence/r37-current-ui.md`). Literal `share.whoami` was not executed.
- [x] AC-6: Independent Agent verification of the bounded local candidate (R38 fresh-context reviewer). This is not human review, merge, release or rollout approval.
