# Remote control

Drive C2 from your phone, tablet, or another machine. The desktop listener supports the native
T3 Code mobile protocol as well as C2's mobile-friendly web client.

## How pairing works

The remote device never needs a long-lived secret up front:

1. Choose **T3 Code mobile** or **C2 browser**. C2 mints a protocol-bound **one-time pairing
   token** (15-minute lifetime) and shows it as a URL + QR code. The token rides in the URL
   *fragment* (`/pair#token=…`), so it never appears in server logs and cannot be exchanged through
   the other client's authentication endpoint.
2. The device opens the link once and exchanges the token for a **per-device bearer**, stored by the
   selected client. The pairing link is now dead.
3. On every connect, the bearer buys a **five-minute single-use WebSocket ticket** — the only
   credential that ever appears in a socket URL.

Paired devices survive restarts (stored as hashes in `remote-devices.json`) and can be revoked at
any time. Treat pairing links like passwords while they're live.

## Two ways to start it

**From the desktop app** — open the command palette (`Mod+K`) → **Remote control** → **Turn on
network access**. This serves the app's live engine *in-process*, so the remote and your desktop
drive the **same running sessions**. The modal shows:

- the reachable endpoints (physical LAN, Tailscale tailnet, and loopback),
- a client choice and a separate one-time pairing link with a QR code (**Create pairing link** /
  **New link**),
- the paired devices, each with a **Revoke** button.

Turning network access off stops the server; paired devices reconnect next time you turn it on.

For Tailscale, install and sign in on both the Mac and phone, then choose the **Tailnet candidate**
whose address matches the Mac address shown by Tailscale. C2 detects `100.64.0.0/10` addresses
separately from physical LAN addresses, but that range is shared CGNAT space and is only a
best-effort Tailscale signal. The phone can connect from another network as long as the tailnet
access policy allows TCP traffic to the selected C2 port.

**Standalone C2 remote** — run it on the machine with your code. Native T3 compatibility is
currently hosted by the desktop app's in-process listener; this launcher follows the C2 daemon
remote protocol:

```sh
cargo run -p codetwo-server
```

It prints a pairing panel: a one-time **URL** (with its auto-detected LAN IP), the **token**, and a
scannable **QR code**. Open the URL on another device on the same network, or scan the QR.

```
  C2 remote is live.

  Open on another device (link is one-time, expires in 15 minutes):
    http://192.168.1.42:4599/pair#token=…

  Pairing token: …

  █▀▀▀▀▀█ …  (QR)
```

Configure it with env vars:

| Var | Default | Meaning |
| --- | --- | --- |
| `CODETWO_HOST` | `0.0.0.0` | bind address |
| `CODETWO_PORT` | `4599` | port |
| `CODETWO_PAIR_TTL` | `900` | pairing-token lifetime (seconds) |

## Run C2 on a server (headless)

`codetwo-server serve` is the daemon for a remote machine. It runs the same engine as the desktop
app with no window, and serves the full C2 web UI too when the `web-ui` assets sit next to the
binary (or `--ui-dir` points at them). Sessions, workspaces, provider logins and terminals all live
on the server.

```sh
codetwo-server serve --data-dir /srv/c2 --host 127.0.0.1 --port 4599
```

- **Safe by default.** It binds `127.0.0.1`. Reach it with an SSH tunnel
  (`ssh -L 4599:127.0.0.1:4599 server`), put it on a tailnet with `--host <tailscale-ip>`, or front
  it with an HTTPS reverse proxy and tell clients the public address with
  `--public-url https://c2.example.com`. Do not publish the raw port to the internet.
- **One owner per data directory.** A second `serve` against the same `--data-dir` refuses to start
  instead of fighting the first for the database.
- **More devices later.** Startup prints a one-time pairing link. On the server,
  `codetwo-server pair --data-dir /srv/c2` asks the running daemon for a new link; nothing on the
  network can mint credentials.
- **Clean shutdown.** `SIGTERM` stops providers and flushes the store.

The provider CLIs (Codex, Claude Code, …) must be installed and signed in **on the server**; the
server's sessions use the server's checkout of your code.

### Connect from your desktop C2

