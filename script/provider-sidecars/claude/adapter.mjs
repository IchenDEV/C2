// Claude Agent SDK adapter (`@anthropic-ai/claude-agent-sdk`).
//
// One C2 session = one streaming-input `query()`. A turn is one user message pushed into the
// input queue; it ends at the matching `result` message, never at an interrupt call.
//
// Honesty rules kept here:
//  - Every tool permission goes through `canUseTool` to the host; the SDK is never given a
//    permission mode that auto-approves (`bypassPermissions` / `allowDangerouslySkipPermissions`).
//  - Mid-turn steering is NOT advertised: the SDK offers no per-message delivery receipt, so a
//    pushed message could not be confirmed. The host keeps such input for the next turn.
//  - No 300k context option is offered; context size comes only from SDK-reported model usage.
//  - Credentials are the SDK's own (env / its login store). The sidecar neither reads nor stores them.

import { randomUUID } from "node:crypto";
import { SidecarError } from "../common/ipc.mjs";
import { clip } from "../common/runtime.mjs";
import {
  PERMISSION_OPTIONS,
  createInputQueue,
  deferred,
  questionSchema,
  splitContent,
  tail,
  textContent,
  truncate,
} from "../common/content.mjs";

const MODEL_LIST_TIMEOUT_MS = 15_000;
const EFFORT_OPTION = "reasoning_effort";

export function toolKind(name) {
  switch (name) {
    case "Bash":
    case "BashOutput":
    case "KillShell":
      return "execute";
    case "Read":
      return "read";
    case "Glob":
    case "Grep":
      return "search";
    case "Edit":
    case "MultiEdit":
    case "Write":
    case "NotebookEdit":
      return "edit";
    case "WebFetch":
    case "WebSearch":
      return "fetch";
    case "TodoWrite":
      return "think";
    default:
      return "other";
  }
}

/** `mcp__server__tool` -> {server, tool}. Server names may not contain `__`; tool names may. */
export function parseMcpName(name) {
  const match = /^mcp__([^_](?:.*?[^_])?)__(.+)$/.exec(name);
  return match ? { server: match[1], tool: match[2] } : null;
}

export function toolTitle(name, input) {
  const mcp = parseMcpName(name);
  if (mcp) return `mcp.${mcp.server}.${mcp.tool}`;
  const i = input ?? {};
  switch (name) {
    case "Bash":
      return truncate(i.command ?? i.description ?? "Run command");
    case "Read":
    case "Edit":
    case "MultiEdit":
    case "Write":
      return truncate(`${name} ${i.file_path ?? ""}`.trim());
    case "NotebookEdit":
      return truncate(`Edit notebook ${i.notebook_path ?? ""}`.trim());
    case "Glob":
      return truncate(`Glob ${i.pattern ?? ""}`.trim());
    case "Grep":
      return truncate(`Grep ${i.pattern ?? ""}`.trim());
    case "WebFetch":
      return truncate(`Fetch ${i.url ?? ""}`.trim());
    case "WebSearch":
      return truncate(`Search ${i.query ?? ""}`.trim());
    default:
      return truncate(i.description ?? name);
  }
}

function rawInputFor(name, input) {
  const mcp = parseMcpName(name);
  return mcp ? { server: mcp.server, tool: mcp.tool, arguments: input } : input;
}

export function terminalFromResult(result, stopRequested) {
  const reason = result.terminal_reason;
  if (reason === "aborted_streaming" || reason === "aborted_tools") return { terminal: "cancelled" };
  if (result.subtype === "success") {
    if (result.is_error) return { failed: { message: clip(result.result || "Claude reported an error") } };
    if (reason === "max_turns") return { terminal: "max_turn_requests" };
    if (result.stop_reason === "max_tokens") return { terminal: "max_tokens" };
    if (result.stop_reason === "refusal") return { terminal: "refusal" };
    return { terminal: "end_turn" };
  }
  if (result.subtype === "error_max_turns") return { terminal: "max_turn_requests" };
  if (stopRequested && result.subtype === "error_during_execution") return { terminal: "cancelled" };
  const detail = (result.errors ?? []).join("; ") || result.startup_failure_reason || result.subtype;
  return { failed: { message: clip(String(detail)) } };
}

function toUserMessage(content) {
  const blocks = splitContent(content).map((part) =>
    part.type === "text"
      ? { type: "text", text: part.text }
      : { type: "image", source: { type: "base64", media_type: part.mimeType, data: part.data } },
  );
  return {
    type: "user",
    message: { role: "user", content: blocks },
    parent_tool_use_id: null,
  };
}

