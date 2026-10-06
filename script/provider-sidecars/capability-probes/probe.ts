#!/usr/bin/env bun
/**
 * Native provider connector capability probe (nonpaid, no auth, no model turn).
 *
 * Reads PINNED official artifacts (npm tarballs, sdk-bridge raw proto/docs at a tag, local
 * `codex app-server generate-json-schema`, local CLI --version/--help) and verifies each claim
 * with a regex/JSON predicate against those bytes. A claim's `level` says how strong the proof is:
 *   package-types | protocol-proto | schema | cli-help | bundled-source-read (minified JS read, NOT executed).
 * None of these is runtime end-to-end compatibility proof; nothing here sends a prompt or key.
 *
 * Usage: bun script/provider-sidecars/capability-probes/probe.ts [--clean]
 * Writes report.json + report.md next to this file. Exit 1 if any evidence no longer matches.
 */
import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const HERE = import.meta.dir;
const argv = process.argv.slice(2);
if (argv.some((value) => value !== "--clean")) {
  throw new Error("Usage: probe.ts [--clean]; scratch is restricted to this task-owned directory");
}
const REPO = resolve(HERE, "../../..");
const SCRATCH = join(REPO, ".codex/run/native-provider-connectors/capability-probes");
// Cleanup must never follow a scratch override or a symlink into someone else's files.
for (const suffix of [".codex", ".codex/run", ".codex/run/native-provider-connectors", ".codex/run/native-provider-connectors/capability-probes"]) {
  const path = join(REPO, suffix);
  if (existsSync(path) && lstatSync(path).isSymbolicLink()) {
    throw new Error(`Refusing symlink scratch path: ${path}`);
  }
}

// ---- pinned inputs --------------------------------------------------------------------------
const PKGS = {
  cursor: "@cursor/sdk@1.0.36",
  claude: "@anthropic-ai/claude-agent-sdk@0.3.290",
  oc1: "@opencode-ai/sdk@1.18.34",
  oc2sdk: "@opencode/sdk@2.0.23",
  oc2client: "@opencode/client@2.0.23",
  oc2schema: "@opencode/schema@2.0.23",
} as const;
const BRIDGE_TAG = "v1.0.36";
const BRIDGE_FILES = [
  "proto/manifest.json",
  "proto/sdk/v1/sdk_agent_service.proto",
  "proto/sdk/v1/sdk_bridge_control_service.proto",
  "proto/sdk/v1/sdk_cursor_service.proto",
  "proto/sdk/v1/sdk_custom_tool_callback_service.proto",
  "proto/sdk/v1/sdk_messages.proto",
  "docs/services.md",
  "docs/streaming.md",
];
const CLIS = { codex: ["--version"], claude: ["--version"], opencode: ["--version"], "cursor-agent": ["--version"] } as const;

type Root = keyof typeof PKGS | "bridge" | "codex" | "cli";
const sha = (b: Uint8Array | string, alg = "sha256", enc: "hex" | "base64" = "hex") => createHash(alg).update(b).digest(enc);
const pkgDir = (k: keyof typeof PKGS) => join(SCRATCH, "pkg", k, "package");
const rootDir = (r: Root) => (r === "bridge" ? join(SCRATCH, "bridge") : r === "codex" ? join(SCRATCH, "codex-schema") : r === "cli" ? join(SCRATCH, "cli") : pkgDir(r));

// Do not forward auth environment variables. HOME still permits CLI configuration reads;
// only version/help/schema commands run. npm uses empty user/global config files.
const cleanEnv = { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "", TMPDIR: process.env.TMPDIR ?? "/tmp", LANG: "C" };
const run = (cmd: string[], cwd?: string) => {
  const r = Bun.spawnSync(cmd, { cwd, env: cleanEnv, stdout: "pipe", stderr: "pipe" });
  if (r.exitCode !== 0) throw new Error(`${cmd.join(" ")} -> ${r.exitCode}: ${r.stderr.toString().slice(0, 300)}`);
  return r.stdout.toString();
};

// ---- fetch / extract (idempotent) -----------------------------------------------------------
const artifacts: Record<string, unknown>[] = [];
mkdirSync(SCRATCH, { recursive: true });
if (argv.includes("--clean")) {
  process.once("exit", () => rmSync(SCRATCH, { recursive: true, force: true }));
}
const [userRc, globalRc] = ["user", "global"].map((n) => join(SCRATCH, `empty-${n}.npmrc`));
writeFileSync(userRc, "");
writeFileSync(globalRc, "");
const npmFlags = ["--cache", join(SCRATCH, "npm-cache"), "--userconfig", userRc, "--globalconfig", globalRc];

for (const [k, spec] of Object.entries(PKGS) as [keyof typeof PKGS, string][]) {
  const dir = join(SCRATCH, "pkg", k);
  const registryIntegrity = run(["npm", "view", spec, "dist.integrity", ...npmFlags]).trim().split("\n").pop()!;
  if (!existsSync(join(dir, "package", "package.json"))) {
    mkdirSync(dir, { recursive: true });
    const tgz = run(["npm", "pack", spec, "--silent", ...npmFlags], dir).trim().split("\n").pop()!;
    run(["tar", "xzf", tgz], dir);
  }
  const tgzFiles = [...new Bun.Glob("*.tgz").scanSync({ cwd: dir, onlyFiles: true })];
  if (tgzFiles.length !== 1) throw new Error(`${spec}: expected exactly one tarball`);
  const tgzFile = tgzFiles[0]!;
  const integrity = `sha512-${sha(readFileSync(join(dir, tgzFile)), "sha512", "base64")}`;
  if (integrity !== registryIntegrity) throw new Error(`${spec}: tarball integrity ${integrity} != registry ${registryIntegrity}`);
  artifacts.push({ kind: "npm", spec, integrity, verifiedAgainstRegistry: true });
}

