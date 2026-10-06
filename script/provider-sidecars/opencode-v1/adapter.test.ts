import { describe, expect, test } from "bun:test";
import { createOpenCodeV1Adapter, permissionKind, toolKind, toolTitle } from "./adapter.mjs";
import { initAndStart, startHost, until } from "../common/testkit.mjs";
import { createInputQueue } from "../common/content.mjs";

const providers = {
  providers: [{ id: "anthropic", name: "Anthropic", models: { sonnet: { id: "sonnet", name: "Sonnet", status: "active", limit: { context: 200000 } }, old: { id: "old", name: "Old", status: "deprecated" } } }],
  default: { anthropic: "sonnet" },
};

function fakeSdk(opts: { promptStatus?: number; getFails?: boolean } = {}) {
  const log: any = { serverOptions: null, mcp: [], prompts: [], aborts: 0, replies: [], clients: [], closed: false };
  const queues: any[] = [];
  const sdk = {
    async createOpencodeServer(options: any) {
      log.serverOptions = options;
      return { url: "http://127.0.0.1:1", close: () => (log.closed = true) };
    },
    createOpencodeClient(config: any) {
      const queue = createInputQueue();
      queues.push(queue);
      log.clients.push(config);
      return {
        config: { providers: async () => ({ data: providers }) },
        mcp: { add: async (req: any) => (log.mcp.push(req.body), { data: true }) },
        session: {
          create: async () => ({ data: { id: "ses_1" } }),
          get: async (req: any) => (opts.getFails ? { error: { name: "NotFound", data: { message: "nope" } }, response: { status: 404 } } : { data: { id: req.path.id } }),
          promptAsync: async (req: any) => {
            log.prompts.push(req);
            if (opts.promptStatus) return { error: { name: "X", data: { message: "bad" } }, response: { status: opts.promptStatus } };
            return { data: true };
          },
          abort: async () => (log.aborts++, { data: true }),
        },
        event: { subscribe: async () => ({ stream: queue }) },
        postSessionIdPermissionsPermissionId: async (req: any) => (log.replies.push(req), { data: true }),
      };
    },
  };
  return { sdk, log, emit: (e: any) => queues.forEach((q) => q.push(e)), endStream: () => queues.forEach((q) => q.close()) };
}

const ask = { mode: "ask", sandbox: "workspace_write" };
const boot = (fake: any, handlers: any = {}) =>
  startHost({ backend: "opencode-v1", createAdapter: (host: any) => createOpenCodeV1Adapter({ sdk: fake.sdk, host, env: {}, sdkVersion: "t" }), ...handlers });

const userMsg = { type: "message.updated", properties: { info: { id: "u1", sessionID: "ses_1", role: "user" } } };
const asstMsg = (extra: any = {}) => ({ type: "message.updated", properties: { info: { id: "a1", sessionID: "ses_1", role: "assistant", providerID: "anthropic", modelID: "sonnet", cost: 0.1, tokens: { input: 10, output: 5, reasoning: 0, cache: { read: 0, write: 0 } }, ...extra } } });
const part = (p: any, delta?: string) => ({ type: "message.part.updated", properties: { part: { sessionID: "ses_1", messageID: "a1", id: "p1", ...p }, delta } });
const idle = { type: "session.idle", properties: { sessionID: "ses_1" } };

describe("opencode v1 helpers", () => {
  test("vocabulary", () => {
    expect(permissionKind("bash")).toBe("execute");
    expect(toolKind("edit")).toBe("edit");
    expect(toolTitle("bash", { command: "ls" })).toBe("ls");
    expect(toolTitle("read", { filePath: "/a" })).toBe("read /a");
  });
});

