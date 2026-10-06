import { describe, expect, test } from "bun:test";

import type { RemoteEnvironment } from "../src/remoteEnvironments";
import {
  createRemoteTerminals,
  remoteTerminalId,
} from "../src/remoteTerminals";
import type { TerminalSocketLike } from "../src/remoteTerminals";

class FakeSocket implements TerminalSocketLike {
  onclose: TerminalSocketLike["onclose"] = null;
  onerror: TerminalSocketLike["onerror"] = null;
  onmessage: TerminalSocketLike["onmessage"] = null;
  onopen: TerminalSocketLike["onopen"] = null;
  readonly sent: unknown[] = [];
  closed = false;
  constructor(readonly url: string) {}
  send(data: string) {
    this.sent.push(JSON.parse(data));
  }
  close() {
    this.closed = true;
  }
  open() {
    this.onopen?.(new Event("open"));
  }
  receive(frame: unknown) {
    this.onmessage?.(
      new MessageEvent("message", { data: JSON.stringify(frame) })
    );
  }
  drop() {
    this.onclose?.(new CloseEvent("close"));
  }
}

const environment: RemoteEnvironment = {
  id: "env-1",
  name: "GPU",
  baseUrl: "http://gpu-box:4599",
  workspace: null,
};

function setup(options: { bearer?: string | null } = {}) {
  const sockets: FakeSocket[] = [];
  const tickets: { url: string; authorization: string | null }[] = [];
  const statuses: string[] = [];
  const terminals = createRemoteTerminals({
    registry: {
      get: (id) => (id === environment.id ? environment : null),
      bearer: () => (options.bearer === undefined ? "secret" : options.bearer),
      reportStatus: (_id, state) => void statuses.push(state),
    },
    fetch: (input, init) => {
      tickets.push({
        url: input,
        authorization: new Headers(init?.headers).get("Authorization"),
      });
      return Promise.resolve(
        Response.json({ ticket: `ticket-${tickets.length}` })
      );
    },
    createSocket: (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket;
    },
    reconnectDelayMs: 1,
  });
  return { terminals, sockets, tickets, statuses };
}

const request = {
  environmentId: "env-1",
  id: "t1",
  cwd: "/srv/app",
  rows: 24,
  cols: 80,
  tmuxSession: null,
  projectPath: "/proj",
};

const tick = async () =>
  await new Promise((resolve) => setTimeout(resolve, 20));

async function attach(ctx: ReturnType<typeof setup>, restore = "SCREEN") {
  const attaching = ctx.terminals.spawn(request);
  await tick();
  const socket = ctx.sockets.at(-1) as FakeSocket;
  socket.open();
  socket.receive({
    kind: "attached",
    id: "c2d-t1",
    created: true,
    restore,
    title: "",
  });
  return { socket, result: await attaching };
}

describe("remoteTerminalId", () => {
  test("keeps short safe ids recognisable and hashes the rest stably", () => {
    expect(remoteTerminalId("term-1")).toBe("c2d-term-1");
    const long = `session/${"x".repeat(100)}`;
    const id = remoteTerminalId(long);
    expect(id).toMatch(/^c2d-[A-Za-z0-9]+-[0-9a-f]+$/);
    expect(id.length).toBeLessThanOrEqual(64);
    expect(remoteTerminalId(long)).toBe(id);
    expect(remoteTerminalId(`${long}!`)).not.toBe(id);
  });
});

describe("remote terminals", () => {
  test("attaches with a ticket and replays the shell's screen", async () => {
    const ctx = setup();
    const { socket, result } = await attach(ctx);

    expect(ctx.tickets).toEqual([
      {
        url: "http://gpu-box:4599/api/ws-ticket",
        authorization: "Bearer secret",
      },
    ]);
    expect(socket.url).toBe("ws://gpu-box:4599/ws/terminal?ticket=ticket-1");
    expect(socket.sent[0]).toEqual({
      op: "attach",
      id: "c2d-t1",
      cwd: "/srv/app",
      rows: 24,
      cols: 80,
      tmux_session: null,
    });
    expect(result).toEqual({ created: true, restore: "SCREEN" });
    expect(ctx.terminals.owns("t1")).toBe(true);
    expect(ctx.statuses).toContain("online");
  });

  test("forwards output, title and exit under the renderer's id and realm", async () => {
    const ctx = setup();
    const events: [string, unknown][] = [];
    for (const name of ["pty-output", "pty-title", "pty-exit"]) {
      ctx.terminals.subscribe(
        name,
        (payload) => void events.push([name, payload])
      );
    }
    const { socket } = await attach(ctx);
    socket.receive({ kind: "data", data: "hello" });
    socket.receive({ kind: "title", title: "vim" });
    socket.receive({ kind: "exit" });
    expect(events).toEqual([
      ["pty-output", { id: "t1", project_path: "/proj", data: "hello" }],
      ["pty-title", { id: "t1", project_path: "/proj", title: "vim" }],
      ["pty-exit", { id: "t1", project_path: "/proj" }],
    ]);
  });

  test("input, resize and kill are sent to the shell", async () => {
    const ctx = setup();
    const { socket } = await attach(ctx);
    ctx.terminals.write("t1", "ls\n");
    ctx.terminals.resize("t1", 40, 120);
    ctx.terminals.kill("t1");
    expect(socket.sent.slice(1)).toEqual([
      { op: "input", data: "ls\n" },
      { op: "resize", rows: 40, cols: 120 },
      { op: "kill" },
    ]);
    expect(ctx.terminals.owns("t1")).toBe(false);
  });

  test("a server refusal rejects the spawn and leaves nothing behind", async () => {
    const ctx = setup();
    const attaching = ctx.terminals.spawn(request);
    await tick();
    const socket = ctx.sockets[0] as FakeSocket;
    socket.open();
    socket.receive({ kind: "error", message: "terminal id is invalid" });
    await expect(attaching).rejects.toThrow("terminal id is invalid");
    expect(ctx.terminals.owns("t1")).toBe(false);
    expect(socket.closed).toBe(true);
  });

  test("a missing credential fails before any socket opens", async () => {
    const ctx = setup({ bearer: null });
    await expect(ctx.terminals.spawn(request)).rejects.toThrow(
      "no longer configured"
    );
    expect(ctx.sockets).toEqual([]);
  });

  test("a dropped connection reattaches and repaints from the server", async () => {
    const ctx = setup();
    const events: unknown[] = [];
    ctx.terminals.subscribe(
      "pty-output",
      (payload) => void events.push(payload)
    );
    const { socket } = await attach(ctx);

    socket.drop();
    await tick();
    const next = ctx.sockets.at(-1) as FakeSocket;
    expect(next).not.toBe(socket);
    next.open();
    next.receive({
      kind: "attached",
      id: "c2d-t1",
      created: false,
      restore: "AGAIN",
      title: "",
    });
    await tick();
    expect(events).toEqual([
      { id: "t1", project_path: "/proj", data: "\u001BcAGAIN" },
    ]);
    expect(ctx.terminals.owns("t1")).toBe(true);
  });

  test("attaching the same id again replaces the viewer, not the shell", async () => {
    const ctx = setup();
    const { socket } = await attach(ctx);
    await attach(ctx, "SECOND");
    expect(socket.closed).toBe(true);
    expect(
      socket.sent.some((frame) => (frame as { op: string }).op === "kill")
    ).toBe(false);
  });
});
