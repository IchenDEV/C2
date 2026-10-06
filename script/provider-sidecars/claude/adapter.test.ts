import { describe, expect, test } from "bun:test";
import { createClaudeAdapter, parseMcpName, terminalFromResult, toolKind, toolTitle } from "./adapter.mjs";
import { initAndStart, startHost, until } from "../common/testkit.mjs";
import { createInputQueue, deferred } from "../common/content.mjs";

type Script = (ctx: { options: any; push: (m: any) => void; end: () => void; userMessages: any[] }) => void;

/** A scriptable stand-in for the SDK's `query()`: messages the test pushes are what "Claude" emits. */
function fakeSdk(opts: { models?: any[]; onUser?: (msg: any, emit: (m: any) => void) => void } = {}) {
  const calls: any[] = [];
  const sdk = {
    query({ prompt, options }: any) {
      const out = createInputQueue();
      const closeOut = out.close;
      const userMessages: any[] = [];
      const state: any = { options, interrupted: 0, model: undefined, effort: undefined, closed: false };
      calls.push(state);
      (async () => {
        for await (const msg of prompt) {
          userMessages.push(msg);
          opts.onUser?.(msg, (m) => out.push(m));
        }
      })();
      state.userMessages = userMessages;
      state.emit = (m: any) => out.push(m);
      state.end = () => closeOut();
      return Object.assign(out, {
        async interrupt() {
          state.interrupted++;
          out.push({ type: "result", subtype: "error_during_execution", terminal_reason: "aborted_streaming", is_error: true, errors: [] });
        },
        async supportedModels() {
          return (
            opts.models ?? [
              { value: "default", displayName: "Default", description: "d", supportsEffort: true, supportedEffortLevels: ["low", "medium", "high"] },
              { value: "haiku", displayName: "Haiku", description: "h" },
            ]
          );
        },
        async setModel(m: string) {
          state.model = m;
        },
        async applyFlagSettings(s: any) {
          state.effort = s.effortLevel;
        },
        close() {
          state.closed = true;
          closeOut();
        },
      });
    },
  };
  return { sdk, calls };
}

const ok = { type: "result", subtype: "success", is_error: false, result: "done", stop_reason: "end_turn", terminal_reason: "completed", usage: { input_tokens: 10, output_tokens: 5 }, modelUsage: { m: { contextWindow: 200000 } }, total_cost_usd: 0.01 };

function boot(fake: ReturnType<typeof fakeSdk>, handlers: any = {}) {
  return startHost({
    backend: "claude-agent-sdk",
    createAdapter: (host: any) => createClaudeAdapter({ sdk: fake.sdk, host, env: {}, sdkVersion: "test" }),
    ...handlers,
  });
}

describe("claude helpers", () => {
  test("tool vocabulary", () => {
    expect(toolKind("Bash")).toBe("execute");
    expect(toolKind("Edit")).toBe("edit");
    expect(toolKind("mcp__srv__do")).toBe("other");
    expect(parseMcpName("mcp__github__create__issue")).toEqual({ server: "github", tool: "create__issue" });
    expect(toolTitle("mcp__github__create_issue", {})).toBe("mcp.github.create_issue");
    expect(toolTitle("Bash", { command: "ls -la" })).toBe("ls -la");
  });

  test("terminal mapping", () => {
    expect(terminalFromResult(ok as any, false)).toEqual({ terminal: "end_turn" });
    expect(terminalFromResult({ ...ok, stop_reason: "max_tokens" } as any, false)).toEqual({ terminal: "max_tokens" });
    expect(terminalFromResult({ ...ok, terminal_reason: "aborted_tools" } as any, false)).toEqual({ terminal: "cancelled" });
    expect(terminalFromResult({ type: "result", subtype: "error_max_turns" } as any, false)).toEqual({ terminal: "max_turn_requests" });
    expect((terminalFromResult({ type: "result", subtype: "error_during_execution", errors: ["boom"] } as any, false) as any).failed.message).toBe("boom");
    expect(terminalFromResult({ type: "result", subtype: "success", is_error: true, result: "API down" } as any, false)).toEqual({ failed: { message: "API down" } });
  });
});

