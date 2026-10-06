// OpenCode V1 adapter (`@opencode-ai/sdk`, HTTP server + SSE).
//
// V1 is a distinct provider identity from OpenCode V2 (`@opencode/sdk`); the V1 package's own
// `/v2` subpath is still the V1 line and is not used here.
//
// Limits taken from ../capability-probes/report.md:
//  - No steering: V1 prompt bodies carry no delivery/queue control.
//  - Abort answers only a boolean: the turn ends on the session's idle/error events.
//  - Permissions are answered through `permission.updated` -> `permissions/{id}` (once|always|reject).
//    The sidecar spawns its OWN server with every permission set to `ask`, because attaching to an
//    external server would let that server's config silently auto-approve tools.
//  - V1 has no question/elicitation API.
//
// Runtime behavior against a real V1 server is not exercised by the fixture tests.

import { SidecarError } from "../common/ipc.mjs";
import { clip } from "../common/runtime.mjs";
import { PERMISSION_OPTIONS, deferred, splitContent, tail, textContent, truncate } from "../common/content.mjs";

const ALL_ASK = Object.freeze({ edit: "ask", bash: "ask", webfetch: "ask", doom_loop: "ask", external_directory: "ask" });

export function permissionKind(type) {
  switch (type) {
    case "bash":
      return "execute";
    case "edit":
    case "write":
    case "patch":
      return "edit";
    case "webfetch":
      return "fetch";
    default:
      return "other";
  }
}

export function toolKind(tool) {
  switch (tool) {
    case "bash":
      return "execute";
    case "read":
      return "read";
    case "edit":
    case "write":
    case "patch":
      return "edit";
    case "glob":
    case "grep":
    case "list":
      return "search";
    case "webfetch":
      return "fetch";
    case "todowrite":
    case "todoread":
      return "think";
    default:
      return "other";
  }
}

/** `server_tool` MCP names are not reliably separable in V1; they are shown as given. */
export function toolTitle(tool, input, stateTitle) {
  const i = input ?? {};
  if (tool === "bash") return truncate(i.command ?? stateTitle ?? "Run command");
  if (i.filePath) return truncate(`${tool} ${i.filePath}`);
  if (i.pattern) return truncate(`${tool} ${i.pattern}`);
  if (i.url) return truncate(`${tool} ${i.url}`);
  return truncate(stateTitle || tool);
}

function mcpConfig(server) {
  if (server.type === "stdio") {
    return { type: "local", command: [server.command, ...(server.args ?? [])], environment: server.env ?? {}, enabled: true };
  }
  if (server.type === "http" || server.type === "sse") {
    return { type: "remote", url: server.url, headers: server.headers ?? {}, enabled: true };
  }
  throw new SidecarError(`unsupported MCP transport: ${server.type}`, { code: -32602, phase: "pre_dispatch" });
}

function errorMessage(error) {
  return error?.data?.message || error?.message || error?.name || "OpenCode error";
}

/** hey-api returns {data, error, response}; keep the HTTP status so rejection can be classified. */
function unwrap(result, what) {
  if (result && typeof result === "object" && "error" in result && result.error) {
    const status = result.response?.status;
    const err = new Error(`${what}: ${errorMessage(result.error)}`);
    err.status = status;
    throw err;
  }
  return result?.data ?? result;
}

