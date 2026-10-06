// Test harness: drives a sidecar in-process over PassThrough pipes, playing the C2 host.

import { PassThrough } from "node:stream";
import { createInterface } from "node:readline";
import { PROTOCOL_VERSION, runSidecar } from "./runtime.mjs";

export function startHost({ backend, createAdapter, onPermission, onQuestion }) {
  const toSidecar = new PassThrough();
  const fromSidecar = new PassThrough();
  const events = [];
  const permissions = [];
  const questions = [];
  const pending = new Map();
  let nextId = 1;
  let exited = null;

  runSidecar({ backend, createAdapter, input: toSidecar, output: fromSidecar, exit: (code) => (exited = code) });

  const send = (frame) => toSidecar.write(`${JSON.stringify(frame)}\n`);
  createInterface({ input: fromSidecar }).on("line", async (line) => {
    const frame = JSON.parse(line);
    if (frame.method === "session/event") {
      events.push(frame.params);
    } else if (frame.method === "host/permission") {
      permissions.push(frame.params);
      const reply = (await onPermission?.(frame.params)) ?? { outcome: "cancelled" };
      send({ jsonrpc: "2.0", id: frame.id, result: reply });
    } else if (frame.method === "host/question") {
      questions.push(frame.params);
      const reply = (await onQuestion?.(frame.params)) ?? { action: "decline" };
      send({ jsonrpc: "2.0", id: frame.id, result: reply });
    } else if (pending.has(frame.id)) {
      const entry = pending.get(frame.id);
      pending.delete(frame.id);
      if (frame.error) entry.reject(Object.assign(new Error(frame.error.message), frame.error));
      else entry.resolve(frame.result);
    }
  });

  return {
    events,
    permissions,
    questions,
    get exited() {
      return exited;
    },
    request(method, params = {}) {
      const id = nextId++;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        send({ jsonrpc: "2.0", id, method, params });
      });
    },
    notify(method, params = {}) {
      send({ jsonrpc: "2.0", method, params });
    },
    initialize: (extra = {}) => undefined,
    close() {
      toSidecar.end();
    },
    eventTypes: () => events.map((e) => e.event.type),
  };
}

export const execution = { mode: "ask", sandbox: "workspace_write" };

export async function initAndStart(host, backend, extra = {}) {
  const init = await host.request("initialize", { protocol: PROTOCOL_VERSION, backend, clientVersion: "test" });
  const state = await host.request("session/start", { cwd: "/tmp/work", mcpServers: [], execution, ...extra });
  return { init, state };
}

export const tick = (ms = 5) => new Promise((resolve) => setTimeout(resolve, ms));
export async function until(predicate, label = "condition", timeout = 2000) {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > timeout) throw new Error(`timed out waiting for ${label}`);
    await tick();
  }
}
