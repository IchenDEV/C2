# R24 evidence manifest

One root-cause repair: another actor's running turn rendered twice on a distinct client.

- Root cause (captured sequence, see regression): on C, A's `turn_started` arrives with `request_id` scoped to null (`scopeEventToSelf`), so `applyEvent` adds a `(started elsewhere)` placeholder keyed only by `transcript_seq`. The follow-up transcript read returns A's canonical running prompt at the same seq. `mergeLoadedTurns` matched the placeholder to the tail by seq, but then treated the tail as having no request id and returned `[...loaded, ...live]`, keeping the placeholder beside the canonical row.
- Repair: `apps/desktop/src/session/turns.ts` `mergeLoadedTurns` folds the seq-matched tail placeholder into the canonical tail (single source), and keeps the loaded request id when the live row has none. `turns.ts` sha256 20ba4622..., test `apps/desktop/tests/sharedTurnReconcile.test.ts` sha256 78c6175a... (3 new tests failed before the change, 10/10 pass after).
- Bundle: `vite build --mode web` exit 0, entry `percentages-BXMCSKIN-CbpeUEYr.js` sha256 1dd178860029e0d0c19c0a4c8c5887a70ec454c96d4541574c3ecc3bfe6c1fb0. Profile/origins: own default-profile tabs, A `127.0.0.1:52943`, C `localhost:52943`, 1280x800.
- Live (same existing public task by Guest-9d9a, A Run, C observes): C shows exactly one canonical running row (author Guest-b8a2, `Working…`) and 0 `started elsewhere` rows. C queued a message (QUEUED #1). C Stop: A and C show Cancelled with 0 Working rows.

![C single running row](r24-C-foreign-running-single-row.png)