function encodeMcpServers(servers) {
  const out = {};
  for (const server of servers ?? []) {
    if (server.type === "stdio") {
      out[server.name] = { type: "stdio", command: server.command, args: server.args ?? [], env: server.env ?? {} };
    } else if (server.type === "http") {
      out[server.name] = { type: "http", url: server.url, headers: server.headers ?? {} };
    } else if (server.type === "sse") {
      out[server.name] = { type: "sse", url: server.url, headers: server.headers ?? {} };
    } else {
      throw new SidecarError(`unsupported MCP transport: ${server.type}`, { code: -32602, phase: "pre_dispatch" });
    }
  }
  return out;
}

function modelCatalog(models) {
  const available = (models ?? []).map((m) => ({
    id: m.value,
    name: m.displayName || m.value,
    description: m.description || undefined,
  }));
  return available;
}

function effortOption(model, current) {
  const levels = model?.supportsEffort ? (model.supportedEffortLevels ?? []) : [];
  if (!levels.length) return [];
  return [
    {
      id: EFFORT_OPTION,
      name: "Reasoning effort",
      category: "thought_level",
      current: current && levels.includes(current) ? current : levels.includes("medium") ? "medium" : levels[0],
      choices: levels.map((level) => ({ id: level, name: level })),
    },
  ];
}

