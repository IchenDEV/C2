# R14 shared-task UI evidence manifest (2026-10-13, disposable zero-fee fixture)

Source: branch `research/shared-agent-20261008` HEAD `926ebdb5` plus uncommitted worktree. Fixture `crates/server/examples/share_fixture.rs` sha256 `43ffce9a1afcb1f6f333f4ce5ef094579c3bc4e533f67ce598b4ae9d593d3102` (ACP stub now delays a prompt up to 20s so a RUNNING state exists, `session/cancel` ends it). `crates/server/src/serve.rs` sha256 `7d2b8c302d0933fbffd871f2485961608720822820f043a89465652be93d6a6a`. Web bundle `index.html` sha256 `210136c36fb1ac7c318492c21f32b150db00dff119c40471a70b8efb85d8f51a` (unchanged R13 build). Captured 2026-10-13T11:44Z-11:46Z through T3 `preview_snapshot({save:true})`; PNG 1280x800 = CSS 1280x800 viewport (DPR 1 by size match), default light theme, no console errors reported. Two real browser origins: A=127.0.0.1 (device `3714ed74`), B=localhost (device `a1a03caa`). No tokens in images.

| file | sha256 | what |
| --- | --- | --- |
| `r14-b-public-running.png` | 57aa60518dd46a3df80efa0533374c0102dd254cc7c42f860fc61a6a5edaa297 | B pane of A's public task, B message sent, Working |
| `r14-b-after-stop-authors.png` | 7e8ae89c8b520234f1dd8a449261ba4f17d0b502394765cb6139609822869738 | B pane after B Stop; authors Aye/Bee visible, Following You, Aye |
| `r14-a-public-after-b-stop.png` | 873c63ef784fa33554bb0515c70803703c01d6eeb3f5187cb1c0c8fed484f2d3 | A pane, same task, Public/Make private |
| `r14-b-after-revoke-cleared.png` | 05d8cd07b94ab808d22733b09bae5a9f2df206a60aa84521676765e1ddbe67fb | B pane cleared after A made task private |
| `r14-a-private-retained.png` | 81d191e8ff55657478ea527a05ba1d884a7c1d20bf600dcf74961b5c5b496ae3 | A keeps Private/Make public and full transcript |

![B after stop](r14-b-after-stop-authors.png) ![B after revoke](r14-b-after-revoke-cleared.png)
![B public running](r14-b-public-running.png) ![A public](r14-a-public-after-b-stop.png) ![A private](r14-a-private-retained.png)
