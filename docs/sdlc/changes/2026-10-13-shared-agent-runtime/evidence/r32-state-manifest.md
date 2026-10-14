# R32 state manifest (PARTIAL; no two-client render performed)

Scope: real source diagnosis + unit/tsc/build only. No fixture started, no PNG, no whoami run (budget).
Window: first start 2026-10-13T16:30:08Z, cutoff 16:40:08Z.

## Diagnosis (actual truth)
1. Queue: r19/fx-data codetwo.db `prompt_deliveries` holds the two prior queue rows with state=cancelled, outcome='Session control changed'. Core cancelled them (`cancel_pending_prompts` -> `cancel_session_deliveries`) but emits NO event (only `PromptQueued` exists), so the client row can only clear on reload. Cause is a missing Core event, not wrong cancellation. NOT FIXED: a Rust event is needed and a naive non-terminal Error would reach other clients as an idle "(session notice)" row (request ids are scoped to null for non-authors). Rust edit was made then fully reverted; no Rust change remains.
2. Cancelled58s -> 0s: `mergeLoadedTurns` only carried live stopReason/timing for the tail; a canonical non-tail turn rebuilt from the transcript has no stop reason and endedAt = last part time. FIXED in turns.ts: carry stopReason/error/endedAt/min(startedAt) from a live terminal turn with the same transcriptStartSeq; unknown seq untouched.
3. Active row 0s after private toggle: `applyShareChanged` "drop" (revoked) called updateRunningSession(false) and cleared latestTurnRequestIds even for the still-allowed creator, so the re-read tail was built as not running. FIXED in App.tsx: that clearing moved into the denied branch only. Verified by code reading + a merge-level test of the running tail; NOT rendered.

## Hashes (first 16 hex)
turns.ts 81d111125771a640; App.tsx 8b829e41b896803f; sharedTurnReconcile.test.ts 5f9ba5e6a8becb87; dist-web/index.html 4b995e2cbeb6a80e (r32/dist-web fresh vite --mode web).

## Exits
bun test share/sharedTurnReconcile/visualizationOrigin: 37 pass exit 0; tsc 0; vite web build 0; sdlc --worktree 0 (contract valid); git diff --check 0. docs.ts not run (no docs link changes besides this file). Old-red run not performed.
