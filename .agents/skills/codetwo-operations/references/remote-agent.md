# C2 Remote Programming Agent

Status: **current operator guide** for `codetwo-agent` and task handoff.

`codetwo-agent` runs C2's ACP coding runtime without the desktop interface. The remote machine owns
the provider process, workspace, terminals, transcript, and access credentials. T3 Code mobile and
the C2 browser remote connect to the same listener.

## Build and install

Build a native executable on the target operating system and architecture:

```bash
cd apps/desktop
bun install
bun run build:remote-agent
install -m 0755 dist/remote-agent/codetwo-agent ~/.local/bin/codetwo-agent
```

The target machine also needs the selected ACP provider installed and authenticated. Provider
availability is reported to connected clients; unavailable providers cannot create a task.

## Start a programming node

```bash
codetwo-agent --port 4599 /path/to/workspace
```

Startup prints a one-time T3 pairing URL. Use `--protocol legacy` for the C2 browser remote,
`--no-pair` when credentials are provisioned separately, and `--json` for process supervisors.
Durable state is stored in `~/.codetwo-agent` unless `--data-dir` or
`CODETWO_AGENT_DATA_DIR` is set.

For a CI runner or service manager, keep the workspace and data directory on persistent storage and
send `SIGTERM` for a clean shutdown. Use the `/health` endpoint as the readiness probe.

## Move an unfinished task

1. Start `codetwo-agent` on the destination device and copy its T3 pairing URL.
2. In the source C2 session, open **Open** → **Move task to device**.
3. Paste the pairing URL and choose a new destination folder on the remote device.

C2 pauses the source at a durable boundary before transfer. The destination receives the complete
transcript and execution settings, the Git commit graph and baseline, staged and unstaged binary
patches, and non-ignored untracked files including binaries and symbolic links. The source is fenced
before the destination becomes active, so only one device can continue the task. If the provider's
ACP session cannot be loaded on the new machine, the first remote turn receives the preserved task
context before the new prompt.

Dirty submodules are rejected because their nested repository state cannot be represented by the
parent repository bundle. Resolve or separately transfer a dirty submodule before moving the task.

## Run a full headless C2 server

`codetwo-agent` is a single-workspace ACP node. For a machine that should host many projects and
sessions and be used from a desktop C2, browser or phone, run `codetwo-server serve`:

```bash
codetwo-server serve --data-dir /srv/c2 --host <tailnet-or-loopback-ip> [--public-url https://…]
codetwo-server pair --data-dir /srv/c2     # fresh one-time link for another device
```

It owns `/srv/c2` exclusively (a second `serve` is refused and a stale `server.pid` from a crash is
replaced), stops cleanly on `SIGTERM`, and is probed at `/health`. Use `--public-url` when clients
reach it through a proxy. The desktop pairs from **Settings → Remote environments**, where **Manage devices** lists and revokes the
server's paired devices; a revoked device loses access immediately, no restart needed. Never stop another operator's daemon to reuse its data
directory or port; pick a new `--data-dir`.

Example `systemd` unit:

```ini
[Service]
ExecStart=/usr/local/bin/codetwo-server serve --data-dir /srv/c2 --host 100.x.y.z
Restart=on-failure
KillSignal=SIGTERM
```

## Network boundary

The agent listens on all interfaces. Bind it to a trusted LAN or tailnet, or place it behind an
authenticated TLS tunnel. Pairing credentials are one-time; paired device access can be revoked
from C2. Do not publish the raw port to the public internet.