describe("opencode v1 adapter", () => {
  test("spawns its own server with every permission set to ask; no steering", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { init, state } = await initAndStart(host, "opencode-v1", { execution: ask });
    expect(fake.log.serverOptions.config.permission).toEqual({ edit: "ask", bash: "ask", webfetch: "ask", doom_loop: "ask", external_directory: "ask" });
    expect(init.capabilities.steering).toBe(false);
    expect(state.sessionId).toBe("ses_1");
    expect(state.models.available.map((m: any) => m.id)).toEqual(["anthropic/sonnet"]);
    expect(state.models.current).toBe("anthropic/sonnet");
    expect(fake.log.clients[0].directory).toBe("/tmp/work");
    host.close();
  });

  test("restore verifies the session exists before claiming it", async () => {
    const host = boot(fakeSdk({ getFails: true }));
    await host.request("initialize", { protocol: 1, backend: "opencode-v1" });
    await expect(host.request("session/restore", { sessionId: "ses_9", cwd: "/w", mcpServers: [], execution: ask })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("MCP servers are registered per transport", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    await host.request("initialize", { protocol: 1, backend: "opencode-v1" });
    await host.request("session/start", { cwd: "/w", execution: ask, mcpServers: [{ name: "a", type: "stdio", command: "node", args: ["x"], env: { K: "v" } }, { name: "b", type: "http", url: "http://h", headers: {} }] });
    expect(fake.log.mcp).toEqual([
      { name: "a", config: { type: "local", command: ["node", "x"], environment: { K: "v" }, enabled: true } },
      { name: "b", config: { type: "remote", url: "http://h", headers: {}, enabled: true } },
    ]);
    host.close();
  });

  test("a turn streams assistant parts only, and ends at idle", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v1", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "hi" }] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    expect(fake.log.prompts[0].body.model).toEqual({ providerID: "anthropic", modelID: "sonnet" });
    fake.emit(userMsg);
    fake.emit({ type: "message.part.updated", properties: { part: { sessionID: "ses_1", messageID: "u1", id: "pu", type: "text", text: "hi" } } });
    fake.emit(part({ type: "text", text: "He" }, "He"));
    fake.emit(asstMsg());
    fake.emit(part({ type: "text", text: "Hello" }, "llo"));
    fake.emit(part({ id: "p2", type: "reasoning", text: "hmm" }, "hmm"));
    fake.emit(part({ id: "p3", type: "tool", callID: "c1", tool: "bash", state: { status: "running", input: { command: "ls" } } }));
    fake.emit(part({ id: "p3", type: "tool", callID: "c1", tool: "bash", state: { status: "completed", input: { command: "ls" }, output: "a.txt", title: "ls" } }));
    fake.emit(idle);
    expect(await turn).toEqual({ terminal: "end_turn" });
    // The part that arrived before its message's role was known is delivered once the role is.
    const texts = host.events.filter((e: any) => e.event.type === "agent_text").map((e: any) => e.event.text);
    expect(texts).toEqual(["He", "llo"]);
    expect(host.eventTypes()).toContain("usage");
    const call = host.events.find((e: any) => e.event.type === "tool_call").event;
    expect(call).toMatchObject({ id: "c1", kind: "execute", title: "ls" });
    expect(host.events.find((e: any) => e.event.type === "tool_update").event).toMatchObject({ id: "c1", status: "completed" });
    host.close();
  });

  test("an idle from before this turn's activity does not end the turn", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v1", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    fake.emit(idle);
    let settled = false;
    turn.then(() => (settled = true));
    await new Promise((r) => setTimeout(r, 20));
    expect(settled).toBe(false);
    fake.emit({ type: "session.status", properties: { sessionID: "ses_1", status: { type: "busy" } } });
    fake.emit(idle);
    expect(await turn).toEqual({ terminal: "end_turn" });
    host.close();
  });

  test("errors map to failed / cancelled / max_tokens", async () => {
    for (const [error, expected] of [
      [{ name: "APIError", data: { message: "rate limited" } }, { failed: { message: "rate limited" } }],
      [{ name: "MessageAbortedError", data: { message: "aborted" } }, { terminal: "cancelled" }],
      [{ name: "MessageOutputLengthError", data: {} }, { terminal: "max_tokens" }],
    ] as const) {
      const fake = fakeSdk();
      const host = boot(fake);
      const { state } = await initAndStart(host, "opencode-v1", { execution: ask });
      const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
      await until(() => fake.log.prompts.length === 1, "prompt");
      fake.emit({ type: "session.error", properties: { sessionID: "ses_1", error } });
      fake.emit(idle);
      expect(await turn).toEqual(expected as any);
      host.close();
    }
  });

  test("prompt rejection: 4xx is pre_dispatch, 5xx is unknown", async () => {
    for (const [status, phase] of [[400, "pre_dispatch"], [500, "unknown"]] as const) {
      const host = boot(fakeSdk({ promptStatus: status }));
      const { state } = await initAndStart(host, "opencode-v1", { execution: ask });
      await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase } });
      // A refused prompt leaves no turn behind.
      host.close();
    }
  });

  test("losing the event stream mid-turn is unknown and blocks further sends", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v1", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    fake.endStream();
    await expect(turn).rejects.toMatchObject({ data: { phase: "unknown" } });
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("permissions: once / always / reject, anything else rejects", async () => {
    for (const [reply, response] of [
      [{ outcome: "selected", optionId: "allow" }, "once"],
      [{ outcome: "selected", optionId: "allow_always" }, "always"],
      [{ outcome: "selected", optionId: "deny" }, "reject"],
      [{ outcome: "cancelled" }, "reject"],
      [{ outcome: "selected", optionId: "bogus" }, "reject"],
    ] as const) {
      const fake = fakeSdk();
      const host = boot(fake, { onPermission: async () => reply });
      await initAndStart(host, "opencode-v1", { execution: ask });
      fake.emit({ type: "permission.updated", properties: { id: "perm1", type: "bash", sessionID: "ses_1", messageID: "a1", callID: "c1", title: "rm -rf x", metadata: {}, time: { created: 1 } } });
      await until(() => fake.log.replies.length === 1, "reply");
      expect(fake.log.replies[0]).toMatchObject({ path: { id: "ses_1", permissionID: "perm1" }, body: { response } });
      expect(host.permissions[0].toolCall).toMatchObject({ toolCallId: "c1", kind: "execute", title: "rm -rf x" });
      host.close();
    }
  });

  test("permissions for other sessions are ignored", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { onPermission: async () => ({ outcome: "selected", optionId: "allow" }) });
    await initAndStart(host, "opencode-v1", { execution: ask });
    fake.emit({ type: "permission.updated", properties: { id: "p", type: "bash", sessionID: "other", title: "x", metadata: {} } });
    await new Promise((r) => setTimeout(r, 20));
    expect(fake.log.replies).toHaveLength(0);
    host.close();
  });

  test("stop only requests an abort; the terminal comes from events", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v1", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    await host.request("turn/stop", { sessionId: state.sessionId });
    expect(fake.log.aborts).toBe(1);
    let settled = false;
    turn.then(() => (settled = true));
    await new Promise((r) => setTimeout(r, 20));
    expect(settled).toBe(false);
    fake.emit({ type: "session.error", properties: { sessionID: "ses_1", error: { name: "MessageAbortedError", data: { message: "x" } } } });
    fake.emit(idle);
    expect(await turn).toEqual({ terminal: "cancelled" });
    host.close();
  });

  test("model selection validates against the catalog and steering is refused", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v1", { execution: ask });
    await expect(host.request("session/set_model", { sessionId: state.sessionId, modelId: "anthropic/old" })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    await expect(host.request("turn/steer", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("host disconnect closes the server it spawned", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    await initAndStart(host, "opencode-v1", { execution: ask });
    host.close();
    await until(() => host.exited === 0, "exit");
    expect(fake.log.closed).toBe(true);
  });
});