describe("claude adapter", () => {
  test("initialize advertises honest capabilities", async () => {
    const host = boot(fakeSdk());
    const { init } = await initAndStart(host, "claude-agent-sdk");
    expect(init.capabilities.steering).toBe(false);
    expect(init.capabilities.stop).toBe("verified_terminal");
    expect(init.sdk.name).toBe("@anthropic-ai/claude-agent-sdk");
    host.close();
  });

  test("start never uses an auto-approving permission mode and offers models", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "claude-agent-sdk");
    const options = fake.calls[0].options;
    expect(options.permissionMode).toBe("default");
    expect(options.allowDangerouslySkipPermissions).toBeUndefined();
    expect(options.sessionId).toBe(state.sessionId);
    expect(state.models.available.map((m: any) => m.id)).toEqual(["default", "haiku"]);
    expect(state.configOptions[0].id).toBe("reasoning_effort");
    host.close();
  });

  test("restore resumes the requested session id", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    await host.request("initialize", { protocol: 1, backend: "claude-agent-sdk" });
    const state = await host.request("session/restore", { sessionId: "abc", cwd: "/w", mcpServers: [], execution: { mode: "ask", sandbox: "read_only" } });
    expect(state.sessionId).toBe("abc");
    expect(fake.calls[0].options.resume).toBe("abc");
    expect(fake.calls[0].options.sessionId).toBeUndefined();
    host.close();
  });

  test("a turn streams events and ends only at the result message", async () => {
    const fake = fakeSdk({
      onUser(_msg, emit) {
        emit({ type: "stream_event", event: { type: "message_start", message: { id: "m1" } } });
        emit({ type: "stream_event", event: { type: "content_block_delta", delta: { type: "text_delta", text: "Hel" } } });
        emit({ type: "stream_event", event: { type: "content_block_delta", delta: { type: "thinking_delta", thinking: "hmm" } } });
        emit({ type: "assistant", message: { id: "m1", content: [{ type: "text", text: "Hello" }, { type: "tool_use", id: "t1", name: "Bash", input: { command: "ls" } }] } });
        emit({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: "t1", content: [{ type: "text", text: "a.txt" }] }] } });
        emit(ok);
      },
    });
    const host = boot(fake);
    const { state } = await initAndStart(host, "claude-agent-sdk");
    const result = await host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "hi" }] });
    expect(result).toEqual({ terminal: "end_turn" });
    const types = host.eventTypes();
    expect(types).toEqual(["agent_text", "agent_thought", "tool_call", "tool_update", "usage"]);
    // Streamed text is not duplicated from the final assistant message.
    expect(host.events.filter((e: any) => e.event.type === "agent_text")).toHaveLength(1);
    const call = host.events.find((e: any) => e.event.type === "tool_call").event;
    expect(call).toMatchObject({ id: "t1", kind: "execute", title: "ls", status: "in_progress" });
    const update = host.events.find((e: any) => e.event.type === "tool_update").event;
    expect(update).toMatchObject({ id: "t1", status: "completed" });
    expect(host.events.find((e: any) => e.event.type === "usage").event).toMatchObject({ used: 15, size: 200000 });
    host.close();
  });

  test("a second turn while one runs is rejected before dispatch", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "claude-agent-sdk");
    const first = host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "a" }] });
    await until(() => fake.calls[0].userMessages.length === 1, "first message");
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "b" }] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    fake.calls[0].emit(ok);
    expect(await first).toEqual({ terminal: "end_turn" });
    host.close();
  });

  test("stop is a request: the turn resolves from the SDK's terminal result", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "claude-agent-sdk");
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "long" }] });
    await until(() => fake.calls[0].userMessages.length === 1, "message");
    await host.request("turn/stop", { sessionId: state.sessionId });
    expect(fake.calls[0].interrupted).toBe(1);
    expect(await turn).toEqual({ terminal: "cancelled" });
    host.close();
  });

  test("stop with no running turn does not call the SDK", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "claude-agent-sdk");
    await host.request("turn/stop", { sessionId: state.sessionId });
    expect(fake.calls[0].interrupted).toBe(0);
    host.close();
  });

  test("steering is refused before dispatch", async () => {
    const host = boot(fakeSdk());
    const { state } = await initAndStart(host, "claude-agent-sdk");
    await expect(host.request("turn/steer", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("the stream dying mid-turn is Unknown, not a clean failure", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "claude-agent-sdk");
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "x" }] });
    await until(() => fake.calls[0].userMessages.length === 1, "message");
    fake.calls[0].end();
    await expect(turn).rejects.toMatchObject({ data: { phase: "unknown" } });
    // The session is now closed: later sends are provably unsent.
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "y" }] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("permissions go to the host; only an offered option approves", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onPermission: async () => ({ outcome: "selected", optionId: "allow" }) });
    const { state } = await initAndStart(host, "claude-agent-sdk");
    const canUse = fake.calls[0].options.canUseTool;
    const allowed = await canUse("Bash", { command: "rm x" }, { toolUseID: "t9", suggestions: [{ type: "addRules" }] });
    expect(allowed).toEqual({ behavior: "allow", updatedInput: { command: "rm x" } });
    const sent = host.permissions[0];
    expect(sent.toolCall).toMatchObject({ toolCallId: "t9", kind: "execute", title: "rm x" });
    expect(sent.options.map((o: any) => o.id)).toEqual(["allow", "allow_always", "deny"]);
    expect(state.sessionId).toBe(sent.sessionId);
    host.close();
  });

  test("cancelled or unanswered approval denies and interrupts", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onPermission: async () => ({ outcome: "cancelled" }) });
    await initAndStart(host, "claude-agent-sdk");
    const result = await fake.calls[0].options.canUseTool("Write", { file_path: "/a" }, { toolUseID: "t1" });
    expect(result.behavior).toBe("deny");
    expect(result.interrupt).toBe(true);
    host.close();
  });

  test("always-allow carries the SDK's own suggestions", async () => {
    const fake = fakeSdk();
    const suggestions = [{ type: "addRules", rules: [{ toolName: "Bash" }] }];
    const host = boot(fake, { onPermission: async () => ({ outcome: "selected", optionId: "allow_always" }) });
    await initAndStart(host, "claude-agent-sdk");
    const result = await fake.calls[0].options.canUseTool("Bash", { command: "ls" }, { toolUseID: "t1", suggestions });
    expect(result).toMatchObject({ behavior: "allow", updatedPermissions: suggestions });
    host.close();
  });

  test("always-allow is not offered when the SDK suggests nothing", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onPermission: async () => ({ outcome: "selected", optionId: "allow_always" }) });
    await initAndStart(host, "claude-agent-sdk");
    const result = await fake.calls[0].options.canUseTool("Bash", { command: "ls" }, { toolUseID: "t1" });
    expect(host.permissions[0].options.map((o: any) => o.id)).toEqual(["allow", "deny"]);
    expect(result.behavior).toBe("deny");
    host.close();
  });

  test("MCP tool approvals carry provenance in meta", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onPermission: async () => ({ outcome: "selected", optionId: "deny" }) });
    await initAndStart(host, "claude-agent-sdk");
    await fake.calls[0].options.canUseTool("mcp__github__create_issue", { title: "x" }, { toolUseID: "t1" });
    expect(host.permissions[0].meta).toEqual({ is_mcp_tool_approval: true, server: "github", tool: "create_issue" });
    expect(host.permissions[0].toolCall.title).toBe("mcp.github.create_issue");
    expect(host.permissions[0].toolCall.rawInput).toMatchObject({ server: "github", tool: "create_issue" });
    host.close();
  });

  test("AskUserQuestion becomes a host question and returns answers keyed by question text", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onQuestion: async () => ({ action: "accept", content: { question_0: "Blue" } }) });
    await initAndStart(host, "claude-agent-sdk");
    const input = { questions: [{ header: "Color", question: "Pick a color", multiSelect: false, options: [{ label: "Blue", description: "b" }, { label: "Red", description: "r" }] }] };
    const result = await fake.calls[0].options.canUseTool("AskUserQuestion", input, { toolUseID: "q1" });
    expect(result).toMatchObject({ behavior: "allow", updatedInput: { answers: { "Pick a color": "Blue" } } });
    expect(host.questions[0].schema.properties.question_0.oneOf.map((o: any) => o.const)).toEqual(["Blue", "Red"]);
    expect(host.permissions).toHaveLength(0);
    host.close();
  });

  test("an unanswered question denies instead of guessing an answer", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onQuestion: async () => ({ action: "decline" }) });
    await initAndStart(host, "claude-agent-sdk");
    const result = await fake.calls[0].options.canUseTool("AskUserQuestion", { questions: [{ question: "q", options: [] }] }, { toolUseID: "q1" });
    expect(result.behavior).toBe("deny");
    host.close();
  });

  test("MCP elicitation forms are relayed; url mode is declined", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onQuestion: async () => ({ action: "accept", content: { name: "x" } }) });
    await initAndStart(host, "claude-agent-sdk");
    const o = fake.calls[0].options;
    expect(await o.onElicitation({ message: "m", requestedSchema: { type: "object" } })).toEqual({ action: "accept", content: { name: "x" } });
    expect(await o.onElicitation({ message: "m", mode: "url", url: "http://x" })).toEqual({ action: "decline" });
    host.close();
  });

  test("model and effort changes validate against the SDK catalog", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "claude-agent-sdk");
    await host.request("session/set_model", { sessionId: state.sessionId, modelId: "haiku" });
    expect(fake.calls[0].model).toBe("haiku");
    await expect(host.request("session/set_model", { sessionId: state.sessionId, modelId: "nope" })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    // haiku has no effort levels
    await expect(host.request("session/set_config_option", { sessionId: state.sessionId, configId: "reasoning_effort", value: "high" })).rejects.toBeDefined();
    await host.request("session/set_model", { sessionId: state.sessionId, modelId: "default" });
    const res = await host.request("session/set_config_option", { sessionId: state.sessionId, configId: "reasoning_effort", value: "high" });
    expect(fake.calls[0].effort).toBe("high");
    expect(res.configOptions[0].current).toBe("high");
    host.close();
  });

  test("MCP servers are translated per transport", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    await host.request("initialize", { protocol: 1, backend: "claude-agent-sdk" });
    await host.request("session/start", {
      cwd: "/w",
      execution: { mode: "ask", sandbox: "workspace_write" },
      mcpServers: [
        { name: "a", type: "stdio", command: "node", args: ["x"], env: { K: "v" } },
        { name: "b", type: "http", url: "http://h", headers: { A: "1" } },
      ],
    });
    expect(fake.calls[0].options.mcpServers).toEqual({
      a: { type: "stdio", command: "node", args: ["x"], env: { K: "v" } },
      b: { type: "http", url: "http://h", headers: { A: "1" } },
    });
    host.close();
  });

  test("closing the host connection shuts the SDK query down and exits", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    await initAndStart(host, "claude-agent-sdk");
    host.close();
    await until(() => host.exited === 0, "exit");
    expect(fake.calls[0].closed).toBe(true);
  });

  test("requests before initialize and for unknown sessions fail before dispatch", async () => {
    const host = boot(fakeSdk());
    await expect(host.request("session/start", { cwd: "/w", mcpServers: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    await host.request("initialize", { protocol: 1, backend: "claude-agent-sdk" });
    await expect(host.request("turn/send", { sessionId: "ghost", content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    await expect(host.request("initialize", { protocol: 1, backend: "cursor-sdk" })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });
});
