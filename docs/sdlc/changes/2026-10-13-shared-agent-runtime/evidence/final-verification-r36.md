# Final verification index R36 (current records, partial: one UI check remains)

Window 2026-10-13T17:14:34Z-17:24:34Z. Worktree `research/shared-agent-20261008` HEAD `926ebdb5e1a8ec644cf1f8d4efc02d28a8e9c6ef`, dirty by design (other workers' and visualization hunks retained). Main `/Users/chenli/projects/codeTwo` branch `refactor/architecture-simplification` HEAD `f94b88b4c1307013f280c01d0c08f382eaf275f1`, clean (0 status lines). Brief SHA256 `c99224046e9fd56707ba72e80809a1b40320b2e4a59ec27c047695eb6645cef1` (4818 B) verified.

This index supersedes stale claims in older manifests and older text of the four records (wrong-surface candidate, unapplied/NOT_IMPLEMENTED proposals, high-risk-only framing, AC-3/AC-4 of the first draft). The older manifests and PNGs below are immutable history and are not edited; each image is labelled with what it actually shows, including failed and empty shots.

## Current actual scope

DEFAULT public; creator-only private includes the machine owner; legacy unowned stays local; normal full UI, public shared control, follow tracking and signatures. Application visibility, not OS isolation. Limits: zero-fee ACP fixture (no native steer), no paid Agent, cloud, CI or deployment evidence; sharing-enabled cross-host handoff without creator provenance stays local/unshared and remote activation is unsupported. Authorization sources: the user requests listed in intent.md only; no independent human approval or waiver is recorded and no high-risk rollout approval exists.

## R36 checks (real foreground/logged exits; Ghostty source and target dir reused, offline)

| check | exit | result |
|---|---|---|
| `cargo check -p codetwo-napi -p codetwo-desktop-host` | 0 | finished (current Event variants) |
| core: prompt_delivery 9, native_share 1, share_authorship 2, share_boundaries 3, share_device_sync_memory 1, share_store_failclosed 4, share_create_atomic 1 | 0 | all pass |
| `cargo test -p codetwo-core --lib share` | 0 | 5 pass |
| server: lib 59, pairing_link 1, share_policy 9, t3_share_privacy 4, team_collaboration 3, web_ui_commands 6, ws_share_parity 1 | 0 | all pass |
| `bun test` share, sharedTurnReconcile, sessionState, visualization, visualizationOrigin, visualizationResponse, windowChromeContract, transcript* | 0 | 132 pass, 0 fail, 9 files |
| `tsc --noEmit` | 0 | clean |
| `vite build --mode web` fresh | 0 | bundle below (not rendered) |
| `bun script/verify/sdlc.ts --worktree` | 0 | contract valid (first run exit 1: retained-resource owner/trigger lines missing in Verification, fixed in the record, not bypassed) |
| `bun script/verify/docs.ts` | 0 | catalog, links, schemas, assets valid |
| `git diff --check` | 0 | clean |
| lifecycle Eval (`checks.test.ts`, `four-stage.test.ts`, `devflow.test.ts`) | 0 | 32 pass; run directly although no checker changed

## Not done / remaining

Current-bundle rendered check (creator-private working row, 390 px header, actor whoami) after the R35 App change: NOT run, so there is no full PASS. R35 run 2 (stopped-C queue gone, continued-A Cancelled 16s kept, C author, one Working) passed on the R35 bundle; R34 creator-private/revoked passed on the previous bundle. Old-code red run of the R35 regression test, full Rust suite, independent review: not done.

## Cleanup

Removed: nothing of others'; no fixture, port or service was started in R36 (background wrapper scripts were reaped by the tool shell). Retained (private, ignored, owner parent coordinator): `.local/agent-work/20261013-full-sharing-final-r36/` (logs, dist-web, archive), `20261013-full-sharing/{deps,cargo-target}`, earlier round directories. Processes: none left from R36. Evidence: logs and hashes below.

## Hashes

#### Source files (SHA256, bytes)

- `apps/desktop/src-host/src/host_events.rs` dec79ca166241f8e967976b84df8c56029da0196877b88d4626d24c1f1ff092b 6072
- `apps/desktop/src-host/src/lib.rs` 4b7de10b3143fc419e6fd48d0d78d82c0d317665b887c5daae7daf2fd2f8f3d8 15070
- `apps/desktop/src-host/src/remote.rs` e9555d55792860487818cd3402991878b6b298ce6cfc127f864ad8e38c585e94 25845
- `apps/desktop/src/App.tsx` 66a4b5647652f43e7a68c637a9c32e2085e1436b5e12d015a00c503054872cef 358993
- `apps/desktop/src/bridge.ts` 30f63c082f48151234e36e2cbdad8c0e4f3757db97eac57c69a69f45bf4ec1b2 166185
- `apps/desktop/src/session/SideChatPanel.tsx` fe88d3b800fa212bd1f2837259e4ec77e2975d149a79b0622dfa4536dcedda2d 37384
- `apps/desktop/src/session/TurnCard.tsx` d0785ac5ee5a5d07e7e51711fddb8c1303384d84a7afea94c1fd6744c6269b3d 39705
- `apps/desktop/src/session/sessionEvents.ts` f48ac86f99289fd071db67b8d56d8e8f3f6094de684a5c8b1b226091f05d6cca 11991
- `apps/desktop/src/session/turns.ts` 5e59ead5fba1f87908b4e2bb9fdd397ef729d67b1f8a68dbf120a278de4398fd 35817
- `apps/desktop/src/sidebar/SessionRail.tsx` 87b2f567dcb634c00be1931291c698cd2e65e87eb193246fc48c8350ec8661d4 99403
- `apps/desktop/src/styles.css` a1e3a3bd8dbfd6eb3a6cb163a82a7bf8198beaaf0348bf3510a70fac70bd42ba 62396
- `apps/desktop/tests/sessionState.test.ts` df312a1d58ecc5f250a69b274cb026aef70a9b1b95a84e9f94857c6f211d653d 27662
- `apps/desktop/tests/windowChromeContract.test.ts` 2b49aad80c574a04c12c1c3c5c99561b37edebd01c40a7c22bf3029555fae580 13701
- `crates/core/examples/live_demo.rs` 9417cd07a1f49e45347ddddc7d926ab1e53b5deaf830345ec4b9e086473627d3 11368
- `crates/core/src/engine.rs` f2aef37f0307489a6c91373501aeb625bd244d5e0ec22f5cb45d14b2062aef8f 447558
- `crates/core/src/event.rs` 4ecaa8affeee9d475c1613abd3dc2d8949c0ae6368e6f0a48b2dcbebdea27091 30511
- `crates/core/src/lib.rs` 264542d7335285b9bbd1ce593546717f66d6cb60cf7817e5a8b53ccc8671ee46 10075
- `crates/core/src/plugins/app/mod.rs` 1bf0cbb4c5e201a086ae17ad2f820d2b169abe77ca57d4fdb09383790e0594a2 22804
- `crates/core/src/plugins/app/plugins/engine.rs` 349ac2598859a181a31a77df165cb5b22b1170acb65746790607d88fb325e7e0 36517
- `crates/core/src/prompt_delivery.rs` a549e99c59d94b0fd3688c094780cf7d9430eae7df61b0913a4f37bc5850496d 13204
- `crates/core/tests/prompt_delivery.rs` 92226a379e101c3e6026109f1e35da9d86982415538096416da757a13feb5260 21144
- `crates/napi/src/lib.rs` 8f234d274f05decadb301c47f3df6916decc79cb0f23158f5eab229677a9b045 3167
- `crates/server/src/lib.rs` d6f5749feb84c6b362766915bf4778e737313478076cb8944ee81d2c4cc0c9d4 167304
- `crates/server/src/serve.rs` 08a53f49a93587878869577a07d9232cf9e476841213c58a3b82b5e0b2196e1e 19931
- `crates/server/src/t3_compat.rs` c4a8e33930a7c80a3a107916904b452314383ef6c1302be71bfc1b7089487e0b 103230
- `crates/server/tests/team_collaboration.rs` c59ee7c62c33eb54dedddbf8727e6cd5366916304071c0d09fde1efa1bd18349 26404
- `crates/server/tests/web_ui_commands.rs` 28251cb0a729f07453947bdde622064dd3ebbcbe758f4ef27d46df7b8d26331f 13482
- `apps/desktop/src/session/ShareControls.tsx` f491d6f9e932e1f47d6f0bd347d48093b382d14d728e2068039f2fdccc42d38b 5491
- `apps/desktop/src/session/share.ts` ed856ab80b833d5f1b856bdc42dfe0e268dc0b60c6de46ae5ffb0d744660ca98 7183
- `apps/desktop/tests/share.test.ts` 171cb291e0be5ca7b50acf23d950340c2c1666a3e8b58857489a6e6d7c34dcfa 4815
- `apps/desktop/tests/sharedTurnReconcile.test.ts` c60f197c831c3d64fbfebc5e925d3bf2ef01da09aba3320b435038a06726e273 8754
- `crates/core/src/share.rs` cbc901c66ed21a83df64ea5ed3a045aa0d5a22d5dc828c2259403dcd9397a80a 23579
- `crates/core/src/share_policy.rs` f3eeb4e54a02b1358ed9e15e0ad4210bc21888740519b1d7e97303728a57614d 24305
- `crates/core/tests/native_share.rs` 883f0d011af00d9088f67bdc11d4da442376ad13c7e1b338a15a4778e076073d 7477
- `crates/core/tests/share_authorship.rs` 77c6ca4d5c8b0dee2593320e1c32f88f4a1166a0b9f340b87bd64b06609f532e 10396
- `crates/core/tests/share_boundaries.rs` 110fac46c868476f4443063dfebaf550b5b82812c20802ed0db4eb087469a81b 5335
- `crates/core/tests/share_create_atomic.rs` adca90ea959b9abb186055eecddbf7f8d697b5e529b8ad65f950117109d368f6 4501
- `crates/core/tests/share_device_sync_memory.rs` ef6a6c117ddc76e2a1dec1305f9d2f28b2ef7b64010b4fe27bb971bcdb8267ad 3325
- `crates/core/tests/share_store_failclosed.rs` 2247c2384a024f9f23643181ef7232ea1f47b9ae92cf364985ba9396ae252cfc 5435
- `crates/server/tests/pairing_link.rs` 2f0440d6d4f3069f1a1f532b69f00dd705cafb0115d913edf6c6c4f30299348e 1963
- `crates/server/tests/share_policy.rs` 0d0b4208fc4c71452cf4e81b2cef07db84fb13311641c41abe9a926ae601e695 16838
- `crates/server/tests/t3_share_privacy.rs` 8510394beb9da254f953ed2b237341cb79ec9984f15b677347eee1374e90f6a2 26280
- `crates/server/tests/ws_share_parity.rs` 9764d5d8db5dadddce0515456d51f88b3cf41b2b5c8b9922fb9fc09c814c2433 9417
- `crates/server/examples/share_fixture.rs` 8c9b81427c94bedfaa532fe5d7201bb331e3335ec171c98cd549e55df9646160 6350

#### Binary

- `.local/agent-work/20261013-full-sharing/cargo-target/debug/examples/share_fixture` 5f85af651f5ae88691d00b3be7fe345361907770ff7f85a2dea074398c85be1f 109538416 bytes (built 2026-10-14 00:49, before R36; not rebuilt as a fixture this round)

#### R35 final bundle (rendered in r35 run 2)

- `r35/dist-web/index.html` 1f21fd49d2dcaba8ffd2e2b60d2409c717de6b05b3a113ec6b3b91549edb23a7 498
- `r35/dist-web/assets/percentages-BXMCSKIN-QOQyy6hz.js` 44ddb1cef4771fa18e5f187c9ab03217f0adff8b4717bfc4e45fc63d3189ae94 5355705
- `r35/dist-web/assets/percentages-BXMCSKIN-CMwT83sK.css` 4fa6007116320bba66b834193fed7b1ef509f1bcef26dc5dcec96ddd824216f6 514802

#### Evidence PNGs and reports (SHA256, bytes)

- `r14-a-private-retained.png` 81d191e8ff55657478ea527a05ba1d884a7c1d20bf600dcf74961b5c5b496ae3 66351
- `r14-a-public-after-b-stop.png` 873c63ef784fa33554bb0515c70803703c01d6eeb3f5187cb1c0c8fed484f2d3 65302
- `r14-b-after-revoke-cleared.png` 05d8cd07b94ab808d22733b09bae5a9f2df206a60aa84521676765e1ddbe67fb 57540
- `r14-b-after-stop-authors.png` 7e8ae89c8b520234f1dd8a449261ba4f17d0b502394765cb6139609822869738 67317
- `r14-b-public-running.png` 57aa60518dd46a3df80efa0533374c0102dd254cc7c42f860fc61a6a5edaa297 70163
- `r15-a-creator-keeps-private-working.png` e588cf4dcc0d33221e5e2439f733b9d86873f8a484af193aceddb404bb3de8ee 63906
- `r15-b-rail-cleared-after-private.png` 03edb1649f0a589d8494692cbdb2357808ec10334fc3eaa2ac2113cbac7c1ae1 53910
- `r20-receiver-A-after-restart.png` f41b25a4192e6f077b21bf3338436bf55b8f4c8d8ca514c691e86ffa17a21c4f 61761
- `r20-receiver-C-after-restart.png` ef7514dbb8e523067ff2da024c5cdf7f098318230cfaa66c888b222c6f3d69b5 57771
- `r21-rail-public-creator-desktop.png` 20b8bf7aa982a0c9c1a9895fc72b9e9060ac976b99b6f727802831e6385e9923 59377
- `r22-A-dark-390-public-fault.png` 62158ec03661a210d1b973c703d34e54786d0f9f4e44a426831fbf63c7c0c4f7 116502
- `r22-A-private-dark-1280.png` 557c5a7fcbbfcfc16a337101226c24ac2535c5afe0cbf70be5835e95c204e807 70220
- `r22-C-foreign-running-desktop.png` ee7d950706cf6697c40327ce42045dc88c87b2dd1d082abc87e647fea0636de4 74124
- `r22-public-pane-desktop-before.png` 63e18810c7b0e3326bd9e4f46547f5bbc55a9211d5ff5e920ac95ba4c15ab98f 69177
- `r23-A-dark-390-public-header.png` 8e5a41af98cf062d1b8879a8883ce2b0802608f5c2321139f2088640278e7a2d 111865
- `r23-A-light-1280-public-header.png` 84f50097b772a564f94af41243bd3f19ba7543a1844fabc001e83fde92e80547 78338
- `r23-C-foreign-running-duplicate-placeholder.png` 0582d7d6c0b58a918d22b5016e326035e243cb38ca4dc34d666e1103468b35b6 75742
- `r24-C-foreign-running-single-row.png` 5aba02e22137939dd62353d488be11aaedd663d2cc5ae1fd08192cedc2f53a87 76268
- `r31-A-continued-C-author-1280.png` 324ea78b4f1a60ea843d2906f515cfe591d59dd1a4e19c0ee566d3e1f70c24a0 81423
- `r31-A-private-header-390-dark.png` 07f0690f09c1da80effb4a938807771d17d567944c05c81c21ba9a20bea20662 107663
- `r31-C-revoked-cleared-1280.png` 1c9ffc429de755b00a9c18872fa54f039ff9e590a277c5d5fa35112d1edd21e9 59284
- `r31-C-running-one-working-row-1280.png` bf0922613ec7d7f8be86451036d6f229c65e7535e76e64c039d220cde6ac926d 80846
- `r33-C-paired-localhost-empty.png` 231666bfd94608eac0679d446f060b1f188f661653dad0e92c84ec8bd91f136a 59307
- `r34-continued-A-lost-cancel.png` dcd3d5a562bf831b24d4396e88c4193031ca0ffa40783e09df34d3e0e0f4c89b 80363
- `r34-creator-private-A-working.png` b13b213da2868389b94aede18b15a233ba1d87cafbbca66d4b47be2cedfc0f89 81119
- `r34-queued-C.png` feb144c823349fc52f6d1c4e3f777951660239c03921a6fa3898102448d30837 80468
- `r34-revoked-C.png` 062d5e749f811e6b0f57fda2d539cb60986b5b7e7625eba0511887adeeb1255f 58845
- `r34-stopped-C-queued-row-stuck.png` 6f9fd2a08756c9b7ae21fb37cbc84e1efc2c693456db16ed02de1a51832caee3 80671
- `r35-browser-screenshot-127-0-0-1-mv6xqi0n-20a015da.png` e9257453622d8db062fb9cf35baf9f0eac08e4466bcb0ca8d047eb2c61d8d665 76016
- `r35-browser-screenshot-127-0-0-1-mv6xqwa3-625ee9a0.png` 007cbee01e7b876485da3d3f7a1794a372bd2df9fe4bf1ae49380f8d96cd626f 76087
- `r35-browser-screenshot-127-0-0-1-mv6xtpwb-d7fe4097.png` 524d1236a0405178574b83dbed995d5330dd9be9b1a54255ff14c398eef66322 84974
- `r35-browser-screenshot-localhost-mv6xq5qj-c70368e3.png` 0d1e73b20c749a2acab24e576bc146b53cd521bf47fea479872ca3a4acb4b6ea 73581
- `r35-browser-screenshot-localhost-mv6xtich-d8475f69.png` aa0e481080702e12bb544ccbdbc1459735be752d94d45cd5773c01b971bc6c38 81153
- `r14-manifest.md` 55afbf757b34afcfaaa3a9de470db978bb2b4e5a327dc200851407e37e621f0b 1950
- `r20-manifest.md` 320a5edef4ea0899b526b5e04c72a6ab1fff1607184a29fbd9f498834e6037d4 1327
- `r21-manifest.md` 83a9040500fcfe0d15cae210f2d71de96eae1ce7f2a00722f4d4e7aeea601222 1047
- `r22-manifest.md` 50a126ff056ada6f5f03706ffa4fb75dcd9f4e57844fad4201f8dc6fe5aa9250 2068
- `r23-manifest.md` c70682f4ea2d32dacb6f7f6472fa421cc6363e173749e31cb05f2346cb569ee0 1958
- `r24-manifest.md` e43fd0a8be162b7b46ca97b1f396c2f58e2eb251f943a7f6693784e350fc7d35 1568
- `r25-manifest.md` 8593704abe108b15fd69c7c8db74f06ba7edfcefaa3838c505ddb02c93936ee0 3375
- `r26-privacy-audit.md` d695936d81d49ed2c5f6d1fb6ab91d11a2e92cdc4fe77fc1dd7a14f8c7a85c19 6302
- `r27-privacy-audit.md` a463880e040abadd5cf7c34407124b400fd3a3df21339b721357e27a1ede46a9 3403
- `r28-privacy-audit.md` 7737463f3fd585c64fee309cf4c851f265b4735142227bb3569ee7a60e36e4a2 3560
- `r29-privacy-audit.md` 7adfb6bab8e1a96c3230d53c22e1ec4fde59756c29f4376000f1b8a1249c0464 4297
- `r30-privacy-audit.md` cfac21ab7001548da4a5a7b9d1ddc47c6e3112672ca1335a3b0991fe0010cb63 4704
- `r31-ui-manifest.md` 13af2989e915e6c72ebb6d57753b005a9412a1fa4c3f8e1288d8d68ef27e2111 6121
- `r32-state-manifest.md` c04238c3e4ec21fff8e2c1126d52525ca746a5ee2b8ce65b606a71115f585efa 2028
- `r33-result.md` 1b93d5fb258966869f2d885d11688ecbc2677f9fa1bd06bedd0cd7677e484ff9 2389
- `r33-state-manifest.md` c7471e22ab174ae080d1f34d8b6f5f6b6da90d18764350e4a4d9647c0577d4b9 1518
- `r34-state-manifest.md` 6c5aac11d2bed21280926d9f1c1d0ed74e3a82967880a059ce2a66137df85e7e 5122
- `r35-state-manifest.md` cd08aaee499e485a576c03572e283f69fadc5fb8ddb7cc2cbbd53ff96ebc0e93 4294

#### R36 fresh `vite build --mode web` bundle (exit 0; NOT rendered this round)

- `r36/dist-web/index.html` 1f21fd49d2dcaba8ffd2e2b60d2409c717de6b05b3a113ec6b3b91549edb23a7 498
- `r36/dist-web/assets/percentages-BXMCSKIN-CMwT83sK.css` 4fa6007116320bba66b834193fed7b1ef509f1bcef26dc5dcec96ddd824216f6 514802
- `r36/dist-web/assets/percentages-BXMCSKIN-QOQyy6hz.js` 44ddb1cef4771fa18e5f187c9ab03217f0adff8b4717bfc4e45fc63d3189ae94 5355705

assets/percentages-BXMCSKIN-QOQyy6hz.js`  

#### Exit logs (private, ignored)

- `r36/logs/cargo-check.log` a9cac404d986f633ea7be5438427e9a61d1e743c3f0c0b8eb707b5e9c9d9f61b 551
- `r36/logs/core-tests.log` c7b51b692f812857fe5afdb69c732905943fc404f317f6caddaf36a24c04d31d 4126
- `r36/logs/core-lib.log` e9d1f93cb6632c44517d8f788bac97890ef16cf5d4ea7369cb69eaf7674d9384 873
- `r36/logs/server-tests.log` 9d829f0bf12aea56a7284ba8371b0ea393c27cb5f9f5b12f4caa9d8a0f340d11 8850
- `r36/logs/tsc.log` e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 0
- `r36/logs/vite-web.log` 4f7538d32652a148df8b6f71eacbd73cee58c6b85a5b09f71276bf0c6e7d7b1a 125509
- `r36/logs/ui-tests.log` a4c54ac56b2e06c4b4a64eac51e8e049d160ba0a975bbd5bb786018ba04f189b 14616

#### Archived previous records (private, ignored; exact pre-R36 versions)

- `r36/archive-prev-records/intent.md` 80b5a1fb0997a3311862d3a39fd2b561a23b5ec7c97a7aca1655366c6f4548f6 2470
- `r36/archive-prev-records/spec.md` adafe2aaa1b0282c55d9bc3ea66acbb97086dcb46bdec8dea6c2b1266d41de23 9871
- `r36/archive-prev-records/plan.md` bfcf71367090f6227a21c589a55031c80821510945d84e5c52bfb0ef2a9af9b6 4478
- `r36/archive-prev-records/verification.md` 48762ef5381c39feb9feeeb05f4df3663e17e727f95e661ca7d97dc15d6f88c6 24082

## Images (all PNGs, labelled by actual content)

![R14 partial run (history; later fixes supersede defects)](r14-a-private-retained.png)

![R14 partial run (history; later fixes supersede defects)](r14-a-public-after-b-stop.png)

![R14 partial run (history; later fixes supersede defects)](r14-b-after-revoke-cleared.png)

![R14 partial run (history; later fixes supersede defects)](r14-b-after-stop-authors.png)

![R14 partial run (history; later fixes supersede defects)](r14-b-public-running.png)

![R15 rail revoke fix](r15-a-creator-keeps-private-working.png)

![R15 rail revoke fix](r15-b-rail-cleared-after-private.png)

![R20 receiver after restart (PASS)](r20-receiver-A-after-restart.png)

![R20 receiver after restart (PASS)](r20-receiver-C-after-restart.png)

![R21 rail share indicator (partial)](r21-rail-public-creator-desktop.png)

![R22 open-fault render (header overflow and missing Working were faults, fixed later)](r22-A-dark-390-public-fault.png)

![R22 open-fault render (header overflow and missing Working were faults, fixed later)](r22-A-private-dark-1280.png)

![R22 open-fault render (header overflow and missing Working were faults, fixed later)](r22-C-foreign-running-desktop.png)

![R22 open-fault render (header overflow and missing Working were faults, fixed later)](r22-public-pane-desktop-before.png)

![R23 header fix render; C-duplicate-placeholder shows a FAULT (fixed in R24)](r23-A-dark-390-public-header.png)

![R23 header fix render; C-duplicate-placeholder shows a FAULT (fixed in R24)](r23-A-light-1280-public-header.png)

![R23 header fix render; C-duplicate-placeholder shows a FAULT (fixed in R24)](r23-C-foreign-running-duplicate-placeholder.png)

![R24 single row after fix](r24-C-foreign-running-single-row.png)

![R31 PASS: one Working row / continued author / revoke / 390 dark](r31-A-continued-C-author-1280.png)

![R31 PASS: one Working row / continued author / revoke / 390 dark](r31-A-private-header-390-dark.png)

![R31 PASS: one Working row / continued author / revoke / 390 dark](r31-C-revoked-cleared-1280.png)

![R31 PASS: one Working row / continued author / revoke / 390 dark](r31-C-running-one-working-row-1280.png)

![R33 EMPTY shot: pairing consumed, no content (not a PASS)](r33-C-paired-localhost-empty.png)

![R34 FAIL: Cancelled lost, 0s (fixed R35)](r34-continued-A-lost-cancel.png)

![R34 PASS creator-private Working](r34-creator-private-A-working.png)

![R34 PASS: ONE Working + Queued #1](r34-queued-C.png)

![R34 PASS revoked C cleared](r34-revoked-C.png)

![R34 FAIL: Queued #1 stuck after Stop (fixed R35)](r34-stopped-C-queued-row-stuck.png)

![R35 run1 FAIL A after continue 0s (bundle 1)](r35-browser-screenshot-127-0-0-1-mv6xqi0n-20a015da.png)

![R35 run1 FAIL second continue 0s (bundle 1)](r35-browser-screenshot-127-0-0-1-mv6xqwa3-625ee9a0.png)

![R35 run2 PASS A continued, Cancelled 16s kept, C author, one Working (final bundle)](r35-browser-screenshot-127-0-0-1-mv6xtpwb-d7fe4097.png)

![R35 run1 C stopped at 22s, queue cleared (PASS, bundle 1)](r35-browser-screenshot-localhost-mv6xq5qj-c70368e3.png)

![R35 run2 PASS C stopped 16s queue gone (final bundle)](r35-browser-screenshot-localhost-mv6xtich-d8475f69.png)
