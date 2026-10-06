# The C2 Plugin Protocol

Status: **normative protocol contract** for external process extensions.

Version **1.0.0**.

This document defines the process wire format. Bundle terminology, manifest namespacing, trust,
policy, contribution support, and host capability rules are normative in the
[C2 Plugin Standard 1.2.0](plugin-standard.md).

An external extension runtime is a process. C2 speaks JSON-RPC 2.0 to it over stdio. Its commands
are declared statically in the Bundle Manifest; `initialize` confirms their implementation and may
subscribe to events. The host registers both in the same scoped kernel registries used by built-in
Rust runtime modules. Installed runtimes have stable managed names of
the form `bundle:<id>`. Their commands appear in `kernel.commands`, are callable from a frontend in
the matching command realm, and disappear the instant the runtime unloads.

Write one in any language that can read stdin and write stdout.

## Why a protocol at all

[`docs/reference/plugins.md`](plugins.md) defines C2's internal runtime-module graph, but a Rust host can only
load modules it was compiled with. This protocol is the external extension seam, using the
transport the app already speaks twice over (ACP to provider CLIs, MCP to tool servers) rather than
inventing a third.

## Transport

- **Framing** — one JSON object per line, UTF-8, `\n`-terminated. No `Content-Length` header.
- **stdout** is the protocol channel. Anything on it that is not JSON is dropped with a warning, so
  a stray `print()` degrades to noise rather than a crash.
- **stderr** is your log channel. C2 routes it into its own tracing output.
- The process is killed when the plugin unloads, and it never outlives the app.
- Frames are limited to **1 MiB including the newline**, even if a process never sends a newline.
  Each peer admits at most **64 pending host requests**, queues **64 outbound messages**, and runs
  at most **16 concurrent plugin-to-host callbacks**. Oversized outgoing requests and excess host
  requests fail before dispatch. Oversized input, callback overflow, or notification/response queue
  overflow terminates that peer and fails its pending calls. Host callback tasks are cancelled on
  peer closure.

## Handshake

On the first invocation of a declared command, the host starts the process, sends `initialize`
first, and waits **10 seconds**. Answer it promptly. A process that misses the window is killed and
that scope generation's activation fails; its dormant command stubs remain fail-closed until the
plugin is reloaded or disabled and re-enabled.

