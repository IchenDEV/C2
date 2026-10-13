> **CURRENT VERDICT (corrected 2026-10-13): PARTIAL / blocked.** No preview PNG and no input manifest were saved, and later `html_preview` retries were rejected by the host, so the 728/390 result below cannot be reproduced. Everything below is HISTORICAL and applies only to the in-memory mock prototype, not to the product, a real Agent, two real clients or server-side filtering. The HTML itself is unchanged (11505 bytes, SHA-256 `12f3fc86c9c84f24bfbb626461108f4d8ac8f101f7cef628b5e688611996a8dc`).

# Preview acceptance (final round, 2026-10-07 UTC, historical executor record)

Subject: `prototype.html`, 11505 bytes, SHA-256 `12f3fc86c9c84f24bfbb626461108f4d8ac8f101f7cef628b5e688611996a8dc` (final; not changed after preview).
Tool: native T3 `html_preview` (headless browser), one call at 728 and one at 390, both on this exact version.
Input: the HTML was transcribed by hand into the tool call (the tool cannot read a disk file). The executor did not machine-compare the input. A reported independent check at 18:47:38 UTC: parsed the 728 and 390 HTML from this round's original activity input objects, compared with Python `read_bytes`/strict UTF-8; both equal the final 11505-byte file, and all three SHA-256 are `12f3fc86c9c84f24bfbb626461108f4d8ac8f101f7cef628b5e688611996a8dc`. The executor viewed the PNGs and the checker read the configurations (reported, not reproducible).

| Width | contentHeight | Console |
|---|---|---|
| 728 | 583 | one `log`: `PASS: simulation default public, one-way publish, private filtering, ... shared state`; no warning or error |
| 390 | 1005 | same single `log`; no warning or error |

Viewed PNG (728 and 390): owner avatars differ by color, shape and first character (circle blue 我, square orange 青); device icon is laptop (Mac) or server (VM); share icon = public, lock = private; each row shows a dashed "模拟" tag. No horizontal overflow or clipping at either width. 390 stacks the two panels. The PNG is only returned to the model; no PNG file is stored.
Interaction (page self-check, not a real client): summary click expands then collapses the details; the check passed with no error. Other covered behaviors: default public, public cannot return to private, private->public keeps owner, shared stop/continue/attribution, private fixture not auto-public.
Reference image (an existing device card with small avatar, pin, status and device icons) was viewed; the existing icon style is kept.

Superseded history: the old r5 pass (7262 bytes, `b19bc48a...`) and the old r7 728 preview (which reported `Shared state failed`) are not evidence for this version.

Limits: simulation only. Undecided: whether the machine owner can see private tasks, and who may publish a legacy private task; this is a one-way public step, not a two-way switch. Queue/steer/worktree stay unverified. No real server, Agent, two clients or cross-device.
