// Wires an adapter (one official SDK) to the C2 sidecar protocol. Keep this file free of SDK and
// provider knowledge: it validates the envelope, serializes nothing, and holds no session state.

import { createPeer, reserveStdout, SidecarError } from "./ipc.mjs";

export const PROTOCOL_VERSION = 1;
export const MAX_EVENT_TEXT = 256 * 1024;

export function clip(text, limit = MAX_EVENT_TEXT) {
  const value = typeof text === "string" ? text : String(text ?? "");
  return value.length > limit ? value.slice(0, limit) : value;
}

/**
 * @param {object} options
 * @param {(host: object) => object} options.createAdapter  receives the host bridge
 * @param {string} options.backend  stable backend name the host expects
 */
export function runSidecar({ backend, createAdapter, input = process.stdin, output = process.stdout, exit = (code) => process.exit(code) }) {
  let initialized = false;
  const sessions = new Set();
  let adapter;
  const requests = {};

  const peer = createPeer({
    input,
    output,
    onClose: async () => {
      // The host is gone: stop SDK work and exit so the sidecar never outlives its owner.
      try {
        await adapter?.shutdown?.();
      } finally {
        exit(0);
      }
    },
    notifications: {
      "session/set_execution_policy": async (params) => {
        if (sessions.has(params.sessionId)) await adapter.setExecutionPolicy?.(params);
      },
      shutdown: async () => {
        try {
          await adapter?.shutdown?.();
        } finally {
          exit(0);
        }
      },
    },
    requests,
  });

  const host = {
    event(sessionId, event) {
      peer.notify("session/event", { sessionId, event });
    },
    async permission(sessionId, request) {
      try {
        return await peer.request("host/permission", { sessionId, ...request });
      } catch {
        // Unreachable host is a denial, never an approval.
        return { outcome: "cancelled" };
      }
    },
    async question(sessionId, request) {
      try {
        return await peer.request("host/question", { sessionId, ...request });
      } catch {
        return { action: "cancel" };
      }
    },
  };
  adapter = createAdapter(host);

  const requireInit = () => {
    if (!initialized) throw new SidecarError("sidecar is not initialized", { code: -32002, phase: "pre_dispatch" });
  };
  const requireSession = (params) => {
    requireInit();
    if (!sessions.has(params.sessionId)) {
      throw new SidecarError("unknown session", { code: -32003, phase: "pre_dispatch" });
    }
  };

  Object.assign(requests, {
    async initialize(params) {
      if (params.protocol !== PROTOCOL_VERSION) {
        throw new SidecarError(`unsupported protocol ${params.protocol}`, { code: -32004, phase: "pre_dispatch" });
      }
      if (params.backend !== backend) {
        throw new SidecarError(`this sidecar serves ${backend}, not ${params.backend}`, { code: -32004, phase: "pre_dispatch" });
      }
      const result = await adapter.initialize(params);
      initialized = true;
      return { protocol: PROTOCOL_VERSION, backend, ...result };
    },
    async "session/start"(params) {
      requireInit();
      const state = await adapter.startSession(params);
      sessions.add(state.sessionId);
      return state;
    },
    async "session/restore"(params) {
      requireInit();
      const state = await adapter.restoreSession(params);
      sessions.add(state.sessionId);
      return state;
    },
    async "turn/send"(params) {
      requireSession(params);
      return adapter.sendTurn(params);
    },
    async "turn/steer"(params) {
      requireSession(params);
      return adapter.steer(params);
    },
    async "turn/stop"(params) {
      requireSession(params);
      await adapter.stop(params);
      return {};
    },
    async "session/set_model"(params) {
      requireSession(params);
      await adapter.setModel(params);
      return {};
    },
    async "session/set_mode"(params) {
      requireSession(params);
      if (!adapter.setMode) throw new SidecarError("this backend has no session modes", { code: -32601, phase: "pre_dispatch" });
      await adapter.setMode(params);
      return {};
    },
    async "session/set_config_option"(params) {
      requireSession(params);
      return adapter.setConfigOption(params);
    },
  });
  return { peer, host, requests };
}

/** Bind the request table into a live peer. Split from `runSidecar` so tests can drive a peer. */
export function serve(options) {
  reserveStdout();
  const created = runSidecar(options);
  return created;
}
