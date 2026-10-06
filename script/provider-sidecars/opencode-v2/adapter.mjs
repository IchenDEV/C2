// OpenCode V2 adapter (`@opencode/sdk`, embedded in-process host).
//
// V2 is a different provider identity and API family from OpenCode V1 (`@opencode-ai/sdk`).
//
// Limits taken from ../capability-probes/report.md and the 2.0.23 types:
//  - Resume needs a persistent database (the default is in memory): the host passes
//    CODETWO_OPENCODE_V2_DB; without it `resume` is not advertised.
//  - Sessions are created with an ask-everything permission ruleset so the host's policy answers
//    every request; a restored session without that ruleset is refused (fail closed).
//  - Prompts carry `delivery: steer|queue`. `session.inbox.delivered` is the only positive steering
//    receipt; a missing/late one is "unconfirmed" and never resent.
//  - `interrupt` returns only {interrupted}; the turn ends on `session.execution.*` events.
//
// The wildcard permission rule and event ordering are not exercised against a real host by the
// fixture tests; the real-backend acceptance must confirm `permission.asked` for edit/bash.

import { SidecarError } from "../common/ipc.mjs";
import { clip } from "../common/runtime.mjs";
import { PERMISSION_OPTIONS, deferred, splitContent, tail, textContent, truncate } from "../common/content.mjs";

const ASK_ALL = Object.freeze({ action: "*", resource: "*", effect: "ask" });
const STEER_RECEIPT_TIMEOUT_MS = 5_000;

export function toolKind(name) {
  const n = String(name ?? "").toLowerCase();
  if (n === "bash" || n === "shell") return "execute";
  if (n === "read") return "read";
  if (["edit", "write", "patch", "apply_patch", "multiedit"].includes(n)) return "edit";
  if (["glob", "grep", "list", "ls", "search"].includes(n)) return "search";
  if (["webfetch", "websearch"].includes(n)) return "fetch";
  if (n === "todowrite" || n === "todoread") return "think";
  return "other";
}

export function permissionKind(action) {
  return toolKind(action);
}

function contentText(content) {
  return (content ?? []).map((c) => (c.type === "text" ? c.text : `[${c.type}]`)).join("\n");
}

export function formToSchema(fields) {
  const properties = {};
  const required = [];
  for (const field of fields ?? []) {
    if (field.type === "external" || field.hidden) continue;
    const base = { title: field.title || field.key, description: field.description ?? "" };
    const options = (field.options ?? []).map((o) => ({ const: o.value, title: o.label ?? o.value, description: o.description ?? "" }));
    if (field.type === "string") properties[field.key] = { ...base, type: "string", ...(options.length ? { oneOf: options } : {}) };
    else if (field.type === "number") properties[field.key] = { ...base, type: "number" };
    else if (field.type === "integer") properties[field.key] = { ...base, type: "integer" };
    else if (field.type === "boolean") properties[field.key] = { ...base, type: "boolean" };
    else if (field.type === "multiselect") properties[field.key] = { ...base, type: "array", items: { anyOf: options } };
    else continue;
    if (field.required) required.push(field.key);
  }
  return { type: "object", properties, required };
}

function mcpConfig(server) {
  if (server.type === "stdio") {
    return { type: "local", command: [server.command, ...(server.args ?? [])], environment: server.env ?? {}, ...(server.cwd ? { cwd: server.cwd } : {}) };
  }
  if (server.type === "http" || server.type === "sse") return { type: "remote", url: server.url, headers: server.headers ?? {} };
  throw new SidecarError(`unsupported MCP transport: ${server.type}`, { code: -32602, phase: "pre_dispatch" });
}

function provenRefusal(error) {
  // Only an explicit 4xx status proves the host refused before admitting the prompt.
  return error?.name === "ClientError" && error.reason === "UnexpectedStatus" && /\b4\d\d\b/.test(String(error.message));
}

function hasAskAll(rules) {
  return Array.isArray(rules) && rules.some((r) => r.action === ASK_ALL.action && r.resource === ASK_ALL.resource && r.effect === "ask");
}

