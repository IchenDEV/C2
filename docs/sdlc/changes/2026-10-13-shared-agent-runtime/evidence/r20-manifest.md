# R20 share/private/restart runtime evidence (fixture, zero-fee, no tokens)
Captured 2026-10-13 ~21:49+08 via preview_snapshot({includeImage:true,save:true}); PNGs copied byte-exact from browser-artifacts. Both 1280x800 CSS viewport, PNG image 1280x800 (DPR 1 as reported by tool), light theme.
- r20-receiver-A-after-restart.png sha256[:16]=f41b25a4192e6f07 (default profile, actor Guest-9d9a, id device:9f6526ee-e7b6-4c25-9c71-87703e559d9a): rail shows BOTH "R20 PRIVATE task" (status Failed: its run was interrupted by the intentional fixture restart) and public "R20 share check: reply briefly".
- r20-receiver-C-after-restart.png sha256[:16]=ef7514dbb8e52306 (incognito profile, actor Guest-7b2b, id device:343c58d1-0b16-4f55-84c2-1efcf3d67b2b): rail shows ONLY the public task; private task absent.
- Actor IDs from ordinary Core command share.whoami via POST /api/web-ui/call (existing transport): distinct.
- Console: ERR_CONNECTION_REFUSED / Failed to fetch entries at 13:48:58-13:49:12Z = fixture intentionally stopped before relaunch (READY ~13:49); no other errors.
- Bundle main JS percentages-BXMCSKIN-Cya-cT38.js sha256[:16]=17b0ad10703cfb4c (R19 dist-web, bridge.ts unchanged); share_fixture.rs=8c9b81427c94bedf; remote.rs=e9555d5579286048.
- Fixture: 127.0.0.1:52943, own PGIDs 90766 / 2629 / 8534 all TERMed.
