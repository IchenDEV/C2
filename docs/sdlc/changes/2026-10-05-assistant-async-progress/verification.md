---
id: 2026-10-05-assistant-async-progress
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-05
based_on: plan.md
revision: "Uncommitted worktree on 14c20fce841384dbeaa16c48c7a7264d6e4a9f81; implementation hashes in evidence/source-sha256.json"
verification_mode: fresh-context
verified_by: "codeTwo independent supervisor (run14; separate execution context)"
verified_at: 2026-10-05
release_target: none
cleanup_status: complete
next_trigger: No pending action in this local scope; any release or external integration requires separate authorization.
---

# Verification: Assistant Async Progress

## Verification

- AC-1: PASS — [Core regressions](evidence/core-tests.txt): real Python ACP processes stall initialize, session/new, model execution and steering; B receives durable dispatch/prompt receipts before A is released. The production edit/worker gate remains obtainable within 250 ms. Shared Engine lanes also pass [delivery integration tests](evidence/core-tests.txt).
- AC-2: PASS — The same barriers stop A before releasing its slow Provider. Recovery with and without reasoning effort, stop before revival registration, and queued prompt takeover/requirement changes after Engine acceptance reject late execution. Uncertain stop recovery blocks resume; only persisted terminal activity releases it. Dispatch rejects an assignment with an outstanding stop. [Core regressions](evidence/core-tests.txt).
- AC-3: PASS — Real A/B review processes independently commit decisions. B changes and completes another review while A retains its original valid run. Same-scope takeover and dependency revision changes alter A's fence. Oversized A context fails locally while B completes. [Core regressions](evidence/core-tests.txt).
- AC-4: PASS — Recovery tests cover creation claim without receipt, original creation receipt reuse, native answer claims, delivery claim/receipt windows, legacy global-run retirement, and stop before send/after send/after terminal receipt. Unknown operations remain recorded and are not replayed. Existing transactional rollback, identity and adoption tests pass. [Core regressions](evidence/core-tests.txt).
- AC-5: PASS — [Desktop tests](evidence/desktop-tests.txt): 58 passed. [Renderer](evidence/renderer-build.txt), [Web build](evidence/web-build.txt) and [server build](evidence/server-build.txt) passed. The actual React Web build paired to one isolated Core was operated through T3 preview: scoped reviews/errors, unknown-answer lockout, message recording, notification read and stop notification. SQLite confirmed the operations and B remained active when A stopped; [UI results](evidence/ui-result.json). Real ACP supported steering and unsupported queue fallback both distinguish acceptance from adoption.
- AC-6: PASS — [Run14 independent acceptance](evidence/run14-independent-acceptance.json) verifies the current source manifest from a separate execution context and isolated source copy: Core 577 (576 existing tests plus the original extra probe), coordination 13, delivery 6 passed. The supervisor rechecked 767 source/fixture files after testing and before cleanup without changes; the recording owner also matched all 767 against the current checkout. [Independent test excerpts](evidence/run14-independent-tests.txt). Prior owner tests remain owner evidence, not independent results.

- AC-7: PASS — The [same supplied ACP probe](evidence/terminal13-original-probe.rs), retained as a production regression with extra no-spin assertions, now advances C from assignments=0/turns=1 to assignments=1/turns=2 after resource release in 0.20 s; [current probe output](evidence/terminal13-resource-release.txt). Typed local rejection, full-capacity no-spin, unknown-result no-replay and one-free-slot/two-waiter admission checks pass.

Verdict: verified.

Run14 independent acceptance resolves the terminal13 resource-release blocker for the authorized local asynchronous coordination scope. Earlier evidence remains attributed to its original executor and scope. This verdict does not authorize or claim publication.
Residual risk: ACP tests use real local protocol processes with deterministic barriers, not live paid model accounts. UI uses the actual app and Core with offline persisted fixture data, not a full external Provider business task. No CI, remote deployment, native packaged desktop notification delivery, email or Feishu/cloud integration was exercised or claimed.

## Evidence and reproduction

The [source manifest](evidence/source-sha256.json) identifies the uncommitted implementation; earlier chief-of-staff edits remain in this worktree. Previous coordination Verification is explicitly historical for this follow-up.

Commands from this checkout:

```sh
cargo test -p codetwo-core --lib --test assistant --test prompt_delivery --target-dir .codex/run/assistant-async/target
cargo check --workspace --all-targets --target-dir .codex/run/assistant-async/target
cargo build -p codetwo-server --target-dir .codex/run/assistant-async/target
```