Open **Settings → Remote environments**, paste the pairing link, and press **Connect**. The server
then behaves as part of this app:

- its sessions are listed in the sidebar beside this Mac's, marked with the server's name, and
  update live;
- opening one shows the same transcript, permission prompts, model and mode controls, and
  Stop button; each action is sent to the machine that owns the session;
- to start a new session on the server, pick it in the **Environment** menu (it then names the
  header button), optionally setting a *Workspace on the server* in Settings. Pick **Local** to go
  back.

Pairing stores a per-device credential on this Mac. **Remove** forgets it on this Mac only. To cut a
device off on the server, press **Manage devices** under the environment: it lists every device
paired with that server (this app is marked), and **Revoke → Revoke now** disconnects one at once,
without restarting the daemon. Revoking this app's own entry also removes the environment here.
Any paired device can revoke any other, so treat a pairing link like a login. If the server is unreachable its sessions disappear from the list while
the rest keeps working, and they come back when it returns.

A remote session's folder is shown as `c2env://<environment>/path` in this app. That prefix is what
sends the file editor, workspace search, Source Control and the terminal to the server instead of
this Mac, so a path that exists on both machines is never confused.

::: info What a remote session can and cannot do (today)
Remote sessions support chat (prompts, queue/steer, cancel, permission and question answers,
model/mode/sandbox changes, rename, pin and archive), the file browser and editor, workspace search,
Source Control (status, diff, stage, commit, push, revert, checkpoints) and terminals. A terminal
reattaches to the same shell after a network drop and after the app restarts.

Still local-only: running project scripts, GitHub pull-request actions, language servers, the
browser dock, parallel tasks and worktree creation, and "reveal in Finder". They show an error or
stay empty for a remote session. The server's own web UI (its address, `/`) remains available.
:::

The same server also works from a phone: open its pairing link in the phone's browser, or use T3
Code mobile with a `codetwo-agent` node.

## Mobile clients

In the desktop app, choose **T3 Code mobile**, then scan the pairing QR from T3 Code mobile's
connection screen. C2 implements T3's environment discovery, OAuth token exchange,
authenticated WebSocket ticket and Effect RPC bootstrap, then projects C2 sessions and
transcripts into T3's shell and thread snapshots. Text prompts, cancellation, runtime mode, model
selection and permission answers are translated back to the same live C2 engine. Plan mode uses
C2's `plan-first` skill as a best-effort equivalent. Image attachments and T3's terminal, Git,
review, preview and hosted-relay surfaces are not part of this compatibility layer yet.

The page served at `/` is a lightweight web client: it pairs itself from the link, reconnects on
its own afterwards (no re-pairing), lists sessions, replays the transcript of the session you pick,
lets you choose a provider and type a prompt, streams the agent's output, and shows permission
prompts with Allow/Reject buttons. This remains available when you do not want to install a phone
app.

If the connection lags badly (slow cellular link), the client is told how many events it missed and
resyncs the transcript instead of silently dropping the socket.

## What's shared

The standalone server opens the same `~/.codetwo/codetwo.db`, so it sees the same session list and
history as the desktop. The in-process (desktop-started) server additionally shares the **live**
event stream, so a turn you start remotely streams to both.

## Managing access

- **Desktop**: the Remote control modal lists paired devices with pair date and last-seen time;
  **Revoke** cuts a device off immediately (including any unredeemed tickets).
- **From another C2 desktop**: Settings → Remote environments → **Manage devices** on that server.
- **Standalone, no other desktop**: delete an entry from `~/.codetwo/remote-devices.json` (or the
  whole file) and restart the server.
- A fresh pairing link never invalidates existing devices; revoke explicitly.

## Security

::: warning
The server binds `0.0.0.0`, so it's reachable by anything on your LAN and by permitted peers on an
active Tailscale tailnet. Access requires pairing, and
credentials are one-time (pairing token), header-only (bearer), or single-use (ws ticket) — but the
transport is plain HTTP. That's fine on a trusted home/office network. For untrusted networks,
front it with a TLS tunnel (e.g. an SSH tunnel) or use the explicitly listed Tailscale endpoint.
A C2-hosted relay isn't built in yet.
:::
