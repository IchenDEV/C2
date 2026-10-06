import { describe, expect, test } from "bun:test";
import { buildCatalog, createCursorAdapter, selectionId, toolKind, toolTitle } from "./adapter.mjs";
import { initAndStart, startHost, until } from "../common/testkit.mjs";
import { deferred } from "../common/content.mjs";

class AgentBusyError extends Error {}
class AuthenticationError extends Error {}

const models = [
  { id: "composer", displayName: "Composer", variants: [{ params: [{ id: "context", value: "200k" }], displayName: "200k", isDefault: true }, { params: [{ id: "context", value: "300k" }], displayName: "300k" }] },
  { id: "plain", displayName: "Plain" },
];

function fakeSdk(opts: { sendError?: Error; steer?: (text: string) => Promise<string>; messages?: any[]; result?: any; hold?: boolean; models?: any } = {}) {
  const log: any = { creates: [], resumes: [], sends: [], cancels: 0, closed: 0 };
  const gate = deferred();
  const sdk = {
    Cursor: { models: { list: async () => opts.models ?? models } },
    Agent: {
      async create(options: any) {
        log.creates.push(options);
        return makeAgent("agent-1");
      },
      async resume(id: string, options: any) {
        log.resumes.push({ id, options });
        return makeAgent(id);
      },
    },
  };
  function makeAgent(agentId: string) {
    return {
      agentId,
      close: () => log.closed++,
      async send(message: any, options: any) {
        log.sends.push({ message, options });
        if (opts.sendError) throw opts.sendError;
        const run: any = {
          id: "run-1",
          async *stream() {
            for (const m of opts.messages ?? []) yield m;
            if (opts.hold) await gate.promise;
          },
          async wait() {
            if (opts.hold) await gate.promise;
            return opts.result ?? { status: opts.hold && run.cancelled ? "cancelled" : "finished" };
          },
          async cancel() {
            log.cancels++;
            run.cancelled = true;
            gate.resolve(undefined);
          },
        };
        if (opts.steer) run.steer = opts.steer;
        return run;
      },
    };
  }
  return { sdk, log };
}

const yolo = { mode: "yolo", sandbox: "danger_full_access" };
const boot = (fake: any, env: any = {}) =>
  startHost({ backend: "cursor-sdk", createAdapter: (host: any) => createCursorAdapter({ sdk: fake.sdk, host, env, sdkVersion: "t" }) });

describe("cursor helpers", () => {
  test("catalog copies variants verbatim and never invents params", () => {
    const { choices, selections } = buildCatalog(models);
    expect(choices.map((c) => c.id)).toEqual(["composer#context=200k", "composer#context=300k", "plain"]);
    expect(selections.get("composer#context=300k")).toEqual({ id: "composer", params: [{ id: "context", value: "300k" }] });
    expect(selections.get("plain")).toEqual({ id: "plain" });
    expect(selectionId("m", [])).toBe("m");
  });
  test("tool vocabulary", () => {
    expect(toolKind("shell")).toBe("execute");
    expect(toolKind("delete")).toBe("delete");
    expect(toolTitle("mcp", { providerIdentifier: "gh", toolName: "pr" })).toBe("mcp.gh.pr");
  });
});