export function createClaudeAdapter({ sdk, host, env = process.env, sdkVersion = "unknown" }) {
  const sessions = new Map();

  async function withTimeout(promise, ms) {
    let timer;
    try {
      return await Promise.race([
        promise,
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error("timed out")), ms);
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  }

  function emitToolCalls(s, message) {
    if (message.parent_tool_use_id) return;
    for (const block of message.message?.content ?? []) {
      if (block.type !== "tool_use") continue;
      s.tools.set(block.id, { name: block.name, input: block.input });
      host.event(s.id, {
        type: "tool_call",
        id: block.id,
        title: toolTitle(block.name, block.input),
        kind: toolKind(block.name),
        status: "in_progress",
        rawInput: rawInputFor(block.name, block.input),
      });
    }
  }

  function emitToolResults(s, message) {
    if (message.parent_tool_use_id) return;
    const content = message.message?.content;
    if (!Array.isArray(content)) return;
    for (const block of content) {
      if (block.type !== "tool_result") continue;
      const known = s.tools.get(block.tool_use_id);
      const text = Array.isArray(block.content)
        ? block.content.map((c) => (c.type === "text" ? c.text : "")).join("")
        : String(block.content ?? "");
      host.event(s.id, {
        type: "tool_update",
        id: block.tool_use_id,
        status: block.is_error ? "failed" : "completed",
        content: textContent(text),
        rawOutput: known ? { name: known.name } : undefined,
      });
      s.tools.delete(block.tool_use_id);
    }
  }

  function handleMessage(s, message) {
    switch (message.type) {
      case "stream_event": {
        if (message.parent_tool_use_id) return;
        const event = message.event;
        if (event?.type === "message_start" && event.message?.id) s.streamed.add(event.message.id);
        if (event?.type === "content_block_delta") {
          if (event.delta?.type === "text_delta" && event.delta.text) {
            host.event(s.id, { type: "agent_text", text: clip(event.delta.text) });
          } else if (event.delta?.type === "thinking_delta" && event.delta.thinking) {
            host.event(s.id, { type: "agent_thought", text: clip(event.delta.thinking) });
          }
        }
        return;
      }
      case "assistant": {
        if (message.parent_tool_use_id) return;
        // Text already arrived as deltas when the message id was streamed; replay only the rest.
        if (!s.streamed.has(message.message?.id)) {
          for (const block of message.message?.content ?? []) {
            if (block.type === "text" && block.text) host.event(s.id, { type: "agent_text", text: clip(block.text) });
            else if (block.type === "thinking" && block.thinking) {
              host.event(s.id, { type: "agent_thought", text: clip(block.thinking) });
            }
          }
        }
        emitToolCalls(s, message);
        return;
      }
      case "user":
        emitToolResults(s, message);
        return;
      case "result": {
        const usage = message.usage ?? {};
        const used =
          (usage.input_tokens ?? 0) +
          (usage.cache_read_input_tokens ?? 0) +
          (usage.cache_creation_input_tokens ?? 0) +
          (usage.output_tokens ?? 0);
        const size = Object.values(message.modelUsage ?? {})
          .map((m) => m.contextWindow)
          .filter((n) => Number.isFinite(n) && n > 0)
          .reduce((max, n) => Math.max(max, n), 0);
        if (used > 0 && size > 0) {
          host.event(s.id, { type: "usage", used, size, costUsd: message.total_cost_usd });
        }
        const outcome = terminalFromResult(message, s.stopRequested);
        const turn = s.turn;
        s.turn = null;
        s.stopRequested = false;
        turn?.resolve(outcome);
        return;
      }
      case "system":
        if (message.subtype === "init" && Array.isArray(message.slash_commands)) {
          host.event(s.id, { type: "commands", names: message.slash_commands });
        }
        return;
      default:
    }
  }

  async function pump(s) {
    try {
      for await (const message of s.query) handleMessage(s, message);
      s.endReason = "ended";
    } catch (error) {
      s.endReason = error?.message ?? "failed";
    }
    s.alive = false;
    const turn = s.turn;
    s.turn = null;
    // The request may have reached the model before the stream died: the outcome is unknown.
    turn?.reject(
      new SidecarError(`Claude session ended mid-turn: ${s.endReason}`, { phase: "unknown" }),
    );
  }

  async function canUseTool(s, toolName, input, options) {
    // The built-in question tool is a question to the user, not a tool approval.
    if (toolName === "AskUserQuestion" && Array.isArray(input?.questions)) {
      const questions = input.questions.map((q, index) => ({
        id: `question_${index}`,
        header: q.header,
        question: q.question,
        multiSelect: q.multiSelect,
        options: q.options,
      }));
      const reply = await host.question(s.id, {
        message: input.questions[0]?.question ?? "Claude has a question",
        toolCallId: options.toolUseID,
        schema: questionSchema(questions),
      });
      if (reply.action !== "accept") {
        return { behavior: "deny", message: "The user did not answer.", interrupt: reply.action === "cancel" };
      }
      const answers = {};
      input.questions.forEach((q, index) => {
        const value = reply.content?.[`question_${index}`];
        if (value !== undefined) answers[q.question] = Array.isArray(value) ? value.join(", ") : String(value);
      });
      return { behavior: "allow", updatedInput: { ...input, answers } };
    }

    const mcp = parseMcpName(toolName);
    const offered = [PERMISSION_OPTIONS.allowOnce];
    if (options.suggestions?.length) offered.push(PERMISSION_OPTIONS.allowAlways);
    offered.push(PERMISSION_OPTIONS.rejectOnce);
    const reply = await host.permission(s.id, {
      toolCall: {
        toolCallId: options.toolUseID,
        title: toolTitle(toolName, input),
        kind: toolKind(toolName),
        rawInput: rawInputFor(toolName, input),
        status: "pending",
      },
      options: offered,
      meta: mcp ? { is_mcp_tool_approval: true, server: mcp.server, tool: mcp.tool } : undefined,
    });
    if (reply.outcome !== "selected") {
      // Dismissed or unreachable: deny and stop, never approve.
      return { behavior: "deny", message: "Permission request was cancelled.", interrupt: true };
    }
    if (reply.optionId === PERMISSION_OPTIONS.allowOnce.id) return { behavior: "allow", updatedInput: input };
    if (reply.optionId === PERMISSION_OPTIONS.allowAlways.id && options.suggestions?.length) {
      return { behavior: "allow", updatedInput: input, updatedPermissions: options.suggestions };
    }
    return { behavior: "deny", message: "The user denied this action." };
  }

  async function onElicitation(s, request) {
    if (request.mode === "url" || !request.requestedSchema) return { action: "decline" };
    const reply = await host.question(s.id, {
      message: request.message,
      schema: request.requestedSchema,
    });
    if (reply.action === "accept") return { action: "accept", content: reply.content ?? {} };
    return { action: reply.action === "cancel" ? "cancel" : "decline" };
  }

  async function open({ id, resume, cwd, mcpServers, execution }) {
    const s = {
      id,
      cwd,
      execution,
      alive: true,
      turn: null,
      stopRequested: false,
      streamed: new Set(),
      tools: new Map(),
      input: createInputQueue(),
      models: [],
      model: undefined,
      effort: undefined,
      query: null,
    };
    const options = {
      cwd,
      includePartialMessages: true,
      permissionMode: "default",
      canUseTool: (name, input, opts) => canUseTool(s, name, input, opts),
      onElicitation: (request) => onElicitation(s, request),
      mcpServers: encodeMcpServers(mcpServers),
      env: { ...env },
      ...(resume ? { resume: id } : { sessionId: id }),
    };
    if (env.CODETWO_CLAUDE_EXECUTABLE) options.pathToClaudeCodeExecutable = env.CODETWO_CLAUDE_EXECUTABLE;
    s.query = sdk.query({ prompt: s.input, options });
    sessions.set(id, s);
    pump(s);

    let models = [];
    try {
      const catalog = await withTimeout(s.query.supportedModels(), MODEL_LIST_TIMEOUT_MS);
      s.models = catalog;
      models = modelCatalog(catalog);
    } catch (error) {
      if (!s.alive) {
        sessions.delete(id);
        throw new SidecarError(`Claude failed to start: ${s.endReason ?? error.message}`, { phase: "pre_dispatch" });
      }
      // Catalog unavailable: the session still works, with no model switcher.
    }
    s.model = models[0]?.id;
    const current = s.models.find((m) => m.value === s.model);
    return {
      sessionId: id,
      models: models.length ? { available: models, current: s.model ?? "" } : undefined,
      configOptions: effortOption(current, s.effort),
    };
  }

  function session(id) {
    const s = sessions.get(id);
    if (!s) throw new SidecarError("unknown session", { code: -32003, phase: "pre_dispatch" });
    return s;
  }

  return {
    async initialize() {
      return {
        sdk: { name: "@anthropic-ai/claude-agent-sdk", version: sdkVersion },
        capabilities: {
          resume: true,
          steering: false,
          stop: "verified_terminal",
          mcpStdio: true,
          mcpHttp: true,
          mcpSse: true,
          imageInput: "supported",
          toolApproval: "supported",
          modelOptions: "supported",
        },
      };
    },
    startSession: (params) => open({ id: randomUUID(), resume: false, ...params }),
    restoreSession: (params) => open({ id: params.sessionId, resume: true, ...params }),

    async sendTurn({ sessionId, content }) {
      const s = session(sessionId);
      if (!s.alive) throw new SidecarError("Claude session is closed", { phase: "pre_dispatch" });
      if (s.turn) throw new SidecarError("a turn is already running", { phase: "pre_dispatch" });
      s.turn = deferred();
      const pending = s.turn.promise;
      s.streamed.clear();
      s.input.push(toUserMessage(content));
      return pending;
    },

    async steer() {
      // Not advertised (capabilities.steering is false); the host never calls this.
      throw new SidecarError("Claude steering is not supported", { code: -32601, phase: "pre_dispatch" });
    },

    async stop({ sessionId }) {
      const s = session(sessionId);
      if (!s.turn) return;
      s.stopRequested = true;
      // A request only: the turn ends when the SDK emits its terminal `result`.
      await s.query.interrupt();
    },

    async setModel({ sessionId, modelId }) {
      const s = session(sessionId);
      const known = s.models.find((m) => m.value === modelId);
      if (s.models.length && !known) {
        throw new SidecarError(`unknown model ${modelId}`, { code: -32602, phase: "pre_dispatch" });
      }
      await s.query.setModel(modelId);
      s.model = modelId;
    },

    async setConfigOption({ sessionId, configId, value }) {
      const s = session(sessionId);
      if (configId !== EFFORT_OPTION) {
        throw new SidecarError(`unknown config option ${configId}`, { code: -32602, phase: "pre_dispatch" });
      }
      const model = s.models.find((m) => m.value === s.model);
      if (!model?.supportedEffortLevels?.includes(value)) {
        throw new SidecarError(`unsupported effort ${value}`, { code: -32602, phase: "pre_dispatch" });
      }
      await s.query.applyFlagSettings({ effortLevel: value });
      s.effort = value;
      return { configOptions: effortOption(model, value) };
    },

    setExecutionPolicy({ sessionId, execution }) {
      // Policy is enforced by the host through `canUseTool`; remember it for diagnostics only.
      const s = sessions.get(sessionId);
      if (s) s.execution = execution;
    },

    async shutdown() {
      for (const s of sessions.values()) {
        s.alive = false;
        s.input.close();
        try {
          s.query.close();
        } catch {
          // already closed
        }
      }
      sessions.clear();
    },
  };
}
