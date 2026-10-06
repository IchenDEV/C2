import { describe, expect, test } from "bun:test";
import { createOpenCodeV2Adapter, formToSchema, toolKind } from "./adapter.mjs";
import { initAndStart, startHost, until } from "../common/testkit.mjs";
import { createInputQueue } from "../common/content.mjs";

const models = [
  { id: "anthropic/sonnet", providerID: "anthropic", modelID: "sonnet", name: "Sonnet", enabled: true, status: "active", limit: { context: 200000 }, variants: [{ id: "high" }] },
  { id: "x/old", providerID: "x", modelID: "old", name: "Old", enabled: true, status: "deprecated", limit: { context: 1 }, variants: [] },
];
const askAll = { action: "*", resource: "*", effect: "ask" };

function fakeSdk(opts: { promptError?: any; existing?: any; hold?: boolean } = {}) {
  const log: any = { created: [], prompts: [], interrupts: 0, replies: [], forms: [], formCancels: [], mcp: [], closed: false, createOptions: null, switched: [] };
  const queue = createInputQueue();
  let inbox = 0;
  const oc = {
    mcp: { add: async (i: any) => log.mcp.push(i) },
    sessions: {
      create: async (i: any) => (log.created.push(i), { id: "ses_1", permissions: i.permissions, location: i.location }),
      get: async ({ sessionID }: any) => opts.existing ?? { id: sessionID, permissions: [askAll] },
      prompt: async (i: any) => {
        log.prompts.push(i);
        if (opts.promptError) throw opts.promptError;
        return { id: `inbox_${++inbox}`, delivery: i.delivery };
      },
      interrupt: async () => (log.interrupts++, { interrupted: true }),
      switchModel: async (i: any) => log.switched.push(i),
      form: { reply: async (i: any) => log.forms.push(i), cancel: async (i: any) => log.formCancels.push(i) },
    },
    model: { list: async () => ({ data: models }) },
    permission: { reply: async (i: any) => log.replies.push(i) },
    events: { subscribe: () => queue },
    close: async () => (log.closed = true),
  };
  const sdk = { OpenCode: { create: async (o: any) => ((log.createOptions = o), oc) } };
  return { sdk, log, emit: (type: string, data: any) => queue.push({ type, data }), end: () => queue.close() };
}

const ask = { mode: "ask", sandbox: "workspace_write" };
const boot = (fake: any, env: any = {}, handlers: any = {}) =>
  startHost({ backend: "opencode-v2", createAdapter: (host: any) => createOpenCodeV2Adapter({ sdk: fake.sdk, host, env, sdkVersion: "t" }), ...handlers });
const sid = { sessionID: "ses_1" };

describe("opencode v2 helpers", () => {
  test("forms become JSON schemas and skip hidden/external fields", () => {
    const schema = formToSchema([
      { key: "a", type: "string", title: "A", required: true, options: [{ value: "x", label: "X" }] },
      { key: "b", type: "multiselect", options: [{ value: "1" }] },
      { key: "c", type: "external", url: "http://x" },
      { key: "d", type: "boolean", hidden: true },
    ]);
    expect(Object.keys(schema.properties)).toEqual(["a", "b"]);
    expect(schema.properties.a.oneOf[0]).toMatchObject({ const: "x", title: "X" });
    expect(schema.required).toEqual(["a"]);
    expect(toolKind("bash")).toBe("execute");
  });
});