for (const f of BRIDGE_FILES) {
  const p = join(SCRATCH, "bridge", f);
  if (!existsSync(p)) {
    mkdirSync(dirname(p), { recursive: true });
    const res = await fetch(`https://raw.githubusercontent.com/cursor/sdk-bridge/${BRIDGE_TAG}/${f}`, { signal: AbortSignal.timeout(30_000) });
    if (!res.ok) throw new Error(`bridge ${f}: HTTP ${res.status}`);
    writeFileSync(p, new Uint8Array(await res.arrayBuffer()));
  }
  artifacts.push({ kind: "github-raw", repo: "cursor/sdk-bridge", tag: BRIDGE_TAG, file: f, sha256: sha(readFileSync(p)) });
}

const cliOut: Record<string, string> = {};
for (const [bin, flags] of Object.entries(CLIS)) {
  const w = Bun.which(bin);
  cliOut[bin] = w ? run([bin, ...flags]).trim().split("\n")[0] : "(not installed)";
}
mkdirSync(rootDir("cli"), { recursive: true });
writeFileSync(join(rootDir("cli"), "opencode-help.txt"), run(["opencode", "--help"])); // help text only; starts nothing
const codexBin = Bun.which("codex");
if (codexBin) {
  rmSync(rootDir("codex"), { recursive: true, force: true });
  run(["codex", "app-server", "generate-json-schema", "--out", rootDir("codex")]);
  artifacts.push({ kind: "local-cli-schema", cli: cliOut.codex, bin: codexBin, schemaSha256: sha(readFileSync(join(rootDir("codex"), "codex_app_server_protocol.v2.schemas.json"))) });
}

// ---- evidence engine ------------------------------------------------------------------------
type Ev =
  | { root: Root; file: string; has: RegExp; why?: string } //   pattern must match in first matching file
  | { root: Root; file: string; absent: RegExp; why?: string } // pattern must match nowhere in glob
  | { root: Root; file: string; json: (d: any) => unknown; label: string }; // predicate on parsed JSON
type Level = "package-types" | "protocol-proto" | "schema" | "cli-help" | "bundled-source-read" | "mixed";
type Verdict = "supported" | "partial" | "unsupported" | "unverified";
type Check = { id: string; provider: string; topic: string; claim: string; verdict: Verdict; level: Level; ev: Ev[]; note?: string };

const globFiles = (root: Root, pat: string) => [...new Bun.Glob(pat).scanSync({ cwd: rootDir(root), onlyFiles: true })].sort();
const excerpt = (t: string, i: number, len: number) => t.slice(Math.max(0, i - 40), i + Math.min(len, 220)).replace(/\s+/g, " ").trim();

function evaluate(e: Ev) {
  const files = globFiles(e.root, e.file);
  if (!files.length) return { ok: false, detail: `no file matches ${e.root}:${e.file}` };
  if ("json" in e) {
    const v = e.json(JSON.parse(readFileSync(join(rootDir(e.root), files[0]), "utf8")));
    return { ok: !!v, file: files[0], detail: `${e.label} => ${JSON.stringify(v)?.slice(0, 260)}` };
  }
  if ("has" in e) {
    for (const f of files) {
      const t = readFileSync(join(rootDir(e.root), f), "utf8");
      const m = e.has.exec(t);
      if (m) return { ok: true, file: f, line: t.slice(0, m.index).split("\n").length, detail: excerpt(t, m.index, m[0].length) };
    }
    return { ok: false, detail: `no match for ${e.has} in ${files.length} file(s)` };
  }
  for (const f of files) {
    const t = readFileSync(join(rootDir(e.root), f), "utf8");
    const m = e.absent.exec(t);
    if (m) return { ok: false, file: f, detail: `UNEXPECTED match: ${excerpt(t, m.index, m[0].length)}` };
  }
  return { ok: true, detail: `absent in ${files.length} file(s) (${e.file})` };
}

