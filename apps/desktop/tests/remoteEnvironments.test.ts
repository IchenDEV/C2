import { describe, expect, test } from "bun:test";

import type { CoreTransport } from "../src/coreTransport";
import {
  createEnvironmentRegistry,
  createFederatedCore,
  environmentPath,
  parseEnvironmentPath,
  parsePairingUrl,
} from "../src/remoteEnvironments";
import type { RemoteTerminals } from "../src/remoteTerminals";

class MemoryStorage {
  readonly values = new Map<string, string>();
  getItem(key: string) {
    return this.values.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.values.set(key, value);
  }
  removeItem(key: string) {
    this.values.delete(key);
  }
}

interface Recorded {
  name: string;
  args: unknown;
  projectPath: string | null;
}

/** A Core stand-in that answers from a table and lets tests push engine events. */
function fakeCore(answers: Record<string, unknown>) {
  const calls: Recorded[] = [];
  const listeners = new Set<(payload: unknown) => void>();
  const transport: CoreTransport = {
    call(name, args, projectPath) {
      calls.push({ name, args, projectPath });
      const answer = answers[name];
      if (answer instanceof Error) return Promise.reject(answer);
      return Promise.resolve(answer as never);
    },
    listen(name, listener) {
      if (name !== "engine-event") return () => undefined;
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
  return {
    transport,
    calls,
    listeners,
    emit: (payload: unknown) => {
      for (const listener of [...listeners]) listener(payload);
    },
  };
}

function setup(remotes: Record<string, ReturnType<typeof fakeCore>> = {}) {
  const storage = new MemoryStorage();
  const pairRequests: { url: string; body: unknown }[] = [];
  let counter = 0;
  const registry = createEnvironmentRegistry({
    storage,
    fetch: (input, init) => {
      pairRequests.push({
        url: input,
        body: JSON.parse(String(init?.body ?? "null")) as unknown,
      });
      const token = (pairRequests.at(-1)?.body as { token: string }).token;
      return Promise.resolve(
        token === "good"
          ? Response.json({
              device_id: "d",
              bearer: `bearer-${pairRequests.length}`,
            })
          : new Response("invalid or expired pairing token", { status: 401 })
      );
    },
    createSocket: () => {
      throw new Error("sockets are provided by the fake transport");
    },
    // The real web transport is covered by coreTransport.test.ts; here each environment's
    // transport is a scripted fake keyed by its host.
    createTransport: (dependencies) => {
      const remote = remotes[dependencies.location.host];
      if (!remote) throw new Error(`no fake for ${dependencies.location.host}`);
      return remote.transport;
    },
    newId: () => `env-${++counter}`,
    onError: () => undefined,
    reconnectDelayMs: 1,
  });
  return { registry, storage, pairRequests };
}

describe("parsePairingUrl", () => {
  test("splits the origin from the fragment token", () => {
    expect(parsePairingUrl("http://10.0.0.5:4599/pair#token=abc")).toEqual({
      baseUrl: "http://10.0.0.5:4599",
      token: "abc",
    });
    expect(parsePairingUrl(" https://c2.example.com/#token=x ").baseUrl).toBe(
      "https://c2.example.com"
    );
  });

  test("rejects links that cannot be redeemed", () => {
    expect(() => parsePairingUrl("not a url")).toThrow("full pairing link");
    expect(() => parsePairingUrl("ftp://host/pair#token=a")).toThrow("http");
    expect(() => parsePairingUrl("http://host:1/pair")).toThrow("no token");
    expect(() => parsePairingUrl("http://host/prefix/pair#token=a")).toThrow(
      "path prefix"
    );
  });
});

describe("environment registry", () => {
  test("pairing stores the credential per environment and survives a reload", async () => {
    const { registry, storage, pairRequests } = setup();
    const environment = await registry.add(
      "http://gpu-box:4599/pair#token=good",
      { name: "GPU box", workspace: " /srv/work " }
    );

    expect(pairRequests[0]).toEqual({
      url: "http://gpu-box:4599/api/pair",
      body: { token: "good", device_name: "C2 Desktop" },
    });
    expect(environment).toEqual({
      id: "env-1",
      name: "GPU box",
      baseUrl: "http://gpu-box:4599",
      workspace: "/srv/work",
    });
    expect(storage.getItem("codetwo.environment.env-1.bearer")).toBe(
      "bearer-1"
    );

    const reloaded = createEnvironmentRegistry({
      ...({} as never),
      storage,
      createTransport: () => ({}) as CoreTransport,
      fetch: () => Promise.reject(new Error("offline")),
      createSocket: () => ({}) as never,
      newId: () => "unused",
      onError: () => undefined,
      reconnectDelayMs: 1,
    });
    expect(reloaded.list()).toEqual([environment]);
  });

  test("a rejected link adds nothing and explains how to recover", async () => {
    const { registry, storage } = setup();
    await expect(
      registry.add("http://gpu-box:4599/pair#token=stale")
    ).rejects.toThrow("codetwo-server pair");
    expect(registry.list()).toEqual([]);
    expect(storage.values.size).toBe(0);
  });

  test("pairing the same server again replaces its credential, not its row", async () => {
    const { registry, storage } = setup();
    const first = await registry.add("http://gpu-box:4599/pair#token=good", {
      name: "GPU box",
    });
    const second = await registry.add("http://gpu-box:4599/pair#token=good");

    expect(second.id).toBe(first.id);
    expect(second.name).toBe("GPU box");
    expect(registry.list()).toHaveLength(1);
    // The second pairing's credential replaced the first.
    expect(storage.getItem(`codetwo.environment.${first.id}.bearer`)).toBe(
      "bearer-2"
    );
  });

  test("removing an environment forgets its credential and active selection", async () => {
    const { registry, storage } = setup();
    const { id } = await registry.add("http://gpu-box:4599/pair#token=good");
    registry.setActive(id);
    expect(registry.activeId()).toBe(id);

    let notifications = 0;
    registry.subscribe(() => (notifications += 1));
    registry.remove(id);

    expect(registry.list()).toEqual([]);
    expect(registry.activeId()).toBeNull();
    expect(storage.getItem(`codetwo.environment.${id}.bearer`)).toBeNull();
    expect(notifications).toBeGreaterThan(0);
  });
});

const localSession = { id: "local-1", title: "Local" };
const remoteSession = { id: "remote-1", title: "Remote" };

async function federated(
  remoteAnswers: Record<string, unknown> = { "sessions.list": [remoteSession] }
) {
  const local = fakeCore({
    "sessions.list": [localSession],
    "sessions.archived": [],
    "sessions.previews": [["local-1", "hello"]],
  });
  const remote = fakeCore({
    "sessions.archived": [],
    "sessions.previews": [["remote-1", "from the server"]],
    "workspace.default_cwd": "/srv/default",
    ...remoteAnswers,
  });
  const { registry } = setup({ "gpu-box:4599": remote });
  const environment = await registry.add("http://gpu-box:4599/pair#token=good");
  return {
    local,
    remote,
    registry,
    environment,
    core: createFederatedCore(local.transport, registry),
  };
}

describe("federated core", () => {
  test("with no environments every call goes to this machine untouched", async () => {
    const local = fakeCore({ "sessions.list": [localSession] });
    const { registry } = setup();
    const core = createFederatedCore(local.transport, registry);

    expect(await core.call("sessions.list", null, "/p")).toEqual([
      localSession,
    ]);
    expect(local.calls).toEqual([
      { name: "sessions.list", args: null, projectPath: "/p" },
    ]);
  });

  test("session lists merge and tag only the remote rows", async () => {
    const { core, environment } = await federated();

    expect(await core.call("sessions.list", null, null)).toEqual([
      localSession,
      { ...remoteSession, environment_id: environment.id },
    ]);
    expect(await core.call("sessions.previews", null, null)).toEqual([
      ["local-1", "hello"],
      ["remote-1", "from the server"],
    ]);
  });

  test("an offline machine does not hide reachable sessions and is reported", async () => {
    const { core, registry, environment } = await federated({
      "sessions.list": new Error("connect ECONNREFUSED"),
    });

    expect(await core.call("sessions.list", null, null)).toEqual([
      localSession,
    ]);
    expect(registry.status(environment.id)).toEqual({
      state: "offline",
      error: "connect ECONNREFUSED",
    });
  });

  test("commands follow the machine that owns the session", async () => {
    const { core, local, remote } = await federated({
      "sessions.list": [remoteSession],
      "engine.prompt": null,
    });
    await core.call("sessions.list", null, null);

    await core.call(
      "engine.prompt",
      { session: "remote-1", doc: [] },
      "/local/path"
    );
    await core.call(
      "engine.prompt",
      { session: "local-1", doc: [] },
      "/local/path"
    );

    expect(remote.calls.at(-1)).toEqual({
      name: "engine.prompt",
      args: { session: "remote-1", doc: [] },
      // A local project path is meaningless on another machine.
      projectPath: null,
    });
    expect(local.calls.at(-1)).toMatchObject({
      name: "engine.prompt",
      args: { session: "local-1", doc: [] },
      projectPath: "/local/path",
    });
    expect(core.ownerOf("remote-1")).not.toBeNull();
    expect(core.ownerOf("local-1")).toBeNull();
  });

  test("a new session is created remotely without leaking the routing field", async () => {
    const { core, local, remote, environment } = await federated({
      "engine.new_session": null,
    });

    await core.call(
      "engine.new_session",
      { provider: "codex", cwd: "/srv/work", environment_id: environment.id },
      null
    );

    expect(remote.calls.at(-1)).toEqual({
      name: "engine.new_session",
      args: { provider: "codex", cwd: "/srv/work" },
      projectPath: null,
    });
    expect(local.calls.some((call) => call.name === "engine.new_session")).toBe(
      false
    );
  });

  test("engine events from every machine reach one listener and teach ownership", async () => {
    const { core, local, remote } = await federated();
    const seen: unknown[] = [];
    const stop = core.listen("engine-event", (event) => seen.push(event));

    local.emit({ event: "session_created", session: "local-2" });
    remote.emit({ event: "session_created", session: "remote-2" });
    expect(seen).toHaveLength(2);
    expect(core.ownerOf("remote-2")).not.toBeNull();
    expect(core.ownerOf("local-2")).toBeNull();

    stop();
    expect(local.listeners.size).toBe(0);
    expect(remote.listeners.size).toBe(0);
  });

  test("an environment paired while listening starts streaming, and removal stops it", async () => {
    const local = fakeCore({});
    const late = fakeCore({});
    const { registry } = setup({ "late:4599": late });
    const core = createFederatedCore(local.transport, registry);
    const seen: unknown[] = [];
    core.listen("engine-event", (event) => seen.push(event));
    expect(late.listeners.size).toBe(0);

    const { id } = await registry.add("http://late:4599/pair#token=good");
    expect(late.listeners.size).toBe(1);
    late.emit({ event: "turn_started", session: "s" });
    expect(seen).toHaveLength(1);

    registry.remove(id);
    expect(late.listeners.size).toBe(0);
  });

  test("other event channels stay local", async () => {
    const { core, remote } = await federated();
    const stop = core.listen("host-status", () => undefined);
    expect(remote.listeners.size).toBe(0);
    stop();
  });
});

describe("remote folders", () => {
  const folderSession = {
    id: "remote-1",
    title: "Remote",
    cwd: "/srv/app",
    project_path: "/srv/app",
    worktree_path: null,
  };

  test("paths carry their environment and round-trip", () => {
    const path = environmentPath("env-1", "/srv/app");
    expect(path).toBe("c2env://env-1/srv/app");
    expect(parseEnvironmentPath(path)).toEqual({
      environmentId: "env-1",
      path: "/srv/app",
    });
    expect(parseEnvironmentPath("c2env://env-1")).toEqual({
      environmentId: "env-1",
      path: "/",
    });
    expect(
      parseEnvironmentPath(environmentPath("env-1", "C:\\work\\app"))
    ).toEqual({ environmentId: "env-1", path: "C:\\work\\app" });
    expect(parseEnvironmentPath("/srv/app")).toBeNull();
    expect(parseEnvironmentPath(null)).toBeNull();
  });

  test("remote sessions and new-session events name folders by environment", async () => {
    const { core, remote, environment } = await federated({
      "sessions.list": [folderSession],
    });
    const rows = (await core.call("sessions.list", null, null)) as Record<
      string,
      unknown
    >[];
    expect(rows[1]).toEqual({
      ...folderSession,
      cwd: `c2env://${environment.id}/srv/app`,
      project_path: `c2env://${environment.id}/srv/app`,
      environment_id: environment.id,
    });

    const seen: unknown[] = [];
    core.listen("engine-event", (event) => seen.push(event));
    remote.emit({
      event: "session_created",
      session: "remote-2",
      cwd: "/srv/app",
      project_path: null,
    });
    expect(seen).toEqual([
      {
        event: "session_created",
        session: "remote-2",
        cwd: `c2env://${environment.id}/srv/app`,
        project_path: null,
      },
    ]);
  });

  test("workspace and git commands for a remote folder run on that machine only", async () => {
    const { core, local, remote, environment } = await federated({
      "workspace.read_text": "remote file",
      "git.status": { branch: "main" },
    });
    const cwd = environmentPath(environment.id, "/srv/app");

    expect(
      await core.call("workspace.read_text", { cwd, path: "a.txt" }, "/p")
    ).toBe("remote file");
    expect(await core.call("git.status", { cwd }, null)).toEqual({
      branch: "main",
    });
    expect(remote.calls).toEqual([
      {
        name: "workspace.read_text",
        args: { cwd: "/srv/app", path: "a.txt" },
        projectPath: null,
      },
      { name: "git.status", args: { cwd: "/srv/app" }, projectPath: null },
    ]);
    expect(local.calls).toEqual([]);
  });

  test("a local folder with the same path never goes remote", async () => {
    const { core, local, remote } = await federated();
    await core.call("git.status", { cwd: "/srv/app" }, null);
    expect(local.calls).toHaveLength(1);
    expect(remote.calls).toEqual([]);
  });

  test("a new session in a remote folder is created there", async () => {
    const { core, local, remote, environment } = await federated();
    await core.call(
      "engine.new_session",
      { provider: "codex", cwd: environmentPath(environment.id, "/srv/app") },
      null
    );
    expect(remote.calls.at(-1)).toEqual({
      name: "engine.new_session",
      args: { provider: "codex", cwd: "/srv/app" },
      projectPath: null,
    });
    expect(local.calls).toEqual([]);
  });

  test("a removed environment fails loudly instead of touching local files", async () => {
    const { core, local, registry, environment } = await federated();
    const cwd = environmentPath(environment.id, "/srv/app");
    registry.remove(environment.id);
    await expect(
      core.call("workspace.delete", { cwd, path: "a" }, null)
    ).rejects.toThrow("no longer configured");
    expect(local.calls).toEqual([]);
  });
});

describe("remote terminals in the federated core", () => {
  function fakeTerminals() {
    const log: string[] = [];
    const listeners = new Map<string, Set<(payload: unknown) => void>>();
    const owned = new Set<string>();
    const terminals: RemoteTerminals = {
      owns: (id) => owned.has(id),
      spawn: (request) => {
        owned.add(request.id);
        log.push(
          `spawn ${request.environmentId} ${request.id} ${request.cwd} ${request.rows}x${request.cols} ${request.projectPath}`
        );
        return Promise.resolve({ created: true, restore: "R" });
      },
      write: (id, data) => void log.push(`write ${id} ${data}`),
      resize: (id, rows, cols) => void log.push(`resize ${id} ${rows}x${cols}`),
      kill: (id) => void log.push(`kill ${id}`),
      subscribe: (name, listener) => {
        const group = listeners.get(name) ?? new Set();
        group.add(listener);
        listeners.set(name, group);
        return () => void group.delete(listener);
      },
    };
    return { terminals, log, listeners };
  }

  async function withTerminals() {
    const { terminals, log, listeners } = fakeTerminals();
    const local = fakeCore({ "terminal.write": "local" });
    const remote = fakeCore({});
    const { registry } = setup({ "gpu-box:4599": remote });
    const environment = await registry.add(
      "http://gpu-box:4599/pair#token=good"
    );
    const core = createFederatedCore(local.transport, registry, terminals);
    return { core, local, log, listeners, environment };
  }

  test("a terminal in a remote folder is driven remotely, local ones stay local", async () => {
    const { core, local, log, environment } = await withTerminals();
    const cwd = environmentPath(environment.id, "/srv/app");

    expect(
      await core.call(
        "terminal.spawn",
        { id: "t1", cwd, rows: 24, cols: 80 },
        "/proj"
      )
    ).toEqual({ created: true, restore: "R" });
    await core.call("terminal.write", { id: "t1", data: "ls\n" }, null);
    await core.call("terminal.resize", { id: "t1", rows: 30, cols: 100 }, null);
    expect(
      await core.call("terminal.dump", { id: "t1", all: true }, null)
    ).toBe("");
    await core.call("terminal.kill", { id: "t1" }, null);
    expect(log).toEqual([
      `spawn ${environment.id} t1 /srv/app 24x80 /proj`,
      "write t1 ls\n",
      "resize t1 30x100",
      "kill t1",
    ]);

    await core.call("terminal.write", { id: "local-term", data: "x" }, null);
    expect(local.calls).toEqual([
      {
        name: "terminal.write",
        args: { id: "local-term", data: "x" },
        projectPath: null,
      },
    ]);
  });

  test("pty events from a remote shell reach the same listeners as local ones", async () => {
    const { core, listeners } = await withTerminals();
    const seen: unknown[] = [];
    const stop = core.listen("pty-output", (event) => seen.push(event));
    for (const listener of listeners.get("pty-output") ?? [])
      listener("remote");
    expect(seen).toEqual(["remote"]);
    stop();
    expect(listeners.get("pty-output")?.size).toBe(0);
  });
});