describe("opencode v2 adapter", () => {
  test("resume is advertised only with a persistent database", async () => {
    let host = boot(fakeSdk());
    let { init } = await initAndStart(host, "opencode-v2", { execution: ask });
    expect(init.capabilities.resume).toBe(false);
    host.close();
    const fake = fakeSdk();
    host = boot(fake, { CODETWO_OPENCODE_V2_DB: "/tmp/x.db" });
    ({ init } = await initAndStart(host, "opencode-v2", { execution: ask }));
    expect(init.capabilities.resume).toBe(true);
    expect(fake.log.createOptions).toEqual({ database: { path: "/tmp/x.db" } });
    host.close();
  });

  test("sessions start with an ask-everything ruleset and expose the catalog", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    expect(fake.log.created[0]).toMatchObject({ location: { directory: "/tmp/work" }, permissions: [askAll] });
    expect(state.models.available.map((m: any) => m.id)).toEqual(["anthropic/sonnet", "anthropic/sonnet#high"]);
    host.close();
  });

  test("restore refuses a session without host-mediated permissions, and without a database", async () => {
    let host = boot(fakeSdk(), {});
    await host.request("initialize", { protocol: 1, backend: "opencode-v2" });
    await expect(host.request("session/restore", { sessionId: "s", cwd: "/w", mcpServers: [], execution: ask })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
    host = boot(fakeSdk({ existing: { id: "s", permissions: [] } }), { CODETWO_OPENCODE_V2_DB: "/x" });
    await host.request("initialize", { protocol: 1, backend: "opencode-v2" });
    await expect(host.request("session/restore", { sessionId: "s", cwd: "/w", mcpServers: [], execution: ask })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
    host = boot(fakeSdk(), { CODETWO_OPENCODE_V2_DB: "/x" });
    await host.request("initialize", { protocol: 1, backend: "opencode-v2" });
    const state = await host.request("session/restore", { sessionId: "s", cwd: "/w", mcpServers: [], execution: ask });
    expect(state.sessionId).toBe("s");
    host.close();
  });

  test("a turn streams events and ends at execution.succeeded", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "text", text: "hi" }] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    expect(fake.log.prompts[0]).toMatchObject({ sessionID: "ses_1", text: "hi", delivery: "queue" });
    fake.emit("session.execution.started", sid);
    fake.emit("session.text.delta", { ...sid, delta: "Hel" });
    fake.emit("session.reasoning.delta", { ...sid, delta: "hm" });
    fake.emit("session.tool.input.started", { ...sid, id: "t1", name: "bash" });
    fake.emit("session.tool.called", { ...sid, id: "t1", input: { command: "ls" } });
    fake.emit("session.tool.success", { ...sid, id: "t1", content: [{ type: "text", text: "a.txt" }] });
    fake.emit("session.usage.updated", { ...sid, cost: 0.1, tokens: { input: 10, output: 5, reasoning: 0, cache: { read: 0, write: 0 } } });
    fake.emit("session.execution.succeeded", sid);
    expect(await turn).toEqual({ terminal: "end_turn" });
    expect(host.eventTypes()).toEqual(["agent_text", "agent_thought", "tool_call", "tool_update", "usage"]);
    expect(host.events[2].event).toMatchObject({ id: "t1", kind: "execute" });
    host.close();
  });

  test("a stale terminal before this turn's activity is ignored", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    fake.emit("session.execution.succeeded", sid);
    let settled = false;
    turn.then(() => (settled = true));
    await new Promise((r) => setTimeout(r, 20));
    expect(settled).toBe(false);
    fake.emit("session.execution.started", sid);
    fake.emit("session.execution.failed", { ...sid, error: { type: "x", message: "overloaded" } });
    expect(await turn).toEqual({ failed: { message: "overloaded" } });
    host.close();
  });

  test("interrupt is a request; the turn ends on session.execution.interrupted", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    fake.emit("session.execution.started", sid);
    await host.request("turn/stop", { sessionId: state.sessionId });
    expect(fake.log.interrupts).toBe(1);
    let settled = false;
    turn.then(() => (settled = true));
    await new Promise((r) => setTimeout(r, 20));
    expect(settled).toBe(false);
    fake.emit("session.execution.interrupted", { ...sid, reason: "user" });
    expect(await turn).toEqual({ terminal: "cancelled" });
    host.close();
  });

  test("steer is delivered only on session.inbox.delivered, otherwise unconfirmed", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    fake.emit("session.execution.started", sid);
    const steer = host.request("turn/steer", { sessionId: state.sessionId, content: [{ type: "text", text: "also" }] });
    await until(() => fake.log.prompts.length === 2, "steer prompt");
    expect(fake.log.prompts[1].delivery).toBe("steer");
    fake.emit("session.inbox.delivered", { ...sid, inboxID: "inbox_2" });
    expect(await steer).toEqual({ receipt: "delivered" });

    const cancelled = host.request("turn/steer", { sessionId: state.sessionId, content: [{ type: "text", text: "x" }] });
    await until(() => fake.log.prompts.length === 3, "steer prompt 2");
    fake.emit("session.inbox.cancelled", { ...sid, inboxID: "inbox_3" });
    expect(await cancelled).toEqual({ receipt: "declined" });
    fake.emit("session.execution.succeeded", sid);
    await turn;
    host.close();
  });

  test("steer with no running turn is refused before dispatch", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    await expect(host.request("turn/steer", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("prompt errors are pre_dispatch only for an explicit 4xx", async () => {
    for (const [error, phase] of [
      [Object.assign(new Error("UnexpectedStatus 422"), { name: "ClientError", reason: "UnexpectedStatus" }), "pre_dispatch"],
      [Object.assign(new Error("UnexpectedStatus 503"), { name: "ClientError", reason: "UnexpectedStatus" }), "unknown"],
      [Object.assign(new Error("fetch failed"), { name: "ClientError", reason: "Transport" }), "unknown"],
    ] as const) {
      const host = boot(fakeSdk({ promptError: error }));
      const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
      await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase } });
      host.close();
    }
  });

  test("losing the event stream mid-turn is unknown and blocks later sends", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    const turn = host.request("turn/send", { sessionId: state.sessionId, content: [] });
    await until(() => fake.log.prompts.length === 1, "prompt");
    fake.end();
    await expect(turn).rejects.toMatchObject({ data: { phase: "unknown" } });
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("permissions map to once/always/reject; dismissal rejects", async () => {
    for (const [reply, decision] of [
      [{ outcome: "selected", optionId: "allow" }, "once"],
      [{ outcome: "selected", optionId: "allow_always" }, "always"],
      [{ outcome: "selected", optionId: "deny" }, "reject"],
      [{ outcome: "cancelled" }, "reject"],
    ] as const) {
      const fake = fakeSdk();
      const host = boot(fake, {}, { onPermission: async () => reply });
      await initAndStart(host, "opencode-v2", { execution: ask });
      fake.emit("permission.asked", { id: "req1", ...sid, action: "bash", resources: ["rm -rf x"], message: "run rm", source: { type: "tool", messageID: "m", id: "call1" } });
      await until(() => fake.log.replies.length === 1, "reply");
      expect(fake.log.replies[0]).toEqual({ sessionID: "ses_1", requestID: "req1", decision });
      expect(host.permissions[0].toolCall).toMatchObject({ toolCallId: "call1", kind: "execute", title: "run rm" });
      host.close();
    }
  });

  test("permissions for other sessions are ignored", async () => {
    const fake = fakeSdk();
    const host = boot(fake, {}, { onPermission: async () => ({ outcome: "selected", optionId: "allow" }) });
    await initAndStart(host, "opencode-v2", { execution: ask });
    fake.emit("permission.asked", { id: "r", sessionID: "other", action: "bash", resources: [] });
    await new Promise((r) => setTimeout(r, 20));
    expect(fake.log.replies).toHaveLength(0);
    host.close();
  });

  test("forms are relayed as questions; unanswered forms are cancelled, not guessed", async () => {
    let fake = fakeSdk();
    let host = boot(fake, {}, { onQuestion: async () => ({ action: "accept", content: { a: "x" } }) });
    await initAndStart(host, "opencode-v2", { execution: ask });
    fake.emit("form.created", { form: { id: "f1", ...sid, title: "Pick", fields: [{ key: "a", type: "string", options: [{ value: "x", label: "X" }] }] } });
    await until(() => fake.log.forms.length === 1, "form reply");
    expect(fake.log.forms[0]).toEqual({ sessionID: "ses_1", formID: "f1", answer: { a: "x" } });
    host.close();

    fake = fakeSdk();
    host = boot(fake, {}, { onQuestion: async () => ({ action: "decline" }) });
    await initAndStart(host, "opencode-v2", { execution: ask });
    fake.emit("form.created", { form: { id: "f2", ...sid, title: "Pick", fields: [{ key: "a", type: "string" }] } });
    await until(() => fake.log.formCancels.length === 1, "form cancel");
    expect(fake.log.forms).toHaveLength(0);
    host.close();
  });

  test("model switching uses the catalog and keeps variants separate", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    await host.request("session/set_model", { sessionId: state.sessionId, modelId: "anthropic/sonnet#high" });
    expect(fake.log.switched[0]).toEqual({ sessionID: "ses_1", model: { providerID: "anthropic", id: "sonnet", variant: "high" } });
    await expect(host.request("session/set_model", { sessionId: state.sessionId, modelId: "x/old" })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    host.close();
  });

  test("images are refused before dispatch until verified", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    const { state } = await initAndStart(host, "opencode-v2", { execution: ask });
    await expect(host.request("turn/send", { sessionId: state.sessionId, content: [{ type: "image", data: "A", mimeType: "image/png" }] })).rejects.toMatchObject({ data: { phase: "pre_dispatch" } });
    expect(fake.log.prompts).toHaveLength(0);
    host.close();
  });

  test("host disconnect closes the embedded host", async () => {
    const fake = fakeSdk();
    const host = boot(fake);
    await initAndStart(host, "opencode-v2", { execution: ask });
    host.close();
    await until(() => host.exited === 0, "exit");
    expect(fake.log.closed).toBe(true);
  });
});
