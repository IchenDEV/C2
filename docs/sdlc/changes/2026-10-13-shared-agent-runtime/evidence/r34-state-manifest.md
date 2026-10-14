# R34 state manifest (actual GUI, partial)

Native start 2026-10-13T16:54:43Z, cutoff 17:04:43Z. Brief SHA256 f95c43937e10d75254a915c24e6a6f23afbf458a786c3884faa9c0fbf96d16a3 (3657 B). Worktree HEAD 926ebdb5; main f94b88b4 clean (0 porcelain lines).

## Provenance (pre-repair, what was rendered)
- Binary full-sharing/cargo-target/debug/examples/share_fixture 109538416 B sha256 5f85af651f5ae88691d00b3be7fe345361907770ff7f85a2dea074398c85be1f (mtime 00:49, newer than engine.rs/event.rs/turns.ts 00:46).
- Bundle r33/dist-web/index.html sha256 b28d01b537839bab2ffdfaaac96a95dd19a8cf37828e71f3473528b0628f17ab (00:48); linked JS percentages-BXMCSKIN-B9ikOeKi.js 5355371 B sha256 69dc24b11539a4242226e914d5606afc57ec448a6b86ebd83a3e715a89e85a9a; CSS percentages-BXMCSKIN-CMwT83sK.css 514802 B sha256 4fa6007116320bba66b834193fed7b1ef509f1bcef26dc5dcec96ddd824216f6.
- Source at render: engine.rs f2aef37f0307489a6c91373501aeb625bd244d5e0ec22f5cb45d14b2062aef8f, turns.ts bfe4fb31a5ffb47fc3d4ba855861a4e80223ddf9dc3d3aed989a595e901e863d, App.tsx 8b829e41b896803f34868f5c0b47e035ebd7dde8520689bde9b5617a9ef67453, share_fixture.rs 8c9b81427c94bedfaa532fe5d7201bb331e3335ec171c98cd549e55df9646160.
- Fixture: port 52943, r19 fx-data + proj, wrapper r34/run.py, pid/pgid 48043 (started 16:55:24Z, READY ~16:56:13Z).

## Browser provenance
Two tabs, both opened with reuseExistingTab:false, default profile, 1280x800 viewport (PNG 1280x800):
- A (creator): tab_s_0f19ca3e..., origin http://127.0.0.1:52943, UI author label Guest-c8f6.
- C (other participant): tab_t_0f19ca3e..., origin http://localhost:52943, nickname Guest-9af3.
Every later call passed its own tabId. Console entries: none reported in any snapshot. Pair tokens not recorded. share.whoami via /api/web-ui/call NOT run (Bearer is held in page memory); distinct actors shown by UI author labels and by scoped delivery ids in the store (three distinct device ids).

## PNGs (copied from T3 browser-artifacts, unmodified)
| file | bytes | sha256 | what |
|---|---|---|---|
| r34-queued-C.png | 80468 | feb144c823349fc52f6d1c4e3f777951660239c03921a6fa3898102448d30837 | C: ONE Working + Queued #1 |
| r34-stopped-C-queued-row-stuck.png | 80671 | 6f9fd2a08756c9b7ae21fb37cbc84e1efc2c693456db16ed02de1a51832caee3 | C after Stop: Cancelled 37s, Queued #1 STILL shown (FAIL) |
| r34-continued-A-lost-cancel.png | 80363 | dcd3d5a562bf831b24d4396e88c4193031ca0ffa40783e09df34d3e0e0f4c89b | A after C continued: author Guest-9af3 correct, old turn shows 0s without Cancelled (FAIL) |
| r34-creator-private-A-working.png | 81119 | b13b213da2868389b94aede18b15a233ba1d87cafbbca66d4b47be2cedfc0f89 | A after Make private: row Working, Stop present (PASS) |
| r34-revoked-C.png | 58845 | 062d5e749f811e6b0f57fda2d539cb60986b5b7e7625eba0511887adeeb1255f | C: task gone from pane and rail, "no longer" notice (PASS) |
No 390 px screenshot (out of time).

## Store corroboration (read-only sqlite, mode=ro)
prompt_deliveries newest rows: state=cancelled outcome='Session control changed' with ids `device:<device-id>/<clientId>` (sequence 3, 2, 1). So Core did cancel C's queued row and ids are actor-scoped; the UI row holds the unscoped client id.

## Failures (actual, rendered)
1. Queue: Stop cancelled the row in the store but C kept Queued #1. A (creator) never had that row.
2. Continue: after C sent a new prompt without Core restart, the historical turn lost its Cancelled label and 37s duration (A and C both show 0s).

## Candidate repair for 1 (NOT re-rendered)
Root cause: scopeEventToSelf (session/share.ts) rewrote only request_id/prompt_request_id; prompts_cancelled.request_ids (scoped array) passed through untouched, so turns.ts could never match the client row id. Change: map each element via ownClientRequestId, keep only own. share.ts sha256 ed856ab80b833d5f1b856bdc42dfe0e268dc0b60c6de46ae5ffb0d744660ca98; tests/share.test.ts sha256 171cb291e0be5ca7b50acf23d950340c2c1666a3e8b58857489a6e6d7c34dcfa. Focused `bun test tests/share.test.ts tests/sessionState.test.ts`: 52 pass 0 fail; `bunx tsc --noEmit` no errors (exit 0). Bundle NOT rebuilt, so the GUI still runs the old code.
Failure 2 not diagnosed.

## Cleanup
Removed: fixture pgid 48043 (SIGTERM to exact verified group; port 52943 no listener; wrapper 47982 gone). Retained: r34 logs/run.py/result.md, r19 fx-data (fixture data, shared), five PNGs in evidence. Misplaced R33 result moved: bytes sha256 1b93d5fb258966869f2d885d11688ecbc2677f9fa1bd06bedd0cd7677e484ff9 at r33/result.md and evidence/r33-result.md; root copy removed. Browser tabs left open (own, harmless). Processes: none owned remain.

## Failure 2 lead (unverified hypothesis, no code change)
mergeLoadedTurns (turns.ts ~L862-885) restores Cancelled status/timing only when a live terminal turn carries transcriptStartSeq and live.length>0. If the rows were rebuilt from a transcript refresh (message_author -> refresh-transcript after the continue) while the live Cancelled turn had no transcriptStartSeq, or live was empty, the loaded 0s row wins. Next step: log transcriptStartSeq of the live turn at refresh time in a render.