Core: 574 library tests, 12 assistant integration tests, 6 delivery integration tests. [Workspace check](evidence/workspace-check.txt) passed. Desktop commands run from `apps/desktop`:

```sh
bun test tests/assistant.test.tsx tests/assistantNotification.test.ts tests/missionControlRendered.test.tsx tests/missionControl.test.ts tests/sessionRailRendered.test.tsx
bun run build:renderer
bunx vite build --mode web --outDir ../../.codex/run/assistant-async/web-dist
```

Actual UI used `codetwo-server webui --no-open`, bound only to `127.0.0.1:14673`, with task-owned data and Web assets. Boot the empty disposable database once, stop its owner, run [the offline fixture seed](evidence/seed-ui.py), then restart the same owner for reproduction. Automatic follow-up is disabled in this UI fixture; no model task is launched. T3 `preview_open` succeeded with omitted initial URL; earlier explicit `about:blank` failures were not product failures. An initial desktop-mode asset build was replaced by the proper Web-mode build before UI acceptance. Build warnings concern existing bundle sizes.

Actual rendering inspected at light 1280×800 and light/dark 640×900 CSS pixels. A navigation-highlight question was resolved by checking `aria-current` and computed styles after the 0.12-second CSS transition; Inbox screenshots were recaptured after settling. The narrow chief-of-staff section measured 640 px width and scroll width. Screenshots:

- [Independent review states, light](evidence/projects-light.png)
- [Unknown clarification cannot be replayed, light](evidence/inbox-light.png)
- [Recorded, accepted, confirmed and unknown messages](evidence/delivery-states-light.png)
- [Independent review states, dark narrow](evidence/projects-dark-narrow.png)
- [Persisted stop notification, dark narrow](evidence/stopped-dark-narrow.png)

The isolated UI action timeline was: mark B's notification read, record a supplement while follow-up is paused, inspect its Recorded state, stop A, then observe the supplement cancelled and a stopped notification. B remained active. A pre-response SQLite read still showed the old state; the final [UI result](evidence/ui-result.json) and actual notification prove the subsequent durable outcome.

## Cleanup

Removed: task-owned `.codex/run/assistant-async/` (3.4 GB), regenerated `apps/desktop/dist` (48 MB), disposable Core data/credentials and original temporary screenshot copies. Compact evidence was consolidated before removal.
Retained: source edits, lifecycle records and compact evidence; prior user/other-work state is preserved. Terminal13 temporary resources are removed as recorded below.
Retention owner: codex for delivered repository evidence; original owners for pre-existing resources.
Cleanup trigger: repository evidence follows normal project retention; no task runtime should remain at handoff.
Processes: only task-owned Web Core processes were stopped; T3 tab 3 was closed through its normal lifecycle. Port 14673 is free. No task runtime remains.
Evidence: [cleanup inventory and exact removal report](evidence/cleanup.json); final checks did not recreate build output.

## Review and release

Approval: Original human design/implementation decisions were independently recovered from the original thread; see Intent and Spec. No new approval inferred from the reviewer's identity or model proposals. Read-only independent review found and drove corrections to revival cancellation and final delivery authorization; see the [independent review record](evidence/independent-review.txt).
Rollback: See plan.md; retain durable unknown attempts and receipts before reverting scheduling.
Release: Local work only. No push, PR, merge, deployment, external messages, paid model run or cloud daemon.
Feedback: Supervision probes were converted to production regressions. [Lifecycle Eval](evidence/lifecycle-tests.txt) passed 32 tests.

## Resource-release correction terminal13

User feedback `codetwo-r1-resource-release-20261005-terminal13` authorizes the bounded correction. The supplied real ACP probe failed against the exact Runtime hash e0a61389ec8b51c880bfb8e78907eaf937f7687152fcaa057545f7a73a94bcc3. The local correction and verification are complete; prior independent review is not acceptance of this correction. See [the before failure](evidence/terminal13-before-failure.txt) and [prior source manifest](evidence/source-sha256-before-terminal13.json).


The fix extracts the existing worker-capacity predicate for both admission and wake checks. A typed local commit rejection records `retry_on_capacity` on the same review document. A newly available slot allows a fresh, bounded review with a fresh run id; it never resends an old Provider attempt. Dispatch rechecks capacity against latest state. A rejected overlarge action batch cannot cause retries merely because its own simulated intents exhausted capacity. Old records without a marker are eligible only after checking the original prompt receipt, normally completed session, valid scoped dispatch output and exact local capacity rejection. Explicit non-retryable records stay non-retryable. Error text alone cannot authorize retry of an unknown operation. The positive probe also removes the marker before resource release to exercise existing persisted failures; the unknown-result regression verifies the contrasting fail-closed path.

