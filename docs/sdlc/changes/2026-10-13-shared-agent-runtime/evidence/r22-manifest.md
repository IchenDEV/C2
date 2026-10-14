# R22 actual UI render manifest (2026-10-13 22:19-22:26 +08)

Fixture: share_fixture (zero-fee ACP stub), port 52943, r19 fx-data/proj reused, dist-web from R21.
Entry JS percentages-BXMCSKIN-CNH3iJqM.js sha256 64959b2c4bd32f93446210c243cdf37e11ecf0aff74c4b83359629326d56e013;
CSS percentages-BXMCSKIN-D4ibvHfr.css sha256 19807d2c840a1372d15db2191037df66f85cb78be153beaded2ab299aa6e8c52.
Source: turns.ts sha256 5e4677d0...673b (33354B, unchanged), ShareControls.tsx sha256 f491d6f9...d38b. No source change in R22.
Measured: DPR 2, CSS 1280x800 (and 390x800 for the narrow shot), PNG 1280x800 (780x1600 for 390 shot @2x scale). Actors: A=Guest-0cfb (default profile), C=Guest-233c (incognito), creator Guest-9d9a.

| PNG | what it shows |
|---|---|
| r22-public-pane-desktop-before.png | A opened public task; rail "PUBLIC · Guest-9d9a"; ShareControls Public/Follow/nickname/Share link; light 1280 |
| r22-C-foreign-running-desktop.png | C (incognito) viewing A's running turn: Stop/Queue present; rail Working; transcript shows NO "Working…" for the foreign running turn; header View button clipped past 1280 |
| r22-A-dark-390-public-fault.png | dark 390: share group + session actions + View overflow the 390 viewport (x up to ~650); nickname/Share link clipped - UNFIXED fault |
| r22-A-private-dark-1280.png | A private task dark 1280: rail "PRIVATE · Guest-0cfb", Make public shown; header overflows (View at x=1323) - UNFIXED fault |

Behaviour observed: A Run -> turn Working; C Stop cancelled A's turn ("Cancelled 50s"); both UIs then idle, no Working, no "(started elsewhere)" row. C's rail lists only the public row while A's private task exists.
Not done: C queue/message, C continue after stop, public->private live clear, native host deny, legacy label, light 390/480, dark 480, console read, fix of narrow-header overflow.

![public pane before](r22-public-pane-desktop-before.png)
![C foreign running](r22-C-foreign-running-desktop.png)
![dark 390 overflow fault](r22-A-dark-390-public-fault.png)
![private dark 1280](r22-A-private-dark-1280.png)
