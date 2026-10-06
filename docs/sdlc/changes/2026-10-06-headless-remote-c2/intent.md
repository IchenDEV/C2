---
id: 2026-10-06-headless-remote-c2
schema: 5
stage: intent
status: accepted
owner: cursor-agent
created: 2026-10-06
source: user
risk: high
approved_by: user
approved_at: 2026-10-06
approval_source: "Current chat: 研发无头版本的 c2 对标 t3 code 在服务器上可以运行的版本。我可以用我本地的 C2 远程连接到服务端的 C2 进行开发，然后会话什么的都是可以聚合的。你可以参考一下 T3 Code 的实现，甚至我还可以在手机上远程去使用。"
---

# Intent: Headless Remote C2

## Intent

Let C2 run headless on a server and let the user's local C2 use it, T3 Code style: the desktop
pairs with a remote C2, its sessions appear next to local ones, and each session keeps running on
the machine that owns it. A phone can use the same server through its browser.

The existing `codetwo-server` (compact remote, `webui`) and `codetwo-agent` (single-workspace ACP
node) already cover browsers and T3 mobile; the gap is a deployable daemon and a desktop that can
federate remote Cores. Constraints: one live Core owner per data directory; credentials stay
header-only or single-use; the server must not expose any network route that mints credentials; a
user's running desktop or daemon is never stopped to free a port or directory; local behavior is
unchanged when no environment is configured.

Non-goals (this change): remote worktree creation, project scripts, GitHub and LSP surfaces inside the
desktop for remote sessions; T3-mobile protocol on `serve`; a hosted relay or built-in TLS; moving running
sessions between machines (already covered by task handoff); release or publication.