Current local evidence:

- [Core regression run](evidence/terminal13-core-tests.txt): 576 library tests, 13 assistant tests (including the original 12), and all 6 delivery tests passed.
- [Latest assistant integration run](evidence/terminal13-assistant-tests.txt): 13 passed, including two independent waiters competing for one free slot.
- [Focused Runtime run](evidence/terminal13-runtime-tests.txt): 23 passed, including all original 21 and the resource-release/unknown-result regressions.
- [Workspace/all-targets check](evidence/terminal13-workspace-check.txt) and [Rust format](evidence/terminal13-format.txt) passed.
- UI source and transport behavior are unchanged in this correction. Prior actual T3 Web/Core rendering evidence is reused; no new browser or user app process was launched.

At the terminal13 handoff, local checks passed but independent reacceptance remained pending. Run14 now supplies that independent verification, recorded below; the original explicit design authorization remains applicable. No owner test is relabelled as independent.

Terminal13 cleanup: dedicated `.codex/run/assistant-capacity/` build/output (2.3 GB) is disposed after final source-hash and process inspection. No app or browser instance was launched in this correction. See [cleanup report](evidence/terminal13-cleanup.json) and [final documentation/scope checks](evidence/terminal13-final-checks.txt).


## Independent acceptance run14 and record-only closeout

Authorization: the current user's `codetwo-independent-acceptance-run14-20261005-record-only` instruction authorizes only this Verification update and necessary compact evidence/check logs. Codex records the result; the independent supervisor executed acceptance outside the implementation owner's context. No code, design, model, permissions or release scope changed.

Source and environment: [the imported report with provenance](evidence/run14-independent-acceptance.json) records the original `/Users/chenli/Documents/Codex/2026-10-05/task-7/` filenames, SHA-256 hashes, timestamps, full verdict/tests/cleanup reports and snapshot metadata. All 767 source/fixture hashes match this checkout. Runtime SHA-256 is `56451c60e7e5affcd4ee629b8ce69cda249256f16e43bace506102b7a9d562ee`; assistant domain is `1a7ce98bfa1612d991e0875a5553a67a7cabe79d501afb956f643fc60062dc49`. The existing source manifest is preserved as the original tested artifact; its historical pending note is superseded by this Verification.

The independent supervisor used `/tmp/codetwo-r1-final-20261005-jsc1rb2v`, a source-isolated copy. Only the copy enabled pkg-config for the existing pinned Ghostty library and appended the original resource-release probe (renamed to avoid a duplicate test name). The user worktree was not modified. The first full Core run had one failure because the isolation manifest omitted the two existing `packs/hello-runtime` fixtures. Copying those exact same-source files corrected the test environment; the confirmed Core run passed all 577 tests. The initial failure and corrected result are both retained in the independent report and [test excerpts](evidence/run14-independent-tests.txt); no product fix was needed for that failure.

Independent result: Core 577, coordination 13 and delivery 6 passed. The original ACP resource-release probe passed in 0.20 s: C advanced from assignments=0/turns=1 to assignments=1/turns=2 without Wake. Full regression also covers old-record recovery, no spin while full, no replay of unknown results, and two waiters competing for one free slot. This closes AC-6 and independently confirms AC-7. Earlier terminal13 commands remain local owner evidence.

Limits retained: UI code did not change, so the earlier actual React Web/Core light, dark and narrow-window rendering is reused. Real paid-model business execution, native packaged desktop notifications, remote CI, email/Feishu and other external integrations remain unverified. Core must remain online; cloud or cross-device continuous operation is outside the approved scope. There is no PR, publication or deployment.

Cleanup: the independent report confirms removal of 1,749,960,847 bytes of temporary build data and no remaining owned processes. The original capacity/async scratch directories are absent and historical UI port 14673 is free. This record-only closeout launches no runtime or model and creates no temporary build. Retained: this Verification, compact provenance/test evidence and check log under normal repository retention; original external evidence remains with the independent supervisor/source task owner. [Record-only scope, cleanup inspection and final checks](evidence/run14-record-checks.txt) record the current inspection and required documentation/lifecycle checks.