**Host → plugin**

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
  "protocolVersion": "1.0.0",
  "host": { "name": "code2", "version": "0.0.0", "commands": ["git.status"], "observations": false },
  "config": { "…": "your entry from the plugin config, verbatim" },
  "dataDir": "/home/me/.codetwo/plugins/.data/my-plugin/projects/09a7…",
  "projectPath": "/home/me/work/my-project"
}}
```

**Plugin → host**

```json
{"jsonrpc":"2.0","id":1,"result":{
  "name": "my-plugin",
  "version": "0.1.0",
  "protocolVersion": "1.0.0",
  "commands": [
    { "name": "my.greet", "description": "Say hello.", "schema": { "type": "object" } }
  ],
  "events": ["engine/event"]
}}
```

For a C2 1.2 bundle, `commands` MUST contain exactly the IDs and schemas declared by
`extensions.dev.codetwo.commands`. It is implementation confirmation, not a second contribution
source: missing, extra, duplicate, or changed schemas are refused and the process is killed. The
host uses Manifest titles and descriptions.

Wire compatibility is by **major version**. Declaring `2.x` to a `1.x` host, or omitting
`protocolVersion`, is refused with a readable error.

`dataDir` is yours to write to and is created before you start. A user-scoped instance gets
`<plugins-dir>/.data/<id>`. A project-scoped instance gets
`<plugins-dir>/.data/<id>/projects/<blake3(normalized-project-path)>`, so two projects do not share
runtime files. `projectPath` is the normalized project identity for that instance and is omitted
for the user-scoped instance.

`host.commands` is the **extension-public** callable surface visible from this realm at
initialization time. Internal Core and frontend commands are omitted. A user-scoped instance sees
public global commands only. A project instance sees public global commands plus public commands
registered for the same normalized project; it never receives another project's command list.
`command/call` rechecks the public marker and uses that same realm's normal project
fallback/blocking rules, so guessing an internal command name does not grant access.

`host.observations` is an optional boolean, default `false`, on the host object. It is `true` only
when this runtime owns a connector declaring `observations` and the host provides the Store seam.
Adapters must negotiate it before submitting feedback; an unsupported host must not be retried as
if it had accepted a batch. This additive field does not change protocol-major compatibility.

## Methods

### Host → plugin

| method | kind | params | result |
|---|---|---|---|
| `initialize` | request | see above | see above |
| `command/invoke` | request | `{ name, args }` | whatever your command returns |
| `event/emit` | notification | `{ name, payload }` | — |

`command/invoke` only ever names a command declared in the Manifest. Returning a JSON-RPC error turns into a
readable failure at the caller — the frontend sees `my.greet: <your message>`.

Commands and host callbacks have a **60-second** default deadline. A bundle may set
`runtime.commandTimeoutMs` to an integer from **1 to 3,600,000** for long operations; this does not
change the independent 10-second initialize deadline. Older C2 builds that do not recognize this
optional manifest field reject it; omit it when targeting those builds.

Timeout or cancellation of a sent command terminates the peer's process and fails every pending
call in that realm. Protocol 1.0 has no cancellation acknowledgement, so cancelling only one remote
operation cannot be guaranteed. Reload or disable/re-enable the plugin before retrying an activated
peer; C2 never automatically repeats a command that may already have produced side effects.
Cancellation during initial activation still permits a subsequent activation attempt.
An extension command that collides with a global command owned by Core or another module is rejected
during activation, so a project extension cannot shadow host dispatch. The same project-capable
extension may register its own command name in both global and project instances.

When a user activates a manifest `ui` contribution, C2 first verifies bundle ownership, trust,
enablement, the selected user/project realm, and that the contribution's command was registered by
this process. The resulting `command/invoke` uses these args:

```json
{
  "context": { "cwd": "/repo", "projectPath": "/repo", "sessionId": "session-id" },
  "input": { "mode": "working-tree" }
}
```

`context` is host state for this activation; `input` is the descriptor's static JSON value. Neither
is a capability grant. Host commands remain accessible only through the allowlisted, realm-aware
`command/call` seam.

### Plugin → host

| method | kind | params | result |
|---|---|---|---|
| `command/call` | request | `{ name, args }` | the extension-public command's result |
| `observation/record` | request | scoped batch, see below | durable batch receipt |
| `event/emit` | notification | `{ name, payload }` | — |
| `log` | notification | `{ level, message }` | — |

`command/call` reaches an extension-public command visible in the process's realm, by name, through
the same registry a Rust runtime module uses. Commands are internal by default and must be
deliberately published by Core. `git.status` is the initial read-only public command; mutating Git,
plugin management, credentials, and other Core commands stay internal. If a public command's owner
is turned off or project fallback is blocked, the call fails through the normal command path.

`level` is one of `error`, `warn`, `info`, `debug`, `trace`.

### Observation record

This host-owned callback is separate from `command/call`; internal `assistant.edit` remains
inaccessible to extensions. The raw process owner, command realm and manifest connector are
verified by the host. They cannot be supplied or replaced by payload fields. Connector/account
bindings are user configuration; adapter-reported remote identities are not host-authenticated users.

Request fields are `connector_id`, `account_scope`, `stream_id`, stable `batch_id`,
`recovery` (`resumable` or `live_only`), `checkpoint_before`, `checkpoint_after`, and `events`.
Checkpoints are opaque strings or actual JSON null. Each event has `event_id`, `kind`, `occurred_at`,
`actor_id`, `resource_id`, `object_id`, nullable `object_version`, `reply_to`, `content`, `reference`,
and `content_origin` (`event`, `snapshot`, or `reference`). Complete the network read before calling
the host. Reference-only inputs are allowed; Core does not fetch arbitrary URLs. Event ids must be
stable within the source, and batches must cover the entire before/after interval.

Limits are 16 events per batch and 16 KiB UTF-8 content per event. Oversized content with a reliable
event id is recorded as terminal rejected metadata; it is never silently truncated. Canonical event
JSON is SHA-256 hashed with a fixed field order, excluding transport headers and host receipt time.
One input body is retained for all goal receipts. Same source/event/hash recovers its receipt;
different content under the same key is quarantined without replacing the original. Opaque object
versions are never ordered by time or lexical comparison.

The response is `{status,batch_id,current_checkpoint,event_receipts,retry_after_ms}` with additive
`reason` when needed. Only `recorded` acknowledges persistent consumption. All inputs, filtering or
rejection metadata, per-goal receipts, batch result and checkpoint CAS commit in one Store transaction.
Empty unchanged polls are `noop` without rows; empty cursor advancement is an adapter's explicit
coverage attestation and is recorded. `backpressure`, `out_of_order`, `unsupported`, `rejected` and
`needs_reset` do not acknowledge consumption. A timeout/disconnect is unknown: recover the exact
original batch id and parameters, never invent a new id and replay a possible effect.

Each source/stream has at most one in-flight record; each runtime has at most four across streams.
Observation overload returns structured backpressure and does not shut down unrelated callbacks.
The short local Store transaction outlives callback cancellation until it finishes, preserving that
in-flight limit. Provider/network calls do not hold the assistant control gate or this transaction.

Optional `cursor_invalid` records the provider's reason and returns `needs_reset`. A user may approve
an exact source version, stream, old checkpoint, new baseline and gap reason through
`assistant.edit` `source_reset`. `assistant.observations` exposes the resulting approval reference.
The adapter then submits `reset: {approval_ref}` on the matching record with those exact
before/after values. The host rejects invented, stale or mismatched approvals and consumes the
approved reset in the same transaction; recording an approval does not claim the adapter completed
the reset. Source/account identity, deduplication keys and unknown attempts survive resets.

Follow-up uses the existing per-goal Runtime with fixed input references and source/control/version
checks. Core allows only sourced reports, questions and proposed changes. External feedback cannot
dispatch work, steer workers, change control, accept deliverables, confirm memory, answer permission
requests or send messages. Paused/budget/attention and unknown receipts stay distinct. Source
capacity, gaps and ambiguous input references are visible in the existing notification channel and
on-demand Event sources inspector; arrival does not increment the user's assistant revision.
The binding, bounded retention and observation-only authority contract above applies to this
protocol. It does not implement a platform subscription or a cloud listener.

## Events

You receive only the events you name in `events`. The host publishes:

| name | payload |
|---|---|
| `engine/event` | one agent-loop `Event` (agent text, tool call, permission request, turn ended, …) |
| `skills/changed` | `null` — the skill library was rebuilt |
| `scenes/changed` | `null` — the scene library was re-resolved |

This list is the contract. Typed Rust events do not cross a pipe, so each entry is a deliberate
decision to expose one — see `publish_host_events` in
`crates/core/src/plugins/app/plugins/extensions.rs`.
Because activation is command-driven, event subscriptions begin only after the first command
has successfully initialized the process; events emitted while the runtime is dormant are not
buffered or replayed.

Your own `event/emit` normally goes onto the host's JSON bus, where other plugins (in or out of
process) can subscribe to it. The JSON bus is currently host-wide, not project-confidential:
project process isolation does not filter events by realm. Do not put project secrets on an event
merely because the sender or subscriber is a project-scoped runtime.

`connector/event` is reserved for a host-rendered connector's provider notifications. It does not
enter that public JSON bus. The host wraps the payload in a typed internal event and adds the
authenticated installed-bundle id; any `plugin_id` supplied by the process is ignored. Desktop
adapters must match that owner and the active connector's bundle-local `connectorId` before using
the event. A connector event payload uses this minimum envelope:

```json
{
  "connectorId": "workspace",
  "eventId": "provider-event-or-message-id",
  "kind": "message.created",
  "createdAt": "1724900000000"
}
```

Provider-specific resource ids and bounded summaries may be added. The process must deduplicate
at-least-once provider delivery before emitting; the host-rendered adapter repeats that guard before
changing local activity state. Event payloads are not a way to inject UI or bypass connector command
ownership.

## Declaring a plugin

Every C2 bundle uses the Agent Plugins 1.0.0 root schema and the mandatory C2 extension. Add the
runtime under C2's client-extension namespace; a top-level `runtime` field invalidates the bundle:

```json
{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "my-plugin",
  "version": "1.0.0",
  "extensions": {
    "dev.codetwo": {
      "standardVersion": "1.2.0",
      "commands": [{
        "id": "my.greet",
        "title": "Say hello",
        "description": "Greet the current user.",
        "argsSchema": { "type": "object", "additionalProperties": false }
      }],
      "runtime": {
        "protocol": "1.0.0",
        "command": "node",
        "args": ["dist/plugin.js"],
        "env": { "MY_PLUGIN_MODE": "release" },
        "inject": ["store"],
        "optionalInject": ["engine"],
        "scopeSupport": ["user", "project"]
      }
    }
  }
}
```

When activated, the process starts with the bundle directory as its working directory.

`inject` gets you the same reactive contract a Rust plugin has: command stubs are not made ready
until those services exist, and an active process is stopped when one is replaced. It is declared
in the Manifest because the host needs it before making the adapter eligible.

`scopeSupport` is an explicit capability declaration. If it is omitted, the runtime supports only
`["user"]`. Include `"project"` only when the process is prepared for
one independently managed process, command realm, `dataDir`, and `projectPath` per active project.
The bundle's skills and other data-only extension components are not made project-scoped by this
field; they remain user-only and are managed through Bundle Tools.

A process runtime declares at least one sibling `commands` entry. UI action descriptors are
declared beside both under `extensions.dev.codetwo.ui`; they do not alter this wire protocol or load
third-party renderer code.

`extensions.dev.codetwo.languageServers` is a separate host-owned stdio LSP contribution. Language
servers use standard `Content-Length` LSP framing, not this newline-delimited plugin protocol. They
may exist without a C2 process runtime and still share bundle trust, enablement, and teardown.

## Trust

**Installing a bundle executes nothing.** That property of the Plugin Hub is not weakened by this
protocol — it is the reason the protocol is shaped this way.

An enabled and **trusted** bundle is eligible: its host adapter and dormant command stubs become
ready, but the process does not start until the first declared command invocation. Trust is a
separate, deliberate user action, and installing a bundle that ships a C2 runtime raises a
diagnostic saying so. Until then the plugin is listed by `extensions.list` under `untrusted`.

Trust is a hard gate in every realm. No configuration setting can bypass it.

## Lifecycle

```
enabled + trusted ──▶ register static stubs ──▶ ready, process dormant
                                                    │ first command
                                                    ▼
                                               spawn + initialize
                                                    │
                              exact command/schema confirmation ──▶ invoke
                                                    │
                             timeout / mismatch ──▶ process killed; activation terminal
                                  caller cancellation ──▶ process killed; next invocation may retry