export function createOpenCodeV1Adapter({ sdk, host, env = process.env, sdkVersion = "unknown" }) {
  const sessions = new Map();
  let server = null;

  async function ensureServer() {
    if (server) return server;
    const pathPrefix = env.CODETWO_OPENCODE_V1_BIN_DIR;
    if (pathPrefix) process.env.PATH = `${pathPrefix}:${process.env.PATH ?? ""}`;
    try {
      server = await sdk.createOpencodeServer({
        hostname: "127.0.0.1",
        port: 0,
        timeout: 30_000,
        config: { permission: { ...ALL_ASK }, share: "disabled", autoupdate: false },
      });
    } catch (error) {
      throw new SidecarError(
        `could not start an OpenCode V1 server (${error.message}). Provide a V1 \`opencode\` binary on PATH or via CODETWO_OPENCODE_V1_BIN_DIR.`,
        { phase: "pre_dispatch" },
      );
    }
    return server;
  }

  async function loadModels(s) {
    try {
      const data = unwrap(await s.client.config.providers(), "providers");
      const choices = [];
      const limits = new Map();
      for (const provider of data.providers ?? []) {
        for (const model of Object.values(provider.models ?? {})) {
          if (model.status === "deprecated") continue;
          const id = `${provider.id}/${model.id}`;
          choices.push({ id, name: `${provider.name}: ${model.name}`, description: undefined });
          limits.set(id, model.limit?.context);
        }
      }
      const defaults = data.default ?? {};
      const firstProvider = Object.keys(defaults)[0];
      const current = firstProvider ? `${firstProvider}/${defaults[firstProvider]}` : choices[0]?.id;
      s.choices = choices;
      s.limits = limits;
      s.model = choices.some((c) => c.id === current) ? current : choices[0]?.id;
    } catch {
      s.choices = [];
      s.limits = new Map();
    }
  }

  async function open({ resumeId, cwd, mcpServers }) {
    const srv = await ensureServer();
    const client = sdk.createOpencodeClient({ baseUrl: srv.url, directory: cwd });
    const s = {
      id: "",
      cwd,
      client,
      turn: null,
      stopRequested: false,
      messages: new Map(), // messageID -> role
      parked: new Map(), // messageID -> parts waiting for the message role
      partText: new Map(),
      seenTools: new Set(),
      choices: [],
      limits: new Map(),
      model: undefined,
      sawActivity: false,
      abort: new AbortController(),
    };
    try {
      for (const server of mcpServers ?? []) {
        unwrap(await client.mcp.add({ body: { name: server.name, config: mcpConfig(server) } }), `mcp ${server.name}`);
      }
      if (resumeId) {
        unwrap(await client.session.get({ path: { id: resumeId } }), "session");
        s.id = resumeId;
      } else {
        s.id = unwrap(await client.session.create({ body: {} }), "session").id;
      }
    } catch (error) {
      if (error instanceof SidecarError) throw error;
      throw new SidecarError(`OpenCode V1 could not ${resumeId ? "resume" : "create"} the session: ${error.message}`, { phase: "pre_dispatch" });
    }
    await loadModels(s);
    sessions.set(s.id, s);
    subscribe(s);
    return {
      sessionId: s.id,
      models: s.choices.length ? { available: s.choices, current: s.model ?? "" } : undefined,
      configOptions: [],
    };
  }

  async function subscribe(s) {
    try {
      const sub = await s.client.event.subscribe({ signal: s.abort.signal });
      for await (const event of sub.stream) {
        try {
          await onEvent(s, event);
        } catch (error) {
          console.error("opencode-v1 event handling failed:", error);
        }
      }
      s.streamEnded = "event stream ended";
    } catch (error) {
      s.streamEnded = error?.message ?? "event stream failed";
    }
    // Without events no terminal can arrive: the turn's fate is unknown.
    const turn = s.turn;
    s.turn = null;
    if (turn && !s.abort.signal.aborted) {
      turn.reject(new SidecarError(`OpenCode event stream lost mid-turn: ${s.streamEnded}`, { phase: "unknown" }));
    }
  }

  function emitPart(s, part, delta) {
    const owner = s.messages.get(part.messageID);
    if (owner === undefined) {
      const list = s.parked.get(part.messageID) ?? [];
      list.push({ part, delta });
      s.parked.set(part.messageID, list);
      return;
    }
    if (owner !== "assistant") return;
    s.sawActivity = true;
    if (part.type === "text" || part.type === "reasoning") {
      if (part.type === "text" && (part.synthetic || part.ignored)) return;
      const previous = s.partText.get(part.id) ?? "";
      const text = delta ?? (part.text?.startsWith(previous) ? part.text.slice(previous.length) : part.text ?? "");
      s.partText.set(part.id, delta !== undefined ? previous + delta : (part.text ?? previous));
      if (text) host.event(s.id, { type: part.type === "text" ? "agent_text" : "agent_thought", text: clip(text) });
      return;
    }
    if (part.type === "tool") {
      const state = part.state ?? {};
      const id = part.callID || part.id;
      const base = { id, kind: toolKind(part.tool) };
      if (!s.seenTools.has(id)) {
        s.seenTools.add(id);
        host.event(s.id, {
          type: "tool_call",
          ...base,
          title: toolTitle(part.tool, state.input, state.title),
          status: state.status === "pending" ? "pending" : "in_progress",
          rawInput: state.input,
        });
      }
      if (state.status === "completed") {
        host.event(s.id, { type: "tool_update", ...base, status: "completed", content: textContent(tail(state.output ?? "")) });
      } else if (state.status === "error") {
        host.event(s.id, { type: "tool_update", ...base, status: "failed", content: textContent(state.error) });
      }
    }
  }

  async function onPermission(s, permission) {
    const reply = await host.permission(s.id, {
      toolCall: {
        toolCallId: permission.callID || permission.id,
        title: truncate(permission.title || permission.type),
        kind: permissionKind(permission.type),
        rawInput: { type: permission.type, pattern: permission.pattern, metadata: permission.metadata },
        status: "pending",
      },
      options: [PERMISSION_OPTIONS.allowOnce, PERMISSION_OPTIONS.allowAlways, PERMISSION_OPTIONS.rejectOnce],
    });
    // Anything but an offered selection is a rejection.
    const response =
      reply.outcome === "selected" && reply.optionId === "allow"
        ? "once"
        : reply.outcome === "selected" && reply.optionId === "allow_always"
          ? "always"
          : "reject";
    try {
      unwrap(await s.client.postSessionIdPermissionsPermissionId({ path: { id: s.id, permissionID: permission.id }, body: { response } }), "permission reply");
    } catch (error) {
      console.error("opencode-v1 permission reply failed:", error.message);
    }
  }

  function finish(s, outcome) {
    const turn = s.turn;
    if (!turn) return;
    s.turn = null;
    s.stopRequested = false;
    turn.resolve(outcome);
  }

  async function onEvent(s, event) {
    const p = event.properties ?? {};
    switch (event.type) {
      case "message.updated": {
        const info = p.info;
        if (!info || info.sessionID !== s.id) return;
        s.messages.set(info.id, info.role);
        const parked = s.parked.get(info.id);
        if (parked) {
          s.parked.delete(info.id);
          for (const entry of parked) emitPart(s, entry.part, entry.delta);
        }
        if (info.role === "assistant" && info.tokens) {
          const used = (info.tokens.input ?? 0) + (info.tokens.output ?? 0) + (info.tokens.reasoning ?? 0) + (info.tokens.cache?.read ?? 0) + (info.tokens.cache?.write ?? 0);
          const size = s.limits.get(`${info.providerID}/${info.modelID}`);
          if (used > 0 && size > 0) host.event(s.id, { type: "usage", used, size, costUsd: info.cost });
        }
        if (info.role === "assistant" && info.error) s.lastError = info.error;
        return;
      }
      case "message.part.updated":
        if (p.part?.sessionID === s.id) emitPart(s, p.part, p.delta);
        return;
      case "permission.updated":
        if (p.sessionID === s.id) onPermission(s, p);
        return;
      case "session.error":
        if (p.sessionID !== undefined && p.sessionID !== s.id) return;
        s.lastError = p.error ?? s.lastError;
        s.sawActivity = true;
        return;
      case "session.status":
        if (p.sessionID === s.id && p.status?.type === "busy") s.sawActivity = true;
        return;
      case "session.idle": {
        if (p.sessionID !== s.id || !s.turn) return;
        // An idle before any activity belongs to a previous turn, not this one.
        if (!s.sawActivity && !s.stopRequested) return;
        const error = s.lastError;
        s.lastError = undefined;
        if (!error) return finish(s, { terminal: "end_turn" });
        if (error.name === "MessageAbortedError") return finish(s, { terminal: "cancelled" });
        if (error.name === "MessageOutputLengthError") return finish(s, { terminal: "max_tokens" });
        return finish(s, { failed: { message: clip(errorMessage(error)) } });
      }
      default:
    }
  }

  function session(id) {
    const s = sessions.get(id);
    if (!s) throw new SidecarError("unknown session", { code: -32003, phase: "pre_dispatch" });
    return s;
  }

  return {
    async initialize() {
      return {
        sdk: { name: "@opencode-ai/sdk", version: sdkVersion },
        capabilities: {
          resume: true,
          steering: false,
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
    restoreSession: (params) => open({ ...params, resumeId: params.sessionId }),

    async sendTurn({ sessionId, content }) {
      const s = session(sessionId);
      if (s.streamEnded) throw new SidecarError(`OpenCode event stream is closed: ${s.streamEnded}`, { phase: "pre_dispatch" });
      if (s.turn) throw new SidecarError("a turn is already running", { phase: "pre_dispatch" });
      const parts = splitContent(content).map((part) =>
        part.type === "text"
          ? { type: "text", text: part.text }
          : { type: "file", mime: part.mimeType, url: `data:${part.mimeType};base64,${part.data}` },
      );
      const selected = s.model ? s.model.split("/") : null;
      const body = { parts, ...(selected ? { model: { providerID: selected[0], modelID: selected.slice(1).join("/") } } : {}) };
      s.turn = deferred();
      const pending = s.turn.promise;
      s.sawActivity = false;
      s.lastError = undefined;
      s.stopRequested = false;
      s.seenTools.clear();
      try {
        unwrap(await s.client.session.promptAsync({ path: { id: s.id }, body }), "prompt");
      } catch (error) {
        s.turn = null;
        const status = error.status;
        // A 4xx proves the server refused the request before any work began.
        if (status >= 400 && status < 500) throw new SidecarError(error.message, { phase: "pre_dispatch" });
        throw new SidecarError(error.message, { phase: "unknown" });
      }
      return pending;
    },

    async steer() {
      throw new SidecarError("OpenCode V1 has no steering control", { code: -32601, phase: "pre_dispatch" });
    },

    async stop({ sessionId }) {
      const s = session(sessionId);
      if (!s.turn) return;
      s.stopRequested = true;
      // Boolean only; the terminal arrives as session error/idle events.
      unwrap(await s.client.session.abort({ path: { id: s.id } }), "abort");
    },

    async setModel({ sessionId, modelId }) {
      const s = session(sessionId);
      if (!s.choices.some((c) => c.id === modelId)) {
        throw new SidecarError(`unknown model ${modelId}`, { code: -32602, phase: "pre_dispatch" });
      }
      s.model = modelId;
    },

    async setConfigOption({ configId }) {
      throw new SidecarError(`unknown config option ${configId}`, { code: -32602, phase: "pre_dispatch" });
    },

    setExecutionPolicy() {
      // Every permission is `ask` on the server; the host's policy answers each request.
    },

    async shutdown() {
      for (const s of sessions.values()) s.abort.abort();
      sessions.clear();
      try {
        server?.close();
      } catch {
        // already closed
      }
      server = null;
    },
  };
}