describe("cursor adapter", () => {
  test("capabilities never claim approvals", async () => {
    const host = boot(fakeSdk());
    const { init } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    expect(init.capabilities.toolApproval).toBe("unsupported");
    expect(init.capabilities.steering).toBe(true);
    host.close();
  });

  test.each([
    [{ mode: "ask", sandbox: "workspace_write" }],
    [{ mode: "accept_edits", sandbox: "workspace_write" }],
    [{ mode: "yolo", sandbox: "read_only" }],
  ])("refuses unavailable policy %p before creating an agent", async (execution) => {
    const fake = fakeSdk();
    const host = boot(fake);
    await host.request("initialize", { protocol: 1, backend: "cursor-sdk" });
    await expect(host.request("session/start", { cwd: "/w", mcpServers: [], execution })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    expect(fake.log.creates).toHaveLength(0);
    host.close();
  });

  test("tools restrictions are re-passed on create and resume", async () => {
    const fake = fakeSdk();
    const host = boot(fake, { CURSOR_API_KEY: "k" });
    await host.request("initialize", { protocol: 1, backend: "cursor-sdk" });
    const params = { cwd: "/w", mcpServers: [], execution: yolo, tools: ["read"], disallowedTools: ["task"] };
    await host.request("session/start", params);
    await host.request("session/restore", { ...params, sessionId: "agent-9" });
    expect(fake.log.creates[0]).toMatchObject({ apiKey: "k", tools: ["read"], disallowedTools: ["task"], local: { cwd: "/w" } });
    expect(fake.log.resumes[0].options).toMatchObject({ tools: ["read"], disallowedTools: ["task"] });
    host.close();
  });

  test("restore yields the requested agent id and exposes the catalog", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    await host.request("initialize", { protocol: 1, backend: "cursor-sdk" });
    const state = await host.request("session/restore", { sessionId: "agent-9", cwd: "/w", mcpServers: [], execution: yolo });
    expect(state.sessionId).toBe("agent-9");
    expect(state.models.available.map((m: any) => m.id)).toContain("composer#context=300k");
    host.close();
  });

  test("a turn streams events and ends from run.wait()", async () => {
    const fake = fakeSdk({
      messages: [
        { type: "thinking", text: "hm" },
        { type: "assistant", message: { content: [{ type: "text", text: "Hello" }] } },
        { type: "tool_call", call_id: "c1", name: "shell", status: "running", args: { command: "ls" } },
        { type: "tool_call", call_id: "c1", name: "shell", status: "completed", args: { command: "ls" }, result: "a.txt" },
      ],
    });
    const host = boot(fake);
    const { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    const result = await host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "hi" }] });
    expect(result).toEqual({ terminal: "end_turn" });
    expect(host.eventTypes()).toEqual(["agent_thought", "agent_text", "tool_call", "tool_update"]);
    expect(host.events[2].event).toMatchObject({ id: "c1", kind: "execute", title: "ls", status: "in_progress" });
    expect(host.events[3].event).toMatchObject({ id: "c1", status: "completed" });
    host.close();
  });

  test("images are passed as SDK images", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    await host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "see" }, { type: "image", data: "AAA", mimeType: "image/png" }] });
    expect(fake.log.sends[0].message).toEqual({ text: "see", images: [{ data: "AAA", mimeType: "image/png" }] });
    host.close();
  });

  test("run errors fail the turn; cancelled maps to cancelled", async () => {
    let host = boot(fakeSdk({ result: { status: "error", error: { message: "quota" } } }));
    let { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    expect(await host.request("turn/send", { sessionId: state.sessionId, content: [] })).toEqual({ failed: { message: "quota" } });
    host.close();
    host = boot(fakeSdk({ result: { status: "cancelled" } }));
    ({ state } = await initAndStart(host, "cursor-sdk", { execution: yolo }));
    expect(await host.request("turn/send", { sessionId: state.sessionId, content: [] })).toEqual({ terminal: "cancelled" });
    host.close();
  });

  test("send errors are pre_dispatch only when the SDK proves no run existed", async () => {
    let host = boot(fakeSdk({ sendError: new AgentBusyError("busy") }));
    let { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
    host = boot(fakeSdk({ sendError: new AuthenticationError("bad key") }));
    ({ state } = await initAndStart(host, "cursor-sdk", { execution: yolo }));
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
    host = boot(fakeSdk({ sendError: new Error("socket hang up") }));
    ({ state } = await initAndStart(host, "cursor-sdk", { execution: yolo }));
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "unknown" } });
    host.close();
  });

  test("steer reports delivered only for complete_delivered", async () => {
    for (const [outcome, receipt] of [["complete_delivered", "delivered"], ["revert_to_followup", "unconfirmed"]]) {
      const fake = fakeSdk({ hold: true, steer: async () => outcome as string });
      const host = boot(fake);
      const { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
      const turn = host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "go" }] });
      await until(() => fake.log.sends.length === 1, "send");
      await until(() => true);
      await new Promise((r) => setTimeout(r, 10));
      expect(await host.request("turn/steer", { sessionId: state.sessionId, content: [{ type: "text", text: "more" }] })).toEqual({ receipt });
      await host.request("turn/stop", { sessionId: state.sessionId });
      await turn;
      host.close();
    }
  });

  test("a rejected steer keeps its phase unknown; no steer function is refused before dispatch", async () => {
    let fake = fakeSdk({ hold: true, steer: async () => { throw new Error("connection lost"); } });
    let host = boot(fake);
    let { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    let turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.sends.length === 1, "send");
    await new Promise((r) => setTimeout(r, 10));
    await expect(host.request("turn/steer", { sessionId: state.sessionId, content: [{ type: "text", text: "x" }] })).rejects.toMatchObject({ data: { phase: "unknown" } });
    await host.request("turn/stop", { sessionId: state.sessionId });
    await turn;
    host.close();

    fake = fakeSdk({ hold: true });
    host = boot(fake);
    ({ state } = await initAndStart(host, "cursor-sdk", { execution: yolo }));
    turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.sends.length === 1, "send");
    await new Promise((r) => setTimeout(r, 10));
    await expect(host.request("turn/steer", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    await host.request("turn/stop", { sessionId: state.sessionId });
    await turn;
    host.close();
  });

  test("stop is a request: the turn ends from run.wait()", async () => {
    const fake = fakeSdk({ hold: true });
    const host = boot(fake);
    const { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.sends.length === 1, "send");
    await new Promise((r) => setTimeout(r, 10));
    await host.request("turn/stop", { sessionId: state.sessionId });
    expect(fake.log.cancels).toBe(1);
    expect(await turn).toEqual({ terminal: "cancelled" });
    host.close();
  });

  test("a policy change away from full-access blocks later turns before dispatch", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    host.notify("session/set_execution_policy", { sessionId: state.sessionId, execution: { mode: "ask", sandbox: "workspace_write" } });
    await new Promise((r) => setTimeout(r, 10));
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    expect(fake.log.sends).toHaveLength(0);
    host.notify("session/set_execution_policy", { sessionId: state.sessionId, execution: yolo });
    await new Promise((r) => setTimeout(r, 10));
    expect(await host.request("turn/send", { sessionId: state.sessionId, content: [] })).toEqual({ terminal: "end_turn" });
    host.close();
  });

  test("models are chosen only from the catalog and sent verbatim", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    await expect(host.request("session/set_model", { sessionId: state.sessionId, modelId: "composer#context=999k" })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    await host.request("session/set_model", { sessionId: state.sessionId, modelId: "composer#context=300k" });
    await host.request("turn/send", { sessionId: state.sessionId, content: [] });
    expect(fake.log.sends[0].options.model).toEqual({ id: "composer", params: [{ id: "context", value: "300k" }] });
    host.close();
  });

  test("shutdown cancels a live run and closes agents when the host disconnects", async () => {
    const fake = fakeSdk({ hold: true });
    const host = boot(fake);
    const { state } = await initAndStart(host, "cursor-sdk", { execution: yolo });
    // The host is gone, so the turn's response has nowhere to go; only cleanup is observable.
    void host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.sends.length === 1, "send");
    await new Promise((r) => setTimeout(r, 10));
    host.close();
    await until(() => host.exited === 0, "exit");
    expect(fake.log.cancels).toBe(1);
    expect(fake.log.closed).toBe(1);
  });
});