// ---- claims ---------------------------------------------------------------------------------
const CURSOR_APPROVAL_API = /\b(canUseTool|onApproval|onToolApproval|requestApproval|onPermission|permissionHandler|approvalCallback|respondApproval)\b/;
const NO_STEER_WORD = /steer/i;
const CHECKS: Check[] = [
  // ===== Cursor (@cursor/sdk 1.0.36 + sdk-bridge v1.0.36) =====
  {
    id: "cursor.approval.no-host-reply-channel", provider: "cursor", topic: "approval pause/reply/deny", verdict: "unsupported", level: "mixed",
    claim: "No public SDK type or Bridge RPC lets the host pause, approve or deny a tool call. Approval is auto-approve (default) or auto-deny (sandbox/autoReview).",
    ev: [
      { root: "cursor", file: "dist/esm/**/*.d.ts", absent: CURSOR_APPROVAL_API },
      { root: "bridge", file: "proto/sdk/v1/*.proto", absent: /approv|rpc \w*Permission/i },
      { root: "cursor", file: "dist/esm/867.js", has: /class \w+\{async requestApproval\(\w\)\{return\{approved:!0\}\}\}/, why: "default decision store approves everything" },
      { root: "cursor", file: "dist/esm/867.js", has: /Local SDK runs cannot request interactive approval/, why: "sandbox/autoReview store denies instead of asking" },
      { root: "cursor", file: "dist/esm/867.js", has: /pendingDecisionProvider:\w+\|\|!0===\w+\.autoReview\?new \w+:void 0/ },
      { root: "cursor", file: "dist/esm/867.js", has: /hookForcesPrompt:\w+\(\w+\.hookApprovalRequirement|hookApprovalRequirement:\w+\(\w+\)\}/, why: "hook-forced prompts flow into the same store (source-read, not executed)" },
    ],
    note: "Hooks look like the same path (hook-forced prompts go to the same decision store; read, not executed), not a reply channel. approval-required mode must be reported UNAVAILABLE for Cursor; never silently run full-access or switch to ACP.",
  },
  {
    id: "cursor.approval.request-event-is-observation", provider: "cursor", topic: "approval pause/reply/deny", verdict: "unsupported", level: "package-types",
    claim: "The only `request` stream message carries agent/run/request IDs and nothing else, so it cannot represent a tool approval.",
    ev: [{ root: "cursor", file: "dist/esm/messages.d.ts", has: /interface SDKRequestMessage \{\s*type: "request";\s*agent_id: string;\s*run_id: string;\s*request_id: string;\s*\}/ }],
  },
  {
    id: "cursor.customtools.bypass-approval", provider: "cursor", topic: "tool restriction", verdict: "partial", level: "package-types",
    claim: "Custom (host callback) tools never require interactive approval; the host callback is the only gate, so the host Tool Broker must authorize by C2 session.",
    ev: [{ root: "cursor", file: "dist/esm/options.d.ts", has: /they never require interactive approval/ }],
  },
  {
    id: "cursor.tools.allow-deny-options", provider: "cursor", topic: "tool restriction", verdict: "partial", level: "mixed",
    claim: "`tools` (allowlist, [] = none) and `disallowedTools` (deny wins) exist in SDK and Bridge (AgentOptions.tools/disallowed_tools), local agents only. They are sent as x-cursor-agent-allowed-tools / x-cursor-agent-exclude-tools headers and applied by the Cursor backend (server-side enforcement; NOT verified offline).",
    ev: [
      { root: "cursor", file: "dist/esm/options.d.ts", has: /tools\?: ToolName\[\];/ },
      { root: "cursor", file: "dist/esm/options.d.ts", has: /disallowedTools\?: ToolName\[\];/ },
      { root: "cursor", file: "dist/esm/tools-option.d.ts", has: /backend restricts\s+\* the session's toolset/ },
      { root: "bridge", file: "proto/sdk/v1/sdk_messages.proto", has: /ToolList tools = 10;/ },
      { root: "bridge", file: "proto/sdk/v1/sdk_messages.proto", has: /repeated string disallowed_tools = 11;/ },
    ],
    note: "Server-side enforcement can only be confirmed by a credentialed run (any send is a model turn, so it was not done here). Fail closed: no coordinator runs on Cursor until a parent-authorized check shows a disallowed tool is neither offered nor executed.",
  },
  {
    id: "cursor.tools.not-persisted-on-resume", provider: "cursor", topic: "tool restriction", verdict: "partial", level: "package-types",
    claim: "`tools`/`disallowedTools` are not persisted on the agent: they must be passed again on every Agent.create/resume or the restriction silently vanishes.",
    ev: [
      { root: "cursor", file: "dist/esm/options.d.ts", has: /Not persisted on the agent: pass `tools` again/ },
      { root: "cursor", file: "dist/esm/options.d.ts", has: /Not persisted on the agent: pass `disallowedTools` again/ },
    ],
  },
  {
    id: "cursor.tools.task-subagents-not-restricted", provider: "cursor", topic: "tool restriction", verdict: "partial", level: "package-types",
    claim: "Restriction applies only to the main agent loop; `task` subagents keep their own toolsets and, without subagentInherit, get no allowlist headers. A coordinator must omit/disallow `task`.",
    ev: [
      { root: "cursor", file: "dist/esm/options.d.ts", has: /Subagents launched through it keep their own\s+\*\s+curated toolsets/ },
      { root: "cursor", file: "dist/esm/options.d.ts", has: /behavior: host workspace path, host executors, and no tool-allowlist\s+\*\s+headers/ },
    ],
  },
  {
    id: "cursor.tools.mcp-group-couples-custom-tools", provider: "cursor", topic: "tool restriction", verdict: "partial", level: "package-types",
    claim: "Custom tools ride the `mcp` capability group: `tools: []` removes them too, while allowing `mcp` also admits configured MCP servers; settingSources/mcpServers must be pinned empty for a coordinator.",
    ev: [{ root: "cursor", file: "dist/esm/options.d.ts", has: /omitting `"mcp"`\s+\*\s+disables MCP entirely/ }],
  },
  {
    id: "cursor.tools.init-message-lists-tools", provider: "cursor", topic: "tool restriction", verdict: "partial", level: "package-types",
    claim: "The run `system` init message has optional `tools: string[]`; a host can compare it to the expected allowlist and cancel on mismatch (defence in depth, type-level only).",
    ev: [{ root: "cursor", file: "dist/esm/messages.d.ts", has: /interface SDKSystemMessage \{[^}]*tools\?: string\[\];/ }],
  },
  {
    id: "cursor.mode.plan-is-not-readonly-guarantee", provider: "cursor", topic: "tool restriction", verdict: "unsupported", level: "protocol-proto",
    claim: "`mode: plan` is documented as 'produce a plan before (or instead of) making edits', not as an enforced read-only sandbox; sandboxOptions only toggles `enabled`. Neither may be the sole coordinator guard.",
    ev: [
      { root: "bridge", file: "proto/sdk/v1/sdk_messages.proto", has: /Plan mode: produce a plan before \(or instead of\) making edits/ },
      { root: "bridge", file: "proto/sdk/v1/sdk_messages.proto", has: /message SandboxOptions \{\s*optional bool enabled = 1;\s*\}/ },
    ],
  },
  {
    id: "cursor.model.selection-is-free-form", provider: "cursor", topic: "300k/high model options", verdict: "unverified", level: "package-types",
    claim: "ModelSelection is {id, params:[{id,value}]} with free strings; valid ids/params/values (including any 300k context or high effort variant) exist only in the account catalog (Cursor.models.list / SdkCursorService.ListModels, needs an API key). Offline types cannot confirm exact 300k/high.",
    ev: [
      { root: "cursor", file: "dist/esm/options.d.ts", has: /interface ModelSelection \{\s*id: string;\s*params\?: ModelParameterValue\[\];/ },
      { root: "cursor", file: "dist/esm/options.d.ts", has: /interface ModelParameterValue \{\s*id: string;\s*value: string;/ },
      { root: "bridge", file: "proto/sdk/v1/sdk_cursor_service.proto", has: /rpc ListModels\(ListModelsRequest\)/ },
    ],
    note: "Host must read the catalog (non-turn call, needs credential), match definition id + value + variant exactly, and reject otherwise; never map to a nearby model.",
  },
  {
    id: "cursor.model.local-validation-id-only", provider: "cursor", topic: "300k/high model options", verdict: "partial", level: "bundled-source-read",
    claim: "Local `resolveLocalModelSelection` checks only id/alias against the catalog, rewrites an alias to the canonical id, passes `params` through unvalidated, and skips validation entirely when no API key is configured.",
    ev: [
      { root: "cursor", file: "dist/esm/index.js", has: /if\(void 0===r\.listModels\)return t/ },
      { root: "cursor", file: "dist/esm/index.js", has: /r\.id===t\.id\|\|!0===r\.aliases\?\.includes\(t\.id\)/ },
      { root: "cursor", file: "dist/esm/index.js", has: /return\{\.\.\.t,id:n\.id\}/ },
    ],
  },
  {
    id: "cursor.steer.ack-types", provider: "cursor", topic: "steering", verdict: "partial", level: "package-types",
    claim: "`Run.steer?(text)` is optional and resolves only complete_delivered | revert_to_followup (confirm_steering is consumed internally). RunOperation has no 'steer', so `supports()` cannot feature-detect it.",
    ev: [
      { root: "cursor", file: "dist/esm/run.d.ts", has: /steer\?\(text: string\): Promise<SteerAckOutcome>;/ },
      { root: "cursor", file: "dist/esm/run.d.ts", has: /type SteerAckOutcome = "complete_delivered" \| "revert_to_followup";/ },
      { root: "cursor", file: "dist/esm/run.d.ts", has: /type RunOperation = "stream" \| "wait" \| "cancel" \| "conversation";/ },
    ],
  },
  {
    id: "cursor.steer.bridge-has-no-rpc", provider: "cursor", topic: "steering", verdict: "unsupported", level: "protocol-proto",
    claim: "sdk.v1 Bridge protos and docs contain no steer RPC/message; steering requires the TypeScript SDK sidecar, not the Bridge.",
    ev: [
      { root: "bridge", file: "proto/sdk/v1/*.proto", absent: NO_STEER_WORD },
      { root: "bridge", file: "docs/*.md", absent: NO_STEER_WORD },
    ],
  },
  {
    id: "cursor.steer.ack-mapping", provider: "cursor", topic: "steering", verdict: "partial", level: "bundled-source-read",
    claim: "Server steer states map: delivered -> complete_delivered; queued -> keep waiting; queued_for_next_turn/cancelled/rejected/other -> revert_to_followup.",
    ev: [{ root: "cursor", file: "dist/esm/867.js", has: /case"queued":return"confirm_steering";case"delivered":return"complete_delivered";case"queued_for_next_turn":case"cancelled":case"rejected":return"revert_to_followup"/ }],
  },
  {
    id: "cursor.steer.revert-is-ambiguous", provider: "cursor", topic: "steering", verdict: "partial", level: "bundled-source-read",
    claim: "revert_to_followup is ALSO returned on a 15s ack timeout and on ANY submit exception (e.g. connection lost after the request left), so it does not prove non-delivery. Only complete_delivered is a positive receipt; revert_to_followup must not be blindly resent (duplicate risk). Empty text also returns it.",
    ev: [
      { root: "cursor", file: "dist/esm/867.js", has: /setTimeout\(\(\)=>\{\w\(\w,"revert_to_followup"\)\},15e3\)|setTimeout\(\(\(\)=>\{\w\(\w,"revert_to_followup"\)\}\),15e3\)/ },
      { root: "cursor", file: "dist/esm/867.js", has: /catch\(\w\)\{return \w\(\w,"revert_to_followup"\),\w+\.warn\("\[agent-session-runner\] context injection failed:"/ },
    ],
    note: "Design hint (unverified): before any follow-up, reconcile against run.conversation(); keep unknown on rejected promise/disconnect.",
  },
  {
    id: "cursor.steer.stub-handles-never-steer", provider: "cursor", topic: "steering", verdict: "unsupported", level: "bundled-source-read",
    claim: "Run handles rebuilt from the store / cloud / reattach return revert_to_followup unconditionally; only the live handle returned by agent.send in the owning process can really steer. Function presence is not capability.",
    ev: [{ root: "cursor", file: "dist/esm/index.js", has: /async steer\(\w\)\{return"revert_to_followup"\}/ }],
  },
  {
    id: "cursor.steer.between-turns-aborts-turn", provider: "cursor", topic: "steering", verdict: "partial", level: "bundled-source-read",
    claim: "With no injectable active turn but a live turn controller, steer aborts that turn and queues the text as the next one (interrupt-and-redirect, not an in-turn append).",
    ev: [{ root: "cursor", file: "dist/esm/867.js", has: /d\.push\(\{text:\w,resolve:\w\}\)\}\)\);return \w\.abort\(\),await \w\}/ }],
  },
  {
    id: "cursor.abort.local-cancel-is-optimistic", provider: "cursor", topic: "abort terminal", verdict: "partial", level: "bundled-source-read",
    claim: "Local Run.cancel() flips status to 'cancelled' and emits CANCELLED BEFORE awaiting the abort, then awaits only store.cancelRun. cancel()'s return or run.status is not proof the writer stopped; run.wait() (awaits controller.done) is.",
    ev: [
      { root: "cursor", file: "dist/esm/index.js", has: /this\.cancellationState\.cancelled=!0,this\.setStatus\("cancelled"\),await this\.emitStatus\("CANCELLED"\),this\.controller\.abort\(\),await this\.store\.cancelRun\(/ },
      { root: "cursor", file: "dist/esm/index.js", has: /"running"!==this\.currentStatus\)return await this\.controller\.done\.catch/ },
    ],
  },
  {
    id: "cursor.abort.bridge-terminal-is-stream-result", provider: "cursor", topic: "abort terminal", verdict: "supported", level: "protocol-proto",
    claim: "Bridge CancelRun returns an empty ack; the terminal proof is the stream's `result` (CANCELLED) then `done`. Dropping the Send stream does not cancel; a failed stream means the run may still be executing (recover with ObserveRun/WaitLiveRun/GetRun).",
    ev: [
      { root: "bridge", file: "proto/sdk/v1/sdk_agent_service.proto", has: /message CancelRunResponse \{\}/ },
      { root: "bridge", file: "docs/streaming.md", has: /terminal\s+`result` \(status `CANCELLED`\) followed by `done`/ },
      { root: "bridge", file: "docs/streaming.md", has: /dropping the `Send` stream does \*not\* cancel/ },
    ],
  },
  {
    id: "cursor.abort.stub-cancel-is-store-only", provider: "cursor", topic: "abort terminal", verdict: "unsupported", level: "bundled-source-read",
    claim: "Store-backed (reattached) Run.cancel() only marks the store and sets status 'cancelled'; it cannot prove a remote/other-process writer stopped.",
    ev: [{ root: "cursor", file: "dist/esm/index.js", has: /async cancel\(\)\{try\{if\("running"!==this\.currentStatus\)return;await this\.store\.cancelRun\(this\.agentId,this\.id\),this\.currentStatus="cancelled"\}/ }],
  },

  // ===== Claude Agent SDK 0.3.290 =====
  {
    id: "claude.approval.canUseTool", provider: "claude", topic: "approval pause/reply/deny", verdict: "supported", level: "package-types",
    claim: "Host callback `canUseTool` returns allow/deny per tool call (deny may interrupt); permissionMode 'default' keeps manual approval; `permissionPrompts:'none'` denies instead of prompting; bypassPermissions needs allowDangerouslySkipPermissions.",
    ev: [
      { root: "claude", file: "sdk.d.ts", has: /canUseTool\?: CanUseTool;/ },
      { root: "claude", file: "sdk.d.ts", has: /behavior: 'deny';\s*message: string;\s*interrupt\?: boolean;/ },
      { root: "claude", file: "sdk.d.ts", has: /permissionPrompts\?: 'host' \| 'none';/ },
      { root: "claude", file: "sdk.d.ts", has: /type PermissionMode = 'default' \| 'acceptEdits' \| 'bypassPermissions' \| 'plan' \| 'dontAsk' \| 'auto';/ },
      { root: "claude", file: "sdk.d.ts", has: /allowDangerouslySkipPermissions\?: boolean;/ },
    ],
  },
  {
    id: "claude.tools.restriction", provider: "claude", topic: "tool restriction", verdict: "supported", level: "package-types",
    claim: "`tools` (base set; [] disables all built-ins) and `disallowedTools` remove tools from the model's context; `dontAsk` denies anything not pre-approved. Enforced by the Claude Code harness, not by prompt.",
    ev: [
      { root: "claude", file: "sdk.d.ts", has: /tools\?: string\[\] \| \{\s*type: 'preset';\s*preset: 'claude_code';/ },
      { root: "claude", file: "sdk.d.ts", has: /disallowedTools\?: string\[\];/ },
      { root: "claude", file: "sdk.d.ts", has: /`\[\]` \(empty array\) - Disable all built-in tools/ },
    ],
  },
  {
    id: "claude.model.effort-and-context", provider: "claude", topic: "model options", verdict: "partial", level: "package-types",
    claim: "Effort low|medium|high|xhigh|max and supportedModels()/ModelInfo.supportedEffortLevels exist; the only context option is the 1M beta flag. There is no 300k setting.",
    ev: [
      { root: "claude", file: "sdk.d.ts", has: /type EffortLevel = 'low' \| 'medium' \| 'high' \| 'xhigh' \| 'max';/ },
      { root: "claude", file: "sdk.d.ts", has: /supportedModels\(\): Promise<ModelInfo\[\]>;/ },
      { root: "claude", file: "sdk.d.ts", has: /supportedEffortLevels\?: \('low' \| 'medium' \| 'high' \| 'xhigh' \| 'max'\)\[\];/ },
      { root: "claude", file: "sdk.d.ts", has: /type SdkBeta = 'context-1m-2025-08-07';/ },
    ],
  },
  {
    id: "claude.steer.stream-input-priority", provider: "claude", topic: "steering", verdict: "partial", level: "package-types",
    claim: "streamInput(AsyncIterable<SDKUserMessage>) with message priority now|next|later gives live input, but streamInput resolves void (no per-message delivered receipt); interrupt() may return a still_queued receipt only if the CLI advertises interrupt_receipt_v1.",
    ev: [
      { root: "claude", file: "sdk.d.ts", has: /streamInput\(stream: AsyncIterable<SDKUserMessage>\): Promise<void>;/ },
      { root: "claude", file: "sdk.d.ts", has: /priority\?: 'now' \| 'next' \| 'later';/ },
      { root: "claude", file: "sdk.d.ts", has: /interrupt\(\): Promise<SDKControlInterruptResponse \| undefined>;/ },
      { root: "claude", file: "sdk.d.ts", has: /still_queued: string\[\];/ },
    ],
  },
  {
    id: "claude.abort.terminal-is-result-message", provider: "claude", topic: "abort terminal", verdict: "partial", level: "package-types",
    claim: "interrupt() resolves with a receipt written before the interrupted turn's result; terminal proof is the SDKResultMessage (terminal_reason aborted_streaming|aborted_tools), not the interrupt promise.",
    ev: [
      { root: "claude", file: "sdk.d.ts", has: /type TerminalReason = [^;]*'aborted_streaming' \| 'aborted_tools'/ },
      { root: "claude", file: "sdk.d.ts", has: /on a clean interrupt this receipt is written before the interrupted turn result/ },
    ],
  },

  // ===== Codex app-server (local generate-json-schema) =====
  {
    id: "codex.approval.server-requests", provider: "codex", topic: "approval pause/reply/deny", verdict: "supported", level: "schema",
    claim: "Server-initiated JSON-RPC requests pause the turn for command/file/permission approval, user input and MCP elicitation; the client answers with a decision (accept/acceptForSession/decline/cancel...). requestId resolution is notified (ServerRequestResolvedNotification).",
    ev: [
      { root: "codex", file: "ServerRequest.json", label: "methods", json: (d) => { const m = d.oneOf.map((o: any) => o.properties.method.enum[0]); return ["item/commandExecution/requestApproval", "item/fileChange/requestApproval", "item/permissions/requestApproval", "item/tool/requestUserInput", "mcpServer/elicitation/request"].every((x) => m.includes(x)) && m; } },
      { root: "codex", file: "CommandExecutionRequestApprovalResponse.json", has: /"acceptForSession"/ },
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", json: (d) => d.definitions.ServerRequestResolvedNotification.required, label: "ServerRequestResolvedNotification.required" },
    ],
  },
  {
    id: "codex.tools.sandbox-and-policy", provider: "codex", topic: "tool restriction", verdict: "supported", level: "schema",
    claim: "Per-turn sandboxPolicy includes readOnly (also workspaceWrite/dangerFullAccess/externalSandbox) and approvalPolicy untrusted|on-request|never|granular, set at thread/start and turn/start.",
    ev: [
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "SandboxPolicy types", json: (d) => d.definitions.SandboxPolicy.oneOf.map((o: any) => o.properties.type.enum[0]).sort().join(",") === "dangerFullAccess,externalSandbox,readOnly,workspaceWrite" && "dangerFullAccess,externalSandbox,readOnly,workspaceWrite" },
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "AskForApproval enum", json: (d) => d.definitions.AskForApproval.oneOf[0].enum },
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "TurnStartParams has sandboxPolicy,approvalPolicy,effort", json: (d) => ["sandboxPolicy", "approvalPolicy", "effort"].every((k) => k in d.definitions.TurnStartParams.properties) },
    ],
  },
  {
    id: "codex.model.effort-no-context-window", provider: "codex", topic: "model options", verdict: "partial", level: "schema",
    claim: "model/list advertises supportedReasoningEfforts and defaultReasoningEffort; the Model schema has no context-window field, so 300k cannot be asserted from the schema.",
    ev: [
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "Model has efforts, no contextWindow", json: (d) => { const p = d.definitions.Model.properties; return "supportedReasoningEfforts" in p && "defaultReasoningEffort" in p && !Object.keys(p).some((k) => /context/i.test(k)); } },
    ],
  },
  {
    id: "codex.steer.accepted-turn-id", provider: "codex", topic: "steering", verdict: "supported", level: "schema",
    claim: "turn/steer requires expectedTurnId (precondition) and answers {turnId} = acceptance; failure is a JSON-RPC error. Acceptance is not a model-consumed receipt beyond the turn id; a lost response is unknown.",
    ev: [
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "TurnSteerParams.required", json: (d) => d.definitions.TurnSteerParams.required },
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "TurnSteerResponse.required", json: (d) => d.definitions.TurnSteerResponse.required },
    ],
  },
  {
    id: "codex.abort.interrupt-terminal-status", provider: "codex", topic: "abort terminal", verdict: "supported", level: "schema",
    claim: "turn/interrupt returns an empty response (ack only); terminal proof is turn/completed with Turn.status 'interrupted' (TurnStatus: completed|interrupted|failed|inProgress).",
    ev: [
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "TurnInterruptResponse properties", json: (d) => Object.keys(d.definitions.TurnInterruptResponse.properties ?? {}).length === 0 && "{}" },
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "TurnStatus", json: (d) => d.definitions.TurnStatus.enum },
      { root: "codex", file: "codex_app_server_protocol.v2.schemas.json", label: "TurnCompletedNotification exists", json: (d) => !!d.definitions.TurnCompletedNotification },
    ],
  },

  // ===== OpenCode V1 SDK (@opencode-ai/sdk 1.18.34; V1 HTTP server) =====
  {
    id: "opencode1.approval.permission-respond", provider: "opencode-v1", topic: "approval pause/reply/deny", verdict: "supported", level: "package-types",
    claim: "Permission requests arrive as `permission.updated` events and are answered with POST /session/{id}/permissions/{permissionID} {response: once|always|reject}; `permission.replied` confirms. Questions are NOT in the V1 root API.",
    ev: [
      { root: "oc1", file: "dist/gen/types.gen.d.ts", has: /type: "permission\.updated";/ },
      { root: "oc1", file: "dist/gen/types.gen.d.ts", has: /response: "once" \| "always" \| "reject";/ },
      { root: "oc1", file: "dist/gen/sdk.gen.js", absent: /\/question/ },
    ],
  },
  {
    id: "opencode1.steer.none", provider: "opencode-v1", topic: "steering", verdict: "unsupported", level: "package-types",
    claim: "V1 prompt/prompt_async bodies have no delivery/steer/queue control; busy-session behaviour is server-defined and unprobed. Per-prompt `tools` boolean map and `agent` exist.",
    ev: [
      { root: "oc1", file: "dist/gen/types.gen.d.ts", absent: /steer|delivery/i },
      { root: "oc1", file: "dist/gen/types.gen.d.ts", has: /noReply\?: boolean;\s*system\?: string;\s*tools\?: \{/ },
    ],
  },
  {
    id: "opencode1.abort.bool-then-idle", provider: "opencode-v1", topic: "abort terminal", verdict: "partial", level: "package-types",
    claim: "POST /session/{id}/abort answers a boolean only; terminal evidence must come from session.idle / session.status idle / session.error(MessageAbortedError) events.",
    ev: [
      { root: "oc1", file: "dist/gen/types.gen.d.ts", has: /type SessionAbortResponses = \{[^}]*200: boolean;/ },
      { root: "oc1", file: "dist/gen/types.gen.d.ts", has: /type: "session\.idle";/ },
      { root: "oc1", file: "dist/gen/types.gen.d.ts", has: /error\?: ProviderAuthError \| UnknownError \| MessageOutputLengthError \| MessageAbortedError \| ApiError;/ },
    ],
  },
  {
    id: "opencode1.v2-subpath-is-not-opencode-v2", provider: "opencode-v1", topic: "naming", verdict: "partial", level: "package-types",
    claim: "The V1 package's own `/v2` subpath (generated client with /question/{requestID}/reply, /permission/{requestID}/reply) is still the V1-line HTTP API (version 1.18.34), NOT the OpenCode V2 SDK; keep it out of the OpenCode2 provider.",
    ev: [
      { root: "oc1", file: "package.json", has: /"\.\/v2": \{/ },
      { root: "oc1", file: "dist/v2/gen/sdk.gen.js", has: /url: "\/question\/\{requestID\}\/reply"/ },
    ],
  },

  // ===== OpenCode V2 (@opencode/sdk, @opencode/client, @opencode/schema 2.0.23) =====
  {
    id: "opencode2.host.embedded", provider: "opencode-v2", topic: "transport", verdict: "partial", level: "package-types",
    claim: "@opencode/sdk create() builds an EMBEDDED host (no HTTP listener assumed); its Interface is the @opencode/client API plus close/asyncDispose. Different package family from @opencode-ai/sdk.",
    ev: [
      { root: "oc2sdk", file: "dist/promise.d.ts", has: /interface CreateOptions extends Omit<EmbeddedHost\.CreateOptions/ },
      { root: "oc2sdk", file: "dist/promise.d.ts", has: /type Interface = Omit<OpenCodeClient, "plugin">/ },
    ],
  },
  {
    id: "opencode2.approval.permission-reply-and-forms", provider: "opencode-v2", topic: "approval pause/reply/deny", verdict: "supported", level: "package-types",
    claim: "permission.reply({sessionID, requestID, decision: once|always|reject, message?}) and permission.request.list; question/elicitation is the session.form.{list,create,get,reply,cancel} API.",
    ev: [
      { root: "oc2client", file: "dist/effect/api/api.d.ts", has: /type PermissionReplyInput = \{\s*readonly sessionID: Session\.ID;\s*readonly requestID: Permission\.ID;\s*readonly decision: Permission\.Reply;\s*readonly message\?: string/ },
      { root: "oc2schema", file: "dist/permission.d.ts", has: /Reply: Schema\.Literals<readonly \["once", "always", "reject"\]>/ },
      { root: "oc2client", file: "dist/effect/generated/client.d.ts", has: /reply: \(input: SessionFormReplyInput\)/ },
      { root: "oc2client", file: "dist/effect/generated/client.d.ts", has: /cancel: \(input: SessionFormCancelInput\)/ },
    ],
  },
  {
    id: "opencode2.steer.delivery-and-inbox", provider: "opencode-v2", topic: "steering", verdict: "partial", level: "package-types",
    claim: "session.prompt takes `delivery: steer|queue` and returns the SessionInbox.User entry; inbox.list/cancel/update(delivery) allow reconciling or reverting queued input. Whether 'steer' is applied mid-turn is runtime behaviour, unprobed.",
    ev: [
      { root: "oc2client", file: "dist/effect/api/api.d.ts", has: /readonly delivery\?: SessionInbox\.Delivery \| undefined;\s*readonly resume\?: boolean/ },
      { root: "oc2schema", file: "dist/session-inbox.d.ts", has: /Delivery: Schema\.Literals<readonly \["steer", "queue"\]>/ },
      { root: "oc2client", file: "dist/effect/api/api.d.ts", has: /type SessionInboxCancelInput = \{\s*readonly sessionID: Session\.ID;\s*readonly inboxID/ },
      { root: "oc2client", file: "dist/effect/api/api.d.ts", has: /type SessionInboxUpdateInput = \{\s*readonly sessionID: Session\.ID;\s*readonly inboxID: SessionMessage\.ID;\s*readonly delivery: SessionInbox\.Delivery/ },
    ],
  },
  {
    id: "opencode2.abort.interrupt-boolean", provider: "opencode-v2", topic: "abort terminal", verdict: "partial", level: "package-types",
    claim: "session.interrupt returns only {interrupted: boolean}; session.wait / session.log events are the way to observe the terminal state, which still needs a runtime probe.",
    ev: [
      { root: "oc2client", file: "dist/effect/api/api.d.ts", has: /type SessionInterruptOutput = \{\s*readonly interrupted: boolean;/ },
      { root: "oc2client", file: "dist/effect/generated/client.d.ts", has: /wait: \(input: SessionWaitInput\)/ },
    ],
  },
  {
    id: "opencode2.installed-cli-is-v2", provider: "opencode-v2", topic: "installed contract", verdict: "partial", level: "cli-help",
    claim: "Installed `opencode` is 2.0.23 and its `serve` is 'the v2 API and web server' with a background `service`; the installed binary is not evidence for the V1 HTTP server contract.",
    ev: [
      { root: "cli", file: "opencode-help.txt", has: /serve\s+Start the v2 API and web server/ },
      { root: "cli", file: "opencode-help.txt", has: /service\s+Manage the background server/ },
    ],
  },
];

const NOT_PROBED = [
  "Any real send/steer/cancel/approval against a live backend (needs credentials and model turns).",
  "Cursor model catalog (Cursor.models.list / ListModels) and therefore exact 300k/high ids, params and values.",
  "Server-side enforcement of Cursor tools/disallowedTools headers.",
  "Runtime import of @cursor/sdk (needs its dependency tree + native optional package; an auto-install Bun import attempt resolved wrong @bufbuild/protobuf and proves nothing).",
  "OpenCode V1 HTTP server behaviour (installed CLI is V2) and OpenCode V2 runtime semantics of delivery:'steer'.",
  "Claude SDK runtime against the installed Claude Code CLI 2.1.288 and its advertised capabilities.",
];
const IMPLICATIONS = [
  "Cursor approval-required: UNAVAILABLE (SDK/Bridge expose no reply channel; default is auto-approve, sandbox/autoReview is auto-deny). Report the mode unavailable; never switch to full-access, ACP or another provider silently.",
  "Cursor coordinator: only allowed with tools/disallowedTools passed on EVERY create/resume (not persisted), `task` disallowed, settingSources/mcpServers pinned, custom tools gated by the host Tool Broker per C2 session, init-message `tools` compared to the allowlist, and one credentialed enforcement check; otherwise fail closed. `mode: plan` and sandbox are not guards.",
  "Cursor model: send only an id+params pair copied verbatim from the account catalog; the SDK checks only the id (rewrites aliases) and passes params unvalidated. Offline types cannot prove a 300k/high variant exists.",
  "Cursor steering needs the TypeScript SDK sidecar (Bridge has no steer RPC), only on the live Run handle. complete_delivered = delivered. revert_to_followup also means 15s ack timeout or submit failure, so treat it as 'not confirmed' and reconcile before any follow-up; a rejected promise/disconnect stays unknown.",
  "Cursor stop: terminal proof is `await run.wait()` (SDK) or stream `result: CANCELLED` + `done` (Bridge), never cancel()'s return or run.status; a reattached store-only handle cannot prove the writer stopped.",
  "Claude/Codex/OpenCode-V1/V2 have first-class approval replies; their steering/stop receipts differ (Codex turn/steer->turnId, Claude streamInput->void, OpenCode V2 prompt->inbox entry with delivery steer|queue) so map each to accepted/unknown separately, never to a shared 'delivered'.",
  "OpenCode V1 SDK (@opencode-ai/sdk, incl. its own /v2 subpath) and OpenCode V2 (@opencode/sdk, embedded host) are different provider identities; the installed CLI is V2.",
];

// ---- run + report ---------------------------------------------------------------------------
const results = CHECKS.map((c) => {
  const ev = c.ev.map((e) => ({ root: e.root, file: e.file, kind: "has" in e ? "has" : "absent" in e ? "absent" : "json", ...evaluate(e) }));
  return { id: c.id, provider: c.provider, topic: c.topic, verdict: c.verdict, level: c.level, claim: c.claim, note: c.note, evidenceOk: ev.every((x) => x.ok), evidence: ev };
});

const report = {
  schema: "native-provider-capability-probe/1",
  generatedAt: new Date().toISOString(),
  honesty: "Package type signatures, protos, generated schemas and minified-source reading are static evidence. They are NOT runtime end-to-end compatibility proof; no model turn, auth, key or paid request was made. Verdict 'supported' means the official surface exists, not that it was exercised.",
  environment: {
    os: `${process.platform}-${process.arch}`, bun: Bun.version, node: Bun.which("node") ? run(["node", "--version"]).trim() : null,
    clis: cliOut, scriptSha256: sha(readFileSync(import.meta.path)),
    subprocessEnvKeys: Object.keys(cleanEnv), npmUserconfig: "empty",
  },
  artifacts,
  summary: Object.fromEntries(
    ["approval pause/reply/deny", "tool restriction", "model options|300k/high model options", "steering", "abort terminal"].map((t) => [
      t,
      Object.fromEntries(
        [...new Set(results.map((r) => r.provider))].map((p) => [p, results.filter((r) => r.provider === p && t.split("|").includes(r.topic)).map((r) => `${r.verdict}:${r.id}`)]),
      ),
    ]),
  ),
  notProbed: NOT_PROBED,
  implications: IMPLICATIONS,
  checks: results,
};
writeFileSync(join(HERE, "report.json"), JSON.stringify(report, null, 1) + "\n");

const md = [
  "# Native provider capability probe", "",
  `Generated ${report.generatedAt} by \`probe.ts\`. ${report.honesty}`, "",
  "| Provider | Topic | Verdict | Level | Evidence | Claim |", "| --- | --- | --- | --- | --- | --- |",
  ...results.map((r) => `| ${r.provider} | ${r.topic} | ${r.verdict} | ${r.level} | ${r.evidenceOk ? "matched" : "**MISMATCH**"} | \`${r.id}\`: ${r.claim.replace(/\|/g, "\\|")}${r.note ? ` _Note: ${r.note.replace(/\|/g, "\\|")}_` : ""} |`),
  "", "## Implications for the implementer", ...IMPLICATIONS.map((i) => `- ${i}`),
  "", "## Not probed", ...NOT_PROBED.map((i) => `- ${i}`),
  "", "## Artifacts", ...artifacts.map((a) => `- \`${JSON.stringify(a)}\``), "",
].join("\n");
writeFileSync(join(HERE, "report.md"), md);

const bad = results.filter((r) => !r.evidenceOk);
console.log(`checks=${results.length} evidenceMatched=${results.length - bad.length}`);
for (const r of bad) console.log(`MISMATCH ${r.id}: ${r.evidence.filter((e) => !e.ok).map((e) => e.detail).join(" | ")}`);
process.exit(bad.length ? 1 : 0);