export function createOpenCodeV2Adapter({ sdk, host, env = process.env, sdkVersion = "unknown" }) {
  const sessions = new Map();
  let opencode = null;
  let events = null;
  const persistent = Boolean(env.CODETWO_OPENCODE_V2_DB);

  async function ensureHost() {
    if (opencode) return opencode;
    try {
      opencode = await sdk.OpenCode.create(persistent ? { database: { path: env.CODETWO_OPENCODE_V2_DB } } : {});
    } catch (error) {
      throw new SidecarError(`could not start the OpenCode V2 host: ${error.message}`, { phase: "pre_dispatch" });
    }
    pumpEvents();
    return opencode;
  }

  async function pumpEvents() {
    const iterable = opencode.events.subscribe();
    events = iterable;
    try {
      for await (const event of iterable) {
        try {
          onEvent(event);
        } catch (error) {
          console.error("opencode-v2 event handling failed:", error);
        }
      }
      lost("event stream ended");
    } catch (error) {
      lost(error?.message ?? "event stream failed");
    }
  }

  function lost(reason) {
    for (const s of sessions.values()) {
      s.streamEnded = reason;
      const turn = s.turn;
      s.turn = null;
      turn?.reject(new SidecarError(`OpenCode event stream lost mid-turn: ${reason}`, { phase: "unknown" }));
    }
  }

  function choicesFrom(models) {
    const choices = [];
    const limits = new Map();
    for (const m of models ?? []) {
      if (m.enabled === false || m.status === "deprecated") continue;
      const base = `${m.providerID}/${m.modelID}`;
      choices.push({ id: base, name: `${m.providerID}: ${m.name}` });
      limits.set(base, m.limit?.context);
      for (const variant of m.variants ?? []) {
        const id = `${base}#${variant.id}`;
        choices.push({ id, name: `${m.providerID}: ${m.name} (${variant.id})` });
        limits.set(id, m.limit?.context);
      }
    }
    return { choices, limits };
  }

  async function open({ resumeId, cwd, mcpServers }) {
    const oc = await ensureHost();
    const s = {
      id: "",
      cwd,
      turn: null,
      inboxId: null,
      activity: false,
      tools: new Map(),
      steer: new Map(), // inboxID -> deferred receipt
      inboxResult: new Map(), // inboxID -> delivered|declined (events can precede the prompt reply)
      choices: [],
      limits: new Map(),
      model: undefined,
      streamEnded: undefined,
    };
    try {
      for (const server of mcpServers ?? []) {
        await oc.mcp.add({ server: server.name, location: { directory: cwd }, config: mcpConfig(server) });
      }
      let info;
      if (resumeId) {
        info = await oc.sessions.get({ sessionID: resumeId });
        if (!hasAskAll(info.permissions)) {
          throw new SidecarError("this OpenCode V2 session was not created with host-mediated permissions", { phase: "pre_dispatch" });
        }
      } else {
        info = await oc.sessions.create({ location: { directory: cwd }, permissions: [ASK_ALL] });
      }
      s.id = info.id;
      if (info.model) s.model = info.model.variant ? `${info.model.providerID}/${info.model.id}#${info.model.variant}` : `${info.model.providerID}/${info.model.id}`;
    } catch (error) {
      if (error instanceof SidecarError) throw error;
      throw new SidecarError(`OpenCode V2 could not ${resumeId ? "resume" : "create"} the session: ${error.message}`, { phase: "pre_dispatch" });
    }
    try {
      const listed = await oc.model.list({ location: { directory: cwd } });
      const { choices, limits } = choicesFrom(listed.data);
      s.choices = choices;
      s.limits = limits;
      if (!s.model || !choices.some((c) => c.id === s.model)) s.model = choices[0]?.id ?? s.model;
    } catch {
      // No catalog: model switching is unavailable but the session works.
    }
    sessions.set(s.id, s);
    return {
      sessionId: s.id,
      models: s.choices.length ? { available: s.choices, current: s.model ?? "" } : undefined,
      configOptions: [],
    };
  }

  function finish(s, outcome) {
    const turn = s.turn;
    if (!turn) return;
    s.turn = null;
    turn.resolve(outcome);
  }

  async function onPermission(s, request) {
    const reply = await host.permission(s.id, {
      toolCall: {
        toolCallId: request.source?.id || request.id,
        title: truncate(request.message || `${request.action} ${request.resources?.join(", ") ?? ""}`.trim()),
        kind: permissionKind(request.action),
        rawInput: { action: request.action, resources: request.resources, metadata: request.metadata },
        status: "pending",
      },
      options: [PERMISSION_OPTIONS.allowOnce, PERMISSION_OPTIONS.allowAlways, PERMISSION_OPTIONS.rejectOnce],
    });
    const decision =
      reply.outcome === "selected" && reply.optionId === "allow"
        ? "once"
        : reply.outcome === "selected" && reply.optionId === "allow_always"
          ? "always"
          : "reject";
    try {
      await opencode.permission.reply({ sessionID: s.id, requestID: request.id, decision });
    } catch (error) {
      console.error("opencode-v2 permission reply failed:", error.message);
    }
  }

  async function onForm(s, form) {
    const reply = await host.question(s.id, { message: form.title, schema: formToSchema(form.fields) });
    try {
      if (reply.action === "accept") {
        await opencode.sessions.form.reply({ sessionID: s.id, formID: form.id, answer: reply.content ?? {} });
      } else {
        await opencode.sessions.form.cancel({ sessionID: s.id, formID: form.id });
      }
    } catch (error) {
      console.error("opencode-v2 form reply failed:", error.message);
    }
  }

  function onEvent(event) {
    const d = event.data ?? {};
    const s = sessions.get(d.sessionID ?? d.form?.sessionID);
    if (!s) return;
    switch (event.type) {
      case "session.execution.started":
        s.activity = true;
        return;
      case "session.inbox.delivered":
        s.activity = true;
        s.inboxResult.set(d.inboxID, "delivered");
        s.steer.get(d.inboxID)?.resolve("delivered");
        return;
      case "session.inbox.cancelled":
        s.inboxResult.set(d.inboxID, "declined");
        s.steer.get(d.inboxID)?.resolve("declined");
        return;
      case "session.text.delta":
        s.activity = true;
        if (d.delta) host.event(s.id, { type: "agent_text", text: clip(d.delta) });
        return;
      case "session.reasoning.delta":
        s.activity = true;
        if (d.delta) host.event(s.id, { type: "agent_thought", text: clip(d.delta) });
        return;
      case "session.tool.input.started":
        s.tools.set(d.id, d.name);
        return;
      case "session.tool.called": {
        const name = s.tools.get(d.id) ?? "tool";
        host.event(s.id, { type: "tool_call", id: d.id, title: truncate(`${name}`), kind: toolKind(name), status: "in_progress", rawInput: d.input });
        return;
      }
      case "session.tool.success":
        host.event(s.id, { type: "tool_update", id: d.id, kind: toolKind(s.tools.get(d.id)), status: "completed", content: textContent(tail(contentText(d.content))) });
        return;
      case "session.tool.failed":
        host.event(s.id, { type: "tool_update", id: d.id, kind: toolKind(s.tools.get(d.id)), status: "failed", content: textContent(d.error?.message ?? "tool failed") });
        return;
      case "session.usage.updated": {
        const t = d.tokens ?? {};
        const used = (t.input ?? 0) + (t.output ?? 0) + (t.reasoning ?? 0) + (t.cache?.read ?? 0) + (t.cache?.write ?? 0);
        const size = s.limits.get(s.model);
        if (used > 0 && size > 0) host.event(s.id, { type: "usage", used, size, costUsd: typeof d.cost === "number" ? d.cost : undefined });
        return;
      }
      case "permission.asked":
        onPermission(s, d);
        return;
      case "form.created":
        onForm(s, d.form);
        return;
      case "session.execution.succeeded":
        if (s.activity) finish(s, { terminal: "end_turn" });
        return;
      case "session.execution.interrupted":
        if (s.activity || s.stopRequested) finish(s, { terminal: "cancelled" });
        return;
      case "session.execution.failed":
        if (s.activity || s.stopRequested) finish(s, { failed: { message: clip(d.error?.message ?? "OpenCode execution failed") } });
        return;
      default:
    }
  }

  function session(id) {
    const s = sessions.get(id);
    if (!s) throw new SidecarError("unknown session", { code: -32003, phase: "pre_dispatch" });
    return s;
  }

  function text(content) {
    return splitContent(content).filter((p) => p.type === "text").map((p) => p.text).join("\n\n");
  }

  return {
    async initialize() {
      return {
        sdk: { name: "@opencode/sdk", version: sdkVersion },
        capabilities: {
          resume: persistent,
          steering: true,
          stop: "verified_terminal",
          mcpStdio: true,
          mcpHttp: true,
          mcpSse: true,
          imageInput: "unverified",
          toolApproval: "supported",
          modelOptions: "supported",
        },
      };
    },
    startSession: (params) => open(params),
    restoreSession: (params) => {
      if (!persistent) throw new SidecarError("OpenCode V2 resume needs CODETWO_OPENCODE_V2_DB", { phase: "pre_dispatch" });
      return open({ ...params, resumeId: params.sessionId });
    },

    async sendTurn({ sessionId, content }) {
      const s = session(sessionId);
      if (s.streamEnded) throw new SidecarError(`OpenCode event stream is closed: ${s.streamEnded}`, { phase: "pre_dispatch" });
      if (s.turn) throw new SidecarError("a turn is already running", { phase: "pre_dispatch" });
      if (splitContent(content).some((p) => p.type === "image")) {
        throw new SidecarError("OpenCode V2 image input is not verified", { code: -32602, phase: "pre_dispatch" });
      }
      s.turn = deferred();
      const pending = s.turn.promise;
      s.activity = false;
      s.stopRequested = false;
      s.tools.clear();
      try {
        const entry = await opencode.sessions.prompt({ sessionID: s.id, text: text(content), delivery: "queue" });
        s.inboxId = entry?.id ?? null;
      } catch (error) {
        s.turn = null;
        throw new SidecarError(error.message, { phase: provenRefusal(error) ? "pre_dispatch" : "unknown" });
      }
      return pending;
    },

    async steer({ sessionId, content }) {
      const s = session(sessionId);
      if (!s.turn) throw new SidecarError("no running turn to steer", { code: -32601, phase: "pre_dispatch" });
      let entry;
      try {
        entry = await opencode.sessions.prompt({ sessionID: s.id, text: text(content), delivery: "steer" });
      } catch (error) {
        // Rejections keep their phase unless provably refused.
        throw new SidecarError(error.message, { phase: provenRefusal(error) ? "pre_dispatch" : "unknown" });
      }
      const receipt = deferred();
      s.steer.set(entry.id, receipt);
      let timer;
      const outcome = await Promise.race([
        s.inboxResult.has(entry.id) ? Promise.resolve(s.inboxResult.get(entry.id)) : receipt.promise,
        new Promise((resolve) => {
          timer = setTimeout(() => resolve("unconfirmed"), STEER_RECEIPT_TIMEOUT_MS);
        }),
      ]);
      clearTimeout(timer);
      s.steer.delete(entry.id);
      return { receipt: outcome };
    },

    async stop({ sessionId }) {
      const s = session(sessionId);
      if (!s.turn) return;
      s.stopRequested = true;
      // {interrupted: boolean} only; the terminal arrives as session.execution.* events.
      await opencode.sessions.interrupt({ sessionID: s.id });
    },

    async setModel({ sessionId, modelId }) {
      const s = session(sessionId);
      if (!s.choices.some((c) => c.id === modelId)) {
        throw new SidecarError(`unknown model ${modelId}`, { code: -32602, phase: "pre_dispatch" });
      }
      const [base, variant] = modelId.split("#");
      const slash = base.indexOf("/");
      await opencode.sessions.switchModel({
        sessionID: s.id,
        model: { providerID: base.slice(0, slash), id: base.slice(slash + 1), ...(variant ? { variant } : {}) },
      });
      s.model = modelId;
    },

    async setConfigOption({ configId }) {
      throw new SidecarError(`unknown config option ${configId}`, { code: -32602, phase: "pre_dispatch" });
    },

    setExecutionPolicy() {
      // Every permission is `ask`; the host's policy answers each request.
    },

    async shutdown() {
      sessions.clear();
      try {
        await opencode?.close();
      } catch {
        // already closed
      }
      opencode = null;
    },
  };
}
