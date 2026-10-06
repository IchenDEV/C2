# Memory contract

Status: **current implementation contract**. Historical comparisons that informed the first
version are archived in [`archive/research/memory-influences-2026-08-06.md`](../archive/research/memory-influences-2026-08-06.md).

C2 has two kinds of continuity that solve different problems:

1. **Provider-native context** continues one ACP session inside Claude Code, Codex, Grok, or
   another provider. The provider owns that context.
2. **C2 project memory** is a local, provider-neutral recall layer. It can carry stable project
   knowledge and earlier outcomes across sessions and providers without pretending to be the
   provider's conversation state.

The second layer lives in `crates/core/src/memory.rs` and the same `codetwo.db` as sessions. It is
enabled by default and has independent switches for learning and prompt-time recall under
**Settings → Memory**.

## The four layers

| Layer | Content | Ownership | Automatic prompt budget |
| --- | --- | --- | --- |
| L0 | Existing raw transcript parts | Canonical app transcript | Up to 3 excerpts, only when the request explicitly asks about earlier work |
| L1 | Stable constraints, preferences, facts, decisions, and manual notes | Derived, with source references | Up to 12; pinned notes are considered first |
| L2 | A bounded request/outcome summary for a completed turn | Derived, with source references | Up to 3 from earlier sessions |
| L3 | A conservative profile assembled after at least 3 active L1 notes | Derived from L1 | 1 profile |

L0 is never copied into another table. L1–L3 rows retain their source session and part sequence,
confidence, timestamps, pin state, and active state. Unpinned L2 history is capped at 300 episodes
per project.

## Capture and recall

Capture runs only after a provider completes a turn successfully. L2 is written immediately as a
source-linked work episode. High-signal constraints, preferences, and decisions first enter a
candidate queue; after a 30-minute settling window, a background maintenance transaction promotes
them into L1 and rebuilds L3. Due candidates are also processed on startup, so closing the app does
not strand them. Each later turn in the same session resets that window, so an active conversation
is not consolidated mid-stream. This borrows Codex's useful separation between foreground work and
slower memory consolidation without adding a hidden model call.

Capture examines the user's original document text, not expanded file contents, project rules,
skill bodies, or an earlier memory block. Deterministic English and Chinese markers keep candidate
generation testable; users can add any important L1 note manually.

Near-duplicate L1 notes (token Jaccard score at least `0.8`) reinforce one row and merge evidence
instead of creating copies. Common private-key, credential-label, OpenAI-key, and GitHub-token
shapes are removed before derived text is stored. Redaction is defense in depth, not a promise that
arbitrary secrets can always be recognized.

Search tokenizes ASCII words and CJK bigrams, takes at most 12 query terms, ranks within each layer,
then combines reciprocal rank and lexical coverage:

```text
layer_weight × (rank_score × 0.6 + lexical_score × 0.4) × confidence
```

L1, L2, L0, and L3 use weights `1.00`, `0.92`, `0.85`, and `0.75`. A small pin boost is applied
after fusion. Automatic recall excludes current-session L0/L2 records and is kept separate from the
persisted transcript, preventing direct self-recapture as user-authored input.

Long-term candidates are matched in the full project scope before applying the candidate cap.
SQL uses bound terms and separate L1/L2/L3 budgets (400/150/50); unrelated new records no longer
hide old relevant notes. Ranking remains lexical and can miss paraphrases without word overlap.
No embeddings or remote memory provider is added.

Every turn retains provenance flags for MCP, files, images, referenced chats, browser context,
provider tool calls, and recalled memory. **Learn from external context** can exclude any such turn
from L1/L2/L3 generation while leaving its canonical transcript intact.

The block sent to the provider is explicitly labeled **untrusted recalled context**. The current
request and repository rules win, and instructions found inside recalled transcripts or episodes
must not be executed merely because memory surfaced them.

## Session policy and receipts

The composer has four session presets: **Memory on**, **Recall only**, **Private session**, and
**Learn only**. They persist independent read and write policies (`inherit`, `allow`, or `deny`) on
the session. The global master, capture, and recall switches still win; a session may narrow global
behavior but cannot silently re-enable a disabled feature.

