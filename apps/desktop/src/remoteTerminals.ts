/**
 * Terminals that run on a paired remote C2 server.
 *
 * The renderer drives every terminal through `terminal.spawn/write/resize/kill` plus `pty-*`
 * events. A remote machine speaks a different, per-terminal WebSocket protocol (`/ws/terminal`),
 * so this module adapts one to the other: the renderer keeps its own terminal id, and each id owns
 * one socket to the machine that runs the shell. The shell itself lives on the server and survives
 * this app, exactly like a local terminal; attaching again replays its screen.
 */
import type { EnvironmentRegistry } from "./remoteEnvironments";

export interface TerminalSocketLike {
  close(): void;
  onclose: WebSocket["onclose"];
  onerror: WebSocket["onerror"];
  onmessage: WebSocket["onmessage"];
  onopen: WebSocket["onopen"];
  send(data: string): void;
}

export interface RemoteTerminalDependencies {
  registry: Pick<EnvironmentRegistry, "bearer" | "get" | "reportStatus">;
  fetch(input: string, init?: RequestInit): Promise<Response>;
  createSocket(url: string): TerminalSocketLike;
  reconnectDelayMs: number;
}

export interface RemoteTerminalAttach {
  created: boolean;
  restore: string;
}

export interface RemoteTerminalSpawn {
  environmentId: string;
  /** Terminal id chosen by the renderer. */
  id: string;
  /** Absolute path on the remote machine. */
  cwd: string | null;
  rows: number;
  cols: number;
  tmuxSession: string | null;
  /** Echoed on every event so the panel's realm filter keeps matching. */
  projectPath: string | null;
}

export interface RemoteTerminals {
  owns(id: string): boolean;
  spawn(request: RemoteTerminalSpawn): Promise<RemoteTerminalAttach>;
  write(id: string, data: string): void;
  resize(id: string, rows: number, cols: number): void;
  kill(id: string): void;
  subscribe(name: string, listener: (payload: unknown) => void): () => void;
}

const MAX_REMOTE_ID = 64;
const VALID_REMOTE_ID = /^[A-Za-z0-9._-]+$/;

/** Renderer ids are free-form; the server accepts short `[A-Za-z0-9._-]` ids only. */
export function remoteTerminalId(id: string): string {
  const prefixed = `c2d-${id}`;
  if (prefixed.length <= MAX_REMOTE_ID && VALID_REMOTE_ID.test(prefixed)) {
    return prefixed;
  }
  // Stable digest (FNV-1a, two seeds) so the same renderer id always reattaches to the same shell.
  let a = 0x81_1c_9d_c5;
  let b = 0x12_34_56_78;
  for (let index = 0; index < id.length; index += 1) {
    const code = id.charCodeAt(index);
    a = Math.imul(a ^ code, 0x01_00_01_93) >>> 0;
    b = Math.imul(b ^ code, 0x01_00_01_93) >>> 0;
  }
  const readable = id.replaceAll(/[^A-Za-z0-9]/g, "").slice(0, 24);
  return `c2d-${readable}-${a.toString(16)}${b.toString(16)}`;
}