unload, dependency lost, bundle disabled/removed ──▶ process killed; stubs removed
```

Unloading is exact: the process is killed and every command it contributed is gone. Nothing has to
remember to clean up, because the plugin's scope owns all of it.

Installed runtimes are ordinary dynamically registered factories named `bundle:<id>`. Installing,
removing, enabling, trusting, or replacing a bundle reconciles that factory set and every live
eligible realm immediately. Add and remove do not require an application restart; replacing a
manifest or installed record rebuilds the same-named runtime because its factory revision changed.
User and project state changes go through `plugins.catalog`, `plugins.plan_change`, and
`plugins.apply_change`, including stale-plan protection and immediate unload.

A complete, runnable example lives in [`packs/hello-runtime/`](../../packs/hello-runtime): it
contributes a command, calls `git.status` back through the host from inside it, feature-detects the
extension-public surface, and listens for a host event — in one file with no dependencies.

## A minimal plugin

```js
#!/usr/bin/env node
const readline = require("readline");
const rl = readline.createInterface({ input: process.stdin });
const send = (m) => process.stdout.write(JSON.stringify(m) + "\n");

rl.on("line", async (line) => {
  const msg = JSON.parse(line);
  if (msg.method === "initialize") {
    send({ jsonrpc: "2.0", id: msg.id, result: {
      name: "hello", version: "1.0.0", protocolVersion: "1.0.0",
      commands: [{ name: "hello.status", description: "Repo status, via the host." }],
      events: ["skills/changed"],
    }});
  } else if (msg.method === "command/invoke") {
    // Call a host command and answer with what it said.
    send({ jsonrpc: "2.0", id: 1000, method: "command/call",
           params: { name: "git.status", args: { cwd: msg.params.args.cwd } } });
    // (a real plugin would correlate the response by id)
  } else if (msg.method === "event/emit") {
    console.error(`host event: ${msg.params.name}`);   // stderr is the log channel
  }
});
```

## Limits, stated plainly

- **No sandbox.** A trusted plugin is a process with your user's permissions. Trust is the whole
  OS security boundary; project command/data/process isolation does not restrict filesystem,
  network, environment, or the host-wide JSON event bus. Treat it as such.
- **Process-wide cancellation.** A cancelled or timed-out sent command terminates concurrent work
  in that plugin realm. Other plugin processes and project realms are unaffected.
- **One process per activated managed realm.** A project-capable runtime may have one global process
  and separate processes for multiple live projects. Fan out inside one realm if you need more.
- **Rust plugins are still compile-time.** This protocol is how a plugin gets added without
  rebuilding C2; it is not dynamic loading of native code, and deliberately so.