Codex is the one provider-specific transport boundary: its **Codex default** preset uses the
inherited session policy but does not append C2 memory to Codex's ACP user message, because Codex
exposes every `session/prompt` text block as user-authored conversation. It still learns according
to the global settings. Choosing **Memory on** or **Recall only** sets an explicit read `allow`,
sends the bounded recalled block, and retains the normal receipt. Other providers continue to use
**Memory on** as their default and resolve `inherit` from the global recall switch.
Codex Quick Chat and Side Chat explicitly use recall-only policy so their existing project-memory
context remains available without learning durable memory from those app-lifetime conversations.

When recall is used, C2 emits and stores a turn receipt containing the exact memory ids, layers,
categories, evidence pointer, retrieval score, text sent, and estimated prompt tokens. The Turn UI
shows it under **Memory used**. Receipts are metadata beside the transcript; the injected memory
block is never persisted as a user-authored message.

## Known boundaries

- Global settings and per-session policies are both enforced; stored and recalled items remain
  isolated by exact project path.
- Derived capture is intentionally conservative and can miss an implicit preference or decision.
- Search is lexical, not embedding-based; it works offline but will miss some semantic matches.
- The desktop app exposes inspection controls today. The core data remains available to other
  frontends, but the remote UI does not yet have a memory manager.
- Disabling recall leaves stored rows untouched. **Forget** deactivates an editable derived row;
  **Undo** reactivates it. Raw transcripts and the L3 profile are inspection-only.

## Chief of staff workspace

Chief recall uses a bounded core pocket plus query recall. Alongside pinned L1 notes, each scope
carries up to four active, conflict-free, manually confirmed or user-corrected preference/constraint
notes even for a greeting with no shared keywords. All L1 input remains capped at twelve per scope;
settings and session read denial take precedence. Global and selected-project scopes stay separate.
Other categories remain query-driven unless pinned. Corrections and forgetting change the next
recall, including this pocket. Unconfirmed proposals and automatic candidates never enter the core;
the existing query/profile path still contains derived notes labeled as untrusted context.

