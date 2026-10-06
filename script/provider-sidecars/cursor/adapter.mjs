// Cursor TypeScript SDK adapter (`@cursor/sdk`, local runtime).
//
// Evidence-driven limits (see ../capability-probes/report.md):
//  - There is NO approval reply channel in the SDK: tool approval is auto-approve or auto-deny.
//    So `toolApproval` is unsupported and any session that is not full-access (`yolo`) is refused
//    before it starts. Nothing here ever pretends an approval happened.
//  - A read-only sandbox cannot be guaranteed (`mode: plan` and `sandboxOptions` are not
//    guarantees), so a `read_only` policy is refused rather than approximated.
//  - `tools` / `disallowedTools` are not persisted by the SDK; they are re-passed on every
//    `Agent.create` and `Agent.resume`.
//  - Steering only works on the live `Run` handle of this process. `complete_delivered` is the only
//    positive receipt; `revert_to_followup` also means timeout/submit failure, so it is reported
//    as unconfirmed and never resent.
//  - Cancel is optimistic in the SDK: the turn ends when `run.wait()` settles, not when
//    `run.cancel()` returns.
//  - Model selection is copied verbatim from the account catalog; nothing is mapped to a "nearby"
//    model, and a 300k/high variant exists only if the catalog lists it.

import { SidecarError } from "../common/ipc.mjs";
import { clip } from "../common/runtime.mjs";
import { splitContent, textContent, tail, truncate } from "../common/content.mjs";

const MODEL_LIST_TIMEOUT_MS = 15_000;

export function toolKind(name) {
  switch (name) {
    case "shell":
      return "execute";
    case "read":
    case "readLints":
    case "readTodos":
      return "read";
    case "edit":
    case "applyAgentDiff":
    case "write":
      return "edit";
    case "delete":
      return "delete";
    case "grep":
    case "glob":
    case "ls":
    case "semSearch":
      return "search";
    case "webFetch":
    case "webSearch":
      return "fetch";
    case "updateTodos":
      return "think";
    default:
      return "other";
  }
}

export function toolTitle(name, args) {
  const a = args && typeof args === "object" ? args : {};
  switch (name) {
    case "shell":
      return truncate(a.command ?? "Run command");
    case "read":
    case "edit":
    case "write":
    case "delete":
      return truncate(`${name} ${a.path ?? a.filePath ?? ""}`.trim());
    case "grep":
    case "glob":
      return truncate(`${name} ${a.pattern ?? a.globPattern ?? ""}`.trim());
    case "mcp": {
      const server = a.providerIdentifier ?? a.server;
      const tool = a.toolName ?? a.tool;
      return server && tool ? `mcp.${server}.${tool}` : "mcp";
    }
    default:
      return truncate(name);
  }
}

function resultText(result) {
  if (result == null) return "";
  if (typeof result === "string") return result;
  try {
    return JSON.stringify(result);
  } catch {
    return String(result);
  }
}

/** Errors the SDK raises before any run exists. Everything else may have reached the model. */
function isPreRun(error) {
  const name = error?.constructor?.name ?? error?.name ?? "";
  return ["AgentBusyError", "AuthenticationError", "ConfigurationError", "AgentNotFoundError", "IntegrationNotConnectedError"].includes(name);
}

function encodeMcpServers(servers) {
  const out = {};
  for (const server of servers ?? []) {
    if (server.type === "stdio") {
      out[server.name] = { type: "stdio", command: server.command, args: server.args ?? [], env: server.env ?? {}, ...(server.cwd ? { cwd: server.cwd } : {}) };
    } else if (server.type === "http" || server.type === "sse") {
      out[server.name] = { type: server.type, url: server.url, headers: server.headers ?? {} };
    } else {
      throw new SidecarError(`unsupported MCP transport: ${server.type}`, { code: -32602, phase: "pre_dispatch" });
    }
  }
  return out;
}

/** Choice id for a catalog entry: stable and reversible, params copied verbatim. */
export function selectionId(modelId, params) {
  return params?.length ? `${modelId}#${params.map((p) => `${p.id}=${p.value}`).join("&")}` : modelId;
}

export function buildCatalog(models) {
  const choices = [];
  const selections = new Map();
  for (const model of models ?? []) {
    const variants = model.variants?.length ? model.variants : [{ params: [], displayName: model.displayName, description: model.description }];
    for (const variant of variants) {
      const id = selectionId(model.id, variant.params);
      if (selections.has(id)) continue;
      selections.set(id, variant.params?.length ? { id: model.id, params: variant.params.map((p) => ({ id: p.id, value: p.value })) } : { id: model.id });
      choices.push({
        id,
        name: variants.length > 1 ? `${model.displayName} — ${variant.displayName}` : model.displayName,
        description: variant.description ?? model.description,
        isDefault: variant.isDefault === true,
      });
    }
  }
  return { choices, selections };
}