interface Entry {
  request: RemoteTerminalSpawn;
  rows: number;
  cols: number;
  socket: TerminalSocketLike | null;
  /** Bumped whenever the entry takes a new socket, so a superseded one cannot report. */
  generation: number;
  closed: boolean;
  reconnectTimer: ReturnType<typeof setTimeout> | null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function createRemoteTerminals(
  dependencies: RemoteTerminalDependencies
): RemoteTerminals {
  const entries = new Map<string, Entry>();
  const listeners = new Map<string, Set<(payload: unknown) => void>>();

  const emit = (name: string, payload: unknown) => {
    for (const listener of [...(listeners.get(name) ?? [])]) listener(payload);
  };

  const ticketFor = async (environmentId: string): Promise<string> => {
    const environment = dependencies.registry.get(environmentId);
    const bearer = dependencies.registry.bearer(environmentId);
    if (!environment || bearer === null) {
      throw new Error("Remote environment is no longer configured");
    }
    const response = await dependencies.fetch(
      `${environment.baseUrl}/api/ws-ticket`,
      { method: "POST", headers: { Authorization: `Bearer ${bearer}` } }
    );
    const body = await response.text();
    let payload: unknown = null;
    try {
      payload = body ? JSON.parse(body) : null;
    } catch {
      payload = body;
    }
    const ticket = isRecord(payload) ? payload.ticket : null;
    if (!response.ok || typeof ticket !== "string" || ticket === "") {
      throw new Error(
        typeof payload === "string" && payload !== ""
          ? payload
          : `Terminal ticket request failed (${response.status})`
      );
    }
    return ticket;
  };

  const detach = (entry: Entry) => {
    entry.generation += 1;
    if (entry.reconnectTimer !== null) clearTimeout(entry.reconnectTimer);
    entry.reconnectTimer = null;
    const { socket } = entry;
    entry.socket = null;
    if (socket) {
      socket.onclose = null;
      socket.onerror = null;
      socket.onmessage = null;
      socket.close();
    }
  };

  const scheduleReconnect = (rendererId: string, entry: Entry) => {
    if (entry.reconnectTimer !== null) return;
    entry.reconnectTimer = setTimeout(() => {
      entry.reconnectTimer = null;
      if (
        entry.closed ||
        !dependencies.registry.get(entry.request.environmentId)
      ) {
        return;
      }
      connect(rendererId, entry).then(
        ({ restore }) => {
          if (restore !== "") {
            // RIS first so the repaint replaces, rather than appends to, what is on screen.
            emit("pty-output", {
              id: rendererId,
              project_path: entry.request.projectPath,
              data: `\u001Bc${restore}`,
            });
          }
        },
        () => {
          scheduleReconnect(rendererId, entry);
        }
      );
    }, dependencies.reconnectDelayMs);
  };

  /** Open a socket for `entry` and resolve with the server's attach reply. */
  const connect = async (
    rendererId: string,
    entry: Entry
  ): Promise<RemoteTerminalAttach> => {
    const { request } = entry;
    const environment = dependencies.registry.get(request.environmentId);
    if (!environment)
      throw new Error("Remote environment is no longer configured");
    const ticket = await ticketFor(request.environmentId);
    // Replaced or killed while the ticket was in flight.
    if (entry.closed) throw new Error("Remote terminal was closed");
    detach(entry);
    const { generation } = entry;
    const base = new URL(environment.baseUrl);
    const protocol = base.protocol === "https:" ? "wss:" : "ws:";
    const socket = dependencies.createSocket(
      `${protocol}//${base.host}/ws/terminal?ticket=${encodeURIComponent(ticket)}`
    );
    entry.socket = socket;
    const current = () => entry.generation === generation;
    const baseEvent = { id: rendererId, project_path: request.projectPath };

    return await new Promise<RemoteTerminalAttach>((resolve, reject) => {
      let attached = false;
      const fail = (error: Error) => {
        if (!current()) return;
        if (!attached) {
          detach(entry);
          reject(error);
          return;
        }
        if (entry.closed) return;
        // The shell survives on the server: reattach and repaint instead of ending the panel.
        entry.socket = null;
        scheduleReconnect(rendererId, entry);
      };

      socket.onopen = () => {
        socket.send(
          JSON.stringify({
            op: "attach",
            id: remoteTerminalId(rendererId),
            cwd: request.cwd,
            rows: entry.rows,
            cols: entry.cols,
            tmux_session: request.tmuxSession,
          })
        );
      };
      socket.onerror = () => {
        fail(new Error("Remote terminal connection failed"));
      };
      socket.onclose = () => {
        fail(new Error("Remote terminal connection closed"));
      };
      socket.onmessage = ({ data }) => {
        if (!current() || typeof data !== "string") return;
        let frame: unknown;
        try {
          frame = JSON.parse(data);
        } catch {
          return;
        }
        if (!isRecord(frame)) return;
        switch (frame.kind) {
          case "attached": {
            attached = true;
            dependencies.registry.reportStatus(request.environmentId, "online");
            resolve({
              created: frame.created === true,
              restore: text(frame.restore),
            });
            const title = text(frame.title);
            if (title !== "") emit("pty-title", { ...baseEvent, title });
            break;
          }
          case "data": {
            emit("pty-output", { ...baseEvent, data: text(frame.data) });
            break;
          }
          case "title": {
            emit("pty-title", { ...baseEvent, title: text(frame.title) });
            break;
          }
          case "exit": {
            entry.closed = true;
            emit("pty-exit", baseEvent);
            break;
          }
          case "error": {
            const error = new Error(
              text(frame.message) || "Remote terminal error"
            );
            if (attached)
              dependencies.registry.reportStatus(
                request.environmentId,
                "online",
                error
              );
            else {
              detach(entry);
              reject(error);
            }
            break;
          }
          default: {
            break;
          }
        }
      };
    });
  };

  return {
    owns: (id) => entries.has(id),

    async spawn(request) {
      const previous = entries.get(request.id);
      if (previous) {
        // A remounted panel attaches again; the shell stays, only the viewer is replaced.
        previous.closed = true;
        detach(previous);
      }
      const entry: Entry = {
        request,
        rows: request.rows,
        cols: request.cols,
        socket: null,
        generation: 0,
        closed: false,
        reconnectTimer: null,
      };
      entries.set(request.id, entry);
      try {
        return await connect(request.id, entry);
      } catch (error) {
        if (entries.get(request.id) === entry) entries.delete(request.id);
        throw error;
      }
    },

    write(id, data) {
      entries.get(id)?.socket?.send(JSON.stringify({ op: "input", data }));
    },

    resize(id, rows, cols) {
      const entry = entries.get(id);
      if (!entry) return;
      entry.rows = rows;
      entry.cols = cols;
      entry.socket?.send(JSON.stringify({ op: "resize", rows, cols }));
    },

    kill(id) {
      const entry = entries.get(id);
      if (!entry) return;
      entry.closed = true;
      entry.socket?.send(JSON.stringify({ op: "kill" }));
      entries.delete(id);
      // Let the kill frame flush before the socket goes away.
      setTimeout(() => {
        detach(entry);
      }, 200);
    },

    subscribe(name, listener) {
      const group = listeners.get(name) ?? new Set();
      group.add(listener);
      listeners.set(name, group);
      return () => {
        group.delete(listener);
      };
    },
  };
}