This borrows compact durable context from [OpenClaw memory](https://docs.openclaw.ai/concepts/memory)
and bounded always-present context from [Letta memory blocks](https://docs.letta.com/v1-sdk/memory/memory-blocks).
C2 retains its Store, receipts and confirmation boundary. It does not install those systems or add
Markdown writers, paid consolidation, or a second facts database.

The global **Chief of staff / 幕僚** entry is one persistent conversation with a personal project agent.
Discussion and progress questions do not create work. Explicit assignments can route to several
authorized projects; goals, execution, delivery and acceptance retain their original records.
The default view shows the conversation and relevant decisions, with project scope, other work,
execution and memory available on demand. The notebook
uses this same Memory Store: shared notes use the reserved scope `codetwo://chief-of-staff`, and
project notes retain their registered project path. It exposes active L1 notes, source sessions,
manual corrections and forgetting. Existing capture/recall policies still apply. Forgetting excludes
a note from subsequent reviews; it cannot retract text already sent to a provider.

Confirmed suggestions write their exact trimmed text through the same manual-memory operation.
Manual notes reinforce only identical text; automatic capture retains similarity deduplication.
The note and derived profile commit in one transaction. Confirmation alone means waiting to save;
the conversation shows saved only after the durable memory id is recorded. An unknown write outcome
requires inspection and is never automatically repeated.

Coordination is off until the user selects projects and enables follow-up. The Core persists goals,
priorities, acceptance criteria, next steps, blockers and assignment receipts separately from the
canonical Engine execution state. It recalls the selected scopes with source receipts at the start
of each fresh review. Review sessions deny memory capture so their combined context does not become
new project knowledge. Editing a note or policy while a review is running invalidates that decision.

The coordinator returns a bounded structured proposal. Core validates its scope, revision and
allowance before creating ordinary Engine sessions. Git projects use isolated worktrees; assignments
refer to the actual execution workspace. Linked existing sessions grant observation only. Users can
pause, resume, open or take over an execution. Disabling follow-up preserves records and stops new
assignments; already submitted workers retain their existing permissions and remain visible.

An idle session is not an accepted goal. Acceptance must reference the latest assigned idle session,
its activity revision, evidence and 1–16 actual workspace files with matching SHA-256 hashes. The
verifier rejects escaped paths, special files and excessive file sizes. Later session or file changes
invalidate the verdict. Model/effort rejection stops managed prompts instead of silently selecting a
fallback. Missing prompt receipts surface an attention item; recovery does not blindly resend them.

Follow-up requires the owning Core to be online. Startup and low-frequency reconciliation use durable
state, without calling a model when no relevant facts changed. Review, assignment, retry and parallel
limits bound progression; they are not a monetary budget. Provider permissions remain the execution
boundary; the coordinator instruction alone does not provide an OS sandbox. The first release does
not provide a cloud daemon, automatic publishing, or a separate memory database.

Workers use a host-bound `codetwo_coordination` MCP server with six tools: `context`, `progress`,
`ask`, `propose_change`, `confirm`, and `submit`. Core supplies their session identity and validates
the current assignment, requirement version and selected project scope. Context includes bounded
project/shared recall and explicit accepted dependency artifacts. Mutations require stable command
ids; repeating an id with different content is rejected. The coordinator cannot grant worker tools
access to an unrelated project or answer a concrete permission request by inference.

Messages, questions, changes, reviews and notifications belong to persistent work records. They do
not become memory automatically. A user can choose to remember an applied change as a project
decision; Core writes it only after the worker confirms the current requirement. The memory content
retains the change id, acceptance and reason. It remains editable/forgettable through this notebook.
Correction/forgetting lists potentially affected active assignments and offers a separate correction
message. It does not claim that a provider has forgotten earlier content.

The conversation projects blocking questions, proposals and submitted results from their original
records. It does not infer permission approval from ordinary discussion. Explicit task stop, priority
changes and requirement changes reuse the existing control and version paths; priority orders ready
work without preempting a running worker. Engine owns one durable
queue/steering path for ordinary inputs and coordinator messages. Delivery receipt, worker
confirmation, submission, acceptance and notification read state remain distinct. Changing a goal
invalidates old acceptance, stops affected execution and requires confirmation of the new version.
An upstream file/session/version change blocks downstream acceptance; an explicit resume authorizes
the new accepted input snapshot until the same worker confirms it. Unknown delivery stays visible
and is never blindly retried. Desktop reminders have no guaranteed display/read acknowledgement;
the durable coordination records remain authoritative.

This bounded first version supports 200 goals, 20 attempts/dependencies per goal, 1000 messages,
500 questions/changes and 200 natural-language requests per workspace. Hitting a history limit
returns an explicit error rather than deleting work records. Read notifications may be pruned after
500 entries; unresolved notifications remain. There is no deadline scheduler or automatic history
archive in this version. Follow-up budgets count planning rounds and assignments, not money.

Persistent messages use stable client turn ids: identical retries are idempotent; a different payload
with the same id is rejected. Recorded, analysing, handled, failed and unknown describe message
intake, separately from provider delivery or worker adoption. Independent bounded intake runs do not
consume the goal-review allowance. Actions compare the affected goal versions and user control
generations; unrelated goal changes do not discard a valid decision. Unknown creation or prompt
outcomes remain visible and are not automatically sent again.

A proposed memory shows its exact content, category and project scope before confirmation. Confirmation
is bound to a content hash and original message; the same Memory Store owns the eventual note.
Unconfirmed suggestions are not factual recall. Memory write attempts are durable: an unknown write
is inspected rather than replayed. Forgetting a confirmed note excludes it from future recall and
does not recreate it from conversation history. The conversation is bounded to 400 entries, with at
most 20 outstanding user messages and 200 memory proposals; a full history rejects new input and
retains existing records. It has no automatic archive or hidden summarisation call.