function checkExecution(execution) {
  if (execution?.mode !== "yolo") {
    throw new SidecarError(
      "Cursor has no tool-approval reply channel, so approval-required modes are unavailable. Use full-access mode or another backend.",
      { code: -32602, phase: "pre_dispatch", data: { unavailable: "tool_approval" } },
    );
  }
  if (execution.sandbox === "read_only") {
    throw new SidecarError("Cursor cannot guarantee a read-only sandbox.", {
      code: -32602,
      phase: "pre_dispatch",
      data: { unavailable: "read_only_sandbox" },
    });
  }
}

export function createCursorAdapter({ sdk, host, env = process.env, sdkVersion = "unknown" }) {
  const sessions = new Map();
  const apiKey = env.CURSOR_API_KEY || undefined;
  let catalog = { choices: [], selections: new Map() };
  let catalogLoaded = false;

  async function loadCatalog() {
    if (catalogLoaded) return catalog;
    catalogLoaded = true;
    try {
      let timer;
      const models = await Promise.race([
        sdk.Cursor.models.list(apiKey ? { apiKey } : undefined),
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error("timed out")), MODEL_LIST_TIMEOUT_MS);
        }),
      ]).finally(() => clearTimeout(timer));
      catalog = buildCatalog(models);
    } catch {
      // No catalog (offline or no credential): model switching is simply unavailable.
    }
    return catalog;
  }

  function agentOptions(s) {
    return {
      ...(apiKey ? { apiKey } : {}),
      ...(s.selection ? { model: s.selection } : {}),
      ...(s.tools ? { tools: s.tools } : {}),
      ...(s.disallowedTools ? { disallowedTools: s.disallowedTools } : {}),
      local: { cwd: s.cwd },
      mcpServers: s.mcpServers,
      mode: "agent",
    };
  }

  function stateOf(s) {
    const { choices } = catalog;
    const current = choices.find((c) => c.id === s.currentChoice)?.id ?? choices.find((c) => c.isDefault)?.id ?? choices[0]?.id;
    return {
      sessionId: s.id,
      models: choices.length ? { available: choices.map(({ id, name, description }) => ({ id, name, description })), current: s.currentChoice ?? current ?? "" } : undefined,
      configOptions: [],
    };
  }

  async function open({ resumeId, cwd, mcpServers, execution, tools, disallowedTools }) {
    checkExecution(execution);
    await loadCatalog();
    const s = {
      id: resumeId ?? "",
      cwd,
      execution,
      mcpServers: encodeMcpServers(mcpServers),
      tools: Array.isArray(tools) ? tools : undefined,
      disallowedTools: Array.isArray(disallowedTools) ? disallowedTools : undefined,
      agent: null,
      run: null,
      stopRequested: false,
      selection: undefined,
      currentChoice: undefined,
      seenTools: new Set(),
    };
    try {
      s.agent = resumeId ? await sdk.Agent.resume(resumeId, agentOptions(s)) : await sdk.Agent.create(agentOptions(s));
    } catch (error) {
      throw new SidecarError(`Cursor could not ${resumeId ? "resume" : "start"} the agent: ${error.message}`, { phase: "pre_dispatch" });
    }
    s.id = s.agent.agentId;
    if (resumeId && s.id !== resumeId) {
      s.agent.close();
      throw new SidecarError("Cursor resumed a different agent than requested", { phase: "pre_dispatch" });
    }
    sessions.set(s.id, s);
    return stateOf(s);
  }

  function session(id) {
    const s = sessions.get(id);
    if (!s) throw new SidecarError("unknown session", { code: -32003, phase: "pre_dispatch" });
    return s;
  }

  function onMessage(s, message) {
    switch (message.type) {
      case "assistant":
        for (const block of message.message?.content ?? []) {
          if (block.type === "text" && block.text) host.event(s.id, { type: "agent_text", text: clip(block.text) });
        }
        return;
      case "thinking":
        if (message.text) host.event(s.id, { type: "agent_thought", text: clip(message.text) });
        return;
      case "tool_call": {
        const common = {
          id: message.call_id,
          kind: toolKind(message.name),
          status: message.status === "completed" ? "completed" : message.status === "error" ? "failed" : "in_progress",
        };
        if (!s.seenTools.has(message.call_id)) {
          s.seenTools.add(message.call_id);
          host.event(s.id, { type: "tool_call", ...common, title: toolTitle(message.name, message.args), rawInput: message.args });
        }
        if (message.status !== "running") {
          host.event(s.id, {
            type: "tool_update",
            ...common,
            content: textContent(tail(resultText(message.result))),
          });
        }
        return;
      }
      default:
    }
  }

  return {
    async initialize() {
      return {
        sdk: { name: "@cursor/sdk", version: sdkVersion },
        capabilities: {
          resume: true,
          steering: true,
          stop: "verified_terminal",
          mcpStdio: true,
          mcpHttp: true,
          mcpSse: true,
          imageInput: "unverified",
          toolApproval: "unsupported",
          modelOptions: "supported",
        },
      };
    },

    startSession: (params) => open(params),
    restoreSession: (params) => open({ ...params, resumeId: params.sessionId }),

    async sendTurn({ sessionId, content }) {
      const s = session(sessionId);
      if (s.policyBlocked) {
        throw new SidecarError(s.policyBlocked, { phase: "pre_dispatch", data: { unavailable: "tool_approval" } });
      }
      if (s.run) throw new SidecarError("a turn is already running", { phase: "pre_dispatch" });
      const parts = splitContent(content);
      const message = {
        text: parts.filter((p) => p.type === "text").map((p) => p.text).join("\n\n"),
        ...(parts.some((p) => p.type === "image")
          ? { images: parts.filter((p) => p.type === "image").map((p) => ({ data: p.data, mimeType: p.mimeType })) } : {}),
      };
      s.seenTools.clear();
      s.stopRequested = false;
      s.sending = true;
      let run;
      try {
        run = await s.agent.send(message, s.selection ? { model: s.selection, mode: "agent" } : { mode: "agent" });
      } catch (error) {
        s.sending = false;
        if (isPreRun(error)) throw new SidecarError(error.message, { phase: "pre_dispatch" });
        // The request may have left before the failure: do not claim it did not.
        throw new SidecarError(`Cursor send failed: ${error.message}`, { phase: "unknown" });
      }
      s.sending = false;
      s.run = run;
      // A stop that arrived while the run was being created applies as soon as the run exists.
      if (s.stopRequested) run.cancel().catch((error) => console.error("cursor cancel failed:", error));
      const streaming = (async () => {
        for await (const message of run.stream()) onMessage(s, message);
      })();
      const [streamed, waited] = await Promise.allSettled([streaming, run.wait()]);
      s.run = null;
      if (waited.status === "rejected") {
        throw new SidecarError(`Cursor run ended without a result: ${waited.reason?.message}`, { phase: "unknown" });
      }
      if (streamed.status === "rejected") console.error("cursor stream ended with an error:", streamed.reason);
      const result = waited.value;
      if (result.status === "finished") return { terminal: "end_turn" };
      if (result.status === "cancelled") return { terminal: "cancelled" };
      return { failed: { message: clip(result.error?.message ?? "Cursor run failed") } };
    },

    async steer({ sessionId, content }) {
      const s = session(sessionId);
      const run = s.run;
      if (!run || typeof run.steer !== "function") {
        throw new SidecarError("no live Cursor run can be steered", { code: -32601, phase: "pre_dispatch" });
      }
      const text = splitContent(content).filter((p) => p.type === "text").map((p) => p.text).join("\n\n");
      // Rejections propagate without a phase: the request may have left, so the host keeps Unknown.
      const outcome = await run.steer(text);
      return { receipt: outcome === "complete_delivered" ? "delivered" : "unconfirmed" };
    },

    async stop({ sessionId }) {
      const s = session(sessionId);
      if (!s.run) {
        if (s.sending) s.stopRequested = true;
        return;
      }
      s.stopRequested = true;
      // Optimistic in the SDK: the turn is only over when run.wait() settles in sendTurn.
      await s.run.cancel();
    },

    async setModel({ sessionId, modelId }) {
      const s = session(sessionId);
      const selection = catalog.selections.get(modelId);
      if (!selection) {
        throw new SidecarError(`model ${modelId} is not in the Cursor catalog`, { code: -32602, phase: "pre_dispatch" });
      }
      s.selection = selection;
      s.currentChoice = modelId;
    },

    async setConfigOption({ configId }) {
      throw new SidecarError(`unknown config option ${configId}`, { code: -32602, phase: "pre_dispatch" });
    },

    setExecutionPolicy({ sessionId, execution }) {
      const s = sessions.get(sessionId);
      if (!s) return;
      s.execution = execution;
      try {
        checkExecution(execution);
        s.policyBlocked = undefined;
      } catch (error) {
        // Never keep running under a policy this backend cannot honor.
        s.policyBlocked = error.message;
      }
    },

    async shutdown() {
      for (const s of sessions.values()) {
        try {
          await s.run?.cancel();
        } catch {
          // best effort during shutdown
        }
        try {
          s.agent?.close();
        } catch {
          // already closed
        }
      }
      sessions.clear();
    },
  };
}
