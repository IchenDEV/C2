// C2 sidecar IPC: JSON-RPC 2.0, one frame per line (see crates/core/src/connectors/sidecar.rs for
// the protocol and its honesty rules). The sidecar only translates; it owns no C2 state.
//
// stdout is the protocol channel. Nothing else may write to it, so `console.log` is rebound to
// stderr by `reserveStdout()` before any SDK is loaded.

import { createInterface } from "node:readline";

export const MAX_FRAME_BYTES = 16 * 1024 * 1024;

export function reserveStdout() {
  console.log = (...args) => console.error(...args);
  console.info = (...args) => console.error(...args);
}

/** An error the host can classify. `phase: "pre_dispatch"` proves the SDK never saw the work. */
export class SidecarError extends Error {
  constructor(message, { code = -32000, phase = "unknown", data } = {}) {
    super(message);
    this.code = code;
    this.phase = phase;
    this.data = data;
  }
}

export function createPeer({ input, output, requests = {}, notifications = {}, onClose }) {
  const pending = new Map();
  let nextId = 1;
  let closed = false;

  const write = (frame) => {
    if (closed) return false;
    try {
      output.write(`${JSON.stringify(frame)}\n`);
      return true;
    } catch {
      return false;
    }
  };

  const respond = (id, result, error) => {
    if (error) {
      write({
        jsonrpc: "2.0",
        id,
        error: {
          code: error.code ?? -32000,
          message: String(error.message ?? error),
          data: { ...(error.data ?? {}), phase: error.phase ?? "unknown" },
        },
      });
    } else {
      write({ jsonrpc: "2.0", id, result: result ?? null });
    }
  };

  const handle = async (frame) => {
    if (typeof frame.method === "string") {
      if (frame.id === undefined || frame.id === null) {
        // Notifications run inline so event ordering is preserved.
        try {
          await notifications[frame.method]?.(frame.params ?? {});
        } catch (error) {
          console.error(`notification ${frame.method} failed:`, error);
        }
        return;
      }
      const handler = requests[frame.method];
      if (!handler) {
        respond(frame.id, undefined, { code: -32601, message: `method not found: ${frame.method}`, phase: "pre_dispatch" });
        return;
      }
      // Requests run concurrently: a parked turn must not block stop, steer or another session.
      handler(frame.params ?? {}).then(
        (result) => respond(frame.id, result),
        (error) => respond(frame.id, undefined, error),
      );
      return;
    }
    const entry = pending.get(frame.id);
    if (!entry) return;
    pending.delete(frame.id);
    if (frame.error) entry.reject(Object.assign(new Error(frame.error.message), { code: frame.error.code }));
    else entry.resolve(frame.result);
  };

  const lines = createInterface({ input, crlfDelay: Infinity });
  lines.on("line", (line) => {
    if (!line.trim()) return;
    if (Buffer.byteLength(line) > MAX_FRAME_BYTES) {
      console.error("dropping oversized frame");
      return;
    }
    let frame;
    try {
      frame = JSON.parse(line);
    } catch (error) {
      console.error("dropping non-JSON frame:", error.message);
      return;
    }
    // Chain so notifications stay ordered relative to each other.
    chain = chain.then(() => handle(frame));
  });
  let chain = Promise.resolve();
  lines.on("close", () => {
    closed = true;
    for (const entry of pending.values()) entry.reject(new Error("host connection closed"));
    pending.clear();
    onClose?.();
  });

  return {
    notify(method, params) {
      write({ jsonrpc: "2.0", method, params });
    },
    request(method, params) {
      if (closed) return Promise.reject(new Error("host connection closed"));
      const id = nextId++;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        if (!write({ jsonrpc: "2.0", id, method, params })) {
          pending.delete(id);
          reject(new Error("host connection closed"));
        }
      });
    },
    get closed() {
      return closed;
    },
  };
}
