/**
 * Remote C2 environments: other machines running `codetwo-server serve`, paired from this app.
 *
 * Each environment owns its sessions, workspace, providers and credentials. This client only
 * remembers where it is and holds one per-device bearer; it never copies sessions. The federated
 * core below makes the renderer's existing Core interface see the union: session lists and engine
 * events are merged, and any command that names a remote session is sent to the machine that owns
 * it. Local behavior is unchanged when no environment is configured.
 */
import type {
  CoreTransport,
  WebCoreTransportDependencies,
} from "./coreTransport";
import type { RemoteTerminals } from "./remoteTerminals";

export interface RemoteEnvironment {
  id: string;
  name: string;
  /** `http(s)://host:port`, no trailing slash and no path. */
  baseUrl: string;
  /** Workspace on the remote machine used for new sessions; null asks the server for its default. */
  workspace: string | null;
}

export type EnvironmentState = "connecting" | "online" | "offline";

export interface EnvironmentStatus {
  state: EnvironmentState;
  error: string | null;
}

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export interface EnvironmentRegistryDependencies {
  storage: StorageLike;
  fetch(input: string, init?: RequestInit): Promise<Response>;
  createSocket: WebCoreTransportDependencies["createSocket"];
  createTransport(dependencies: WebCoreTransportDependencies): CoreTransport;
  newId(): string;
  onError(error: unknown): void;
  reconnectDelayMs: number;
}

/** A device paired with a remote server, as that server reports it. */
export interface RemoteServerDevice {
  id: string;
  name: string;
  protocol: string;
  /** Unix seconds. */
  createdAt: number;
  /** Unix seconds. */
  lastSeen: number;
  /** True for the credential this app is using. */
  current: boolean;
}

/** Immutable view for UI subscriptions; a new object is produced after every change. */
export interface EnvironmentsSnapshot {
  environments: RemoteEnvironment[];
  statuses: Record<string, EnvironmentStatus>;
  activeId: string | null;
}

export interface EnvironmentRegistry {
  snapshot(): EnvironmentsSnapshot;
  list(): RemoteEnvironment[];
  get(id: string): RemoteEnvironment | null;
  /** Redeem a one-time pairing link printed by `codetwo-server serve` / `codetwo-server pair`. */
  add(
    pairingUrl: string,
    options?: { name?: string; workspace?: string | null }
  ): Promise<RemoteEnvironment>;
  update(id: string, patch: { name?: string; workspace?: string | null }): void;
  remove(id: string): void;
  transport(id: string): CoreTransport | null;
  /** Devices paired with that server (any one paired device may review them). */
  devices(id: string): Promise<RemoteServerDevice[]>;
  /**
   * Cut a device off on the server at once. Revoking the credential this app uses also forgets the
   * environment here, since it could not connect again; the result says whether that happened.
   */
  revokeDevice(id: string, deviceId: string): Promise<{ wasCurrent: boolean }>;
  /** Per-device credential for `id`, for protocols that do not run through the Core transport. */
  bearer(id: string): string | null;
  status(id: string): EnvironmentStatus;
  reportStatus(id: string, state: EnvironmentState, error?: unknown): void;
  /** Environment that new sessions are created in; null means this machine. */
  activeId(): string | null;
  setActive(id: string | null): void;
  /** Notified on any change to environments, status, or the active selection. */
  subscribe(listener: () => void): () => void;
}

const ENVIRONMENTS_KEY = "codetwo.environments.v1";
const ACTIVE_KEY = "codetwo.environments.active";
const bearerKey = (id: string) => `codetwo.environment.${id}.bearer`;
const WEB_BEARER_KEY = "codetwo.remote.bearer";

function isRecord(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

/** Trimmed text, or null when nothing meaningful remains. */
function textOrNull(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed === undefined || trimmed === "" ? null : trimmed;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Split a pairing link into the server origin and its one-time token. */
export function parsePairingUrl(raw: string): {
  baseUrl: string;
  token: string;
} {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    throw new Error(
      "Paste the full pairing link, e.g. http://host:4599/pair#token=…"
    );
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("The pairing link must start with http:// or https://");
  }
  if (
    url.pathname !== "/" &&
    url.pathname !== "/pair" &&
    url.pathname !== "/pair/"
  ) {
    throw new Error(
      "Servers behind a path prefix are not supported; use a dedicated host name or port."
    );
  }
  const token = new URLSearchParams(url.hash.replace(/^#/, "")).get("token");
  if (token == null || token === "") {
    throw new Error(
      "The pairing link has no token. Run `codetwo-server pair` for a new one."
    );
  }
  return { baseUrl: url.origin, token };
}

function readEnvironments(storage: StorageLike): RemoteEnvironment[] {
  try {
    const value: unknown = JSON.parse(
      storage.getItem(ENVIRONMENTS_KEY) ?? "[]"
    );
    if (!Array.isArray(value)) return [];
    return value.flatMap((item): RemoteEnvironment[] => {
      if (!isRecord(item)) return [];
      const { id, name, baseUrl, workspace } = item;
      if (typeof id !== "string" || typeof baseUrl !== "string") return [];
      return [
        {
          id,
          baseUrl,
          name: typeof name === "string" && name !== "" ? name : baseUrl,
          workspace:
            typeof workspace === "string" && workspace !== ""
              ? workspace
              : null,
        },
      ];
    });
  } catch {
    return [];
  }
}

export function createEnvironmentRegistry(
  dependencies: EnvironmentRegistryDependencies
): EnvironmentRegistry {
  const { storage } = dependencies;
  let environments = readEnvironments(storage);
  const transports = new Map<string, CoreTransport>();
  const statuses = new Map<string, EnvironmentStatus>();
  const listeners = new Set<() => void>();
  let snapshot: EnvironmentsSnapshot | null = null;

  const notify = () => {
    snapshot = null;
    for (const listener of [...listeners]) listener();
  };
  const persist = () => {
    storage.setItem(ENVIRONMENTS_KEY, JSON.stringify(environments));
  };
  const get = (id: string) =>
    environments.find((environment) => environment.id === id) ?? null;

  const reportStatus = (
    id: string,
    state: EnvironmentState,
    error?: unknown
  ) => {
    const next: EnvironmentStatus = {
      state,
      error: error == null ? null : errorText(error),
    };
    const current = statuses.get(id);
    if (current?.state === next.state && current.error === next.error) return;
    statuses.set(id, next);
    notify();
  };

  const transportFor = (environment: RemoteEnvironment): CoreTransport => {
    const cached = transports.get(environment.id);
    if (cached) return cached;
    const base = new URL(environment.baseUrl);
    const created = dependencies.createTransport({
      // The pairing link was consumed when the environment was added.
      clearPairingFragment: () => undefined,
      createSocket: dependencies.createSocket,
      fetch: async (input, init) =>
        await dependencies.fetch(`${environment.baseUrl}${input}`, init),
      location: {
        hash: "",
        host: base.host,
        pathname: "/",
        protocol: base.protocol,
        search: "",
      },
      onError: (error) => {
        reportStatus(environment.id, "offline", error);
        dependencies.onError(error);
      },
      reconnectDelayMs: dependencies.reconnectDelayMs,
      // The web transport keeps its bearer under one fixed key; scope it to this environment.
      storage: {
        getItem: (key) =>
          storage.getItem(
            key === WEB_BEARER_KEY ? bearerKey(environment.id) : key
          ),
        setItem: (key, value) => {
          storage.setItem(
            key === WEB_BEARER_KEY ? bearerKey(environment.id) : key,
            value
          );
        },
        removeItem: (key) => {
          storage.removeItem(
            key === WEB_BEARER_KEY ? bearerKey(environment.id) : key
          );
        },
      },
    });
    transports.set(environment.id, created);
    return created;
  };

  /** Authenticated request to a server's own HTTP API (not the Core command channel). */
  const serverRequest = async (
    id: string,
    path: string,
    method: "GET" | "POST"
  ): Promise<unknown> => {
    const environment = get(id);
    const bearer = environment ? storage.getItem(bearerKey(id)) : null;
    if (!environment || bearer === null) {
      throw new Error("Remote environment is no longer configured");
    }
    const response = await dependencies.fetch(`${environment.baseUrl}${path}`, {
      method,
      headers: { Authorization: `Bearer ${bearer}` },
    });
    const text = await response.text();
    let payload: unknown = text;
    try {
      payload = text ? JSON.parse(text) : null;
    } catch {
      payload = text;
    }
    if (!response.ok) {
      throw new Error(
        response.status === 401
          ? "This app's credential was rejected by the server. It may have been revoked; remove the environment and pair again."
          : typeof payload === "string" && payload !== ""
            ? payload
            : `Request failed (${response.status})`
      );
    }
    return payload;
  };

  const activeId = (): string | null => {
    const id = storage.getItem(ACTIVE_KEY);
    return id !== null && get(id) ? id : null;
  };

  const registry: EnvironmentRegistry = {
    snapshot() {
      // Stable between changes, as useSyncExternalStore requires.
      snapshot ??= {
        environments,
        statuses: Object.fromEntries(
          environments.map((environment) => [
            environment.id,
            statuses.get(environment.id) ?? {
              state: "connecting",
              error: null,
            },
          ])
        ),
        activeId: activeId(),
      };
      return snapshot;
    },
    list: () => environments,
    get,

    async add(pairingUrl, options = {}) {
      const { baseUrl, token } = parsePairingUrl(pairingUrl);
      const existing = environments.find(
        (environment) => environment.baseUrl === baseUrl
      );
      const response = await dependencies.fetch(`${baseUrl}/api/pair`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ token, device_name: "C2 Desktop" }),
      });
      const text = await response.text();
      let payload: unknown = null;
      try {
        payload = text ? JSON.parse(text) : null;
      } catch {
        payload = text;
      }
      if (!response.ok) {
        throw new Error(
          response.status === 401
            ? "The pairing link is invalid, expired or already used. Run `codetwo-server pair` on the server for a new one."
            : typeof payload === "string" && payload !== ""
              ? payload
              : `Pairing failed (${response.status})`
        );
      }
      const bearer =
        payload != null && typeof payload === "object"
          ? (payload as { bearer?: unknown }).bearer
          : null;
      if (typeof bearer !== "string" || bearer === "") {
        throw new Error("The server did not return a credential.");
      }

      // Re-pairing the same server replaces its credential instead of adding a duplicate row.
      const id = existing?.id ?? dependencies.newId();
      storage.setItem(bearerKey(id), bearer);
      transports.delete(id);
      const environment: RemoteEnvironment = {
        id,
        baseUrl,
        name:
          textOrNull(options.name) ?? existing?.name ?? new URL(baseUrl).host,
        workspace: textOrNull(options.workspace) ?? existing?.workspace ?? null,
      };
      environments = existing
        ? environments.map((item) => (item.id === id ? environment : item))
        : [...environments, environment];
      persist();
      reportStatus(id, "connecting");
      notify();
      return environment;
    },

    update(id, patch) {
      const current = get(id);
      if (!current) return;
      const next: RemoteEnvironment = {
        ...current,
        name: textOrNull(patch.name) ?? current.name,
        workspace:
          patch.workspace === undefined
            ? current.workspace
            : textOrNull(patch.workspace),
      };
      environments = environments.map((item) => (item.id === id ? next : item));
      persist();
      notify();
    },

    remove(id) {
      if (!get(id)) return;
      environments = environments.filter((item) => item.id !== id);
      transports.delete(id);
      statuses.delete(id);
      storage.removeItem(bearerKey(id));
      if (storage.getItem(ACTIVE_KEY) === id) storage.removeItem(ACTIVE_KEY);
      persist();
      notify();
    },

    transport(id) {
      const environment = get(id);
      return environment ? transportFor(environment) : null;
    },

    bearer(id) {
      return get(id) ? storage.getItem(bearerKey(id)) : null;
    },

    async devices(id) {
      const payload = await serverRequest(id, "/api/devices", "GET");
      if (!Array.isArray(payload)) return [];
      return (payload as unknown[]).flatMap((item): RemoteServerDevice[] => {
        if (!isRecord(item) || typeof item.id !== "string") return [];
        return [
          {
            id: item.id,
            name: typeof item.name === "string" ? item.name : item.id,
            protocol: typeof item.protocol === "string" ? item.protocol : "",
            createdAt:
              typeof item.created_at === "number" ? item.created_at : 0,
            lastSeen: typeof item.last_seen === "number" ? item.last_seen : 0,
            current: item.current === true,
          },
        ];
      });
    },

    async revokeDevice(id, deviceId) {
      const payload = await serverRequest(
        id,
        `/api/devices/${encodeURIComponent(deviceId)}/revoke`,
        "POST"
      );
      const wasCurrent = isRecord(payload) && payload.was_current === true;
      if (wasCurrent) registry.remove(id);
      return { wasCurrent };
    },

    status: (id) => statuses.get(id) ?? { state: "connecting", error: null },
    reportStatus,

    activeId,
    setActive(id) {
      if (id != null && !get(id)) return;
      if (id == null) storage.removeItem(ACTIVE_KEY);
      else storage.setItem(ACTIVE_KEY, id);
      notify();
    },

    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  return registry;
}

// ---- federation -------------------------------------------------------------------------------

const ENVIRONMENT_PATH_PREFIX = "c2env://";
const WINDOWS_DRIVE_PATH = /^\/[A-Za-z]:[/\\]/;
const ENVIRONMENT_PATH = /^c2env:\/\/([^/]+)(\/[\s\S]*)?$/;

/**
 * A folder on a remote machine, as the renderer holds it. Panels pass a session's `cwd` straight
 * back to the Core, so the owning environment travels inside the path itself: `c2env://<id>/srv/app`
 * can never be mistaken for a local folder, even when both machines have `/srv/app`.
 */
export function environmentPath(environmentId: string, path: string): string {
  const absolute = path.startsWith("/") ? path : `/${path}`;
  return `${ENVIRONMENT_PATH_PREFIX}${environmentId}${absolute}`;
}

/** Inverse of {@link environmentPath}; null for ordinary local paths. */
export function parseEnvironmentPath(
  value: unknown
): { environmentId: string; path: string } | null {
  if (typeof value !== "string") return null;
  const match = ENVIRONMENT_PATH.exec(value);
  if (!match) return null;
  const rest = match[2] ?? "/";
  return {
    environmentId: match[1] ?? "",
    path: WINDOWS_DRIVE_PATH.test(rest) ? rest.slice(1) : rest,
  };
}

const PATH_FIELDS = ["cwd", "project_path", "worktree_path"] as const;

/** Rewrite the folder fields of a remote row or event so they carry their environment. */
function withEnvironmentPaths(
  environmentId: string,
  record: Record<string, unknown>
): Record<string, unknown> {
  const next = { ...record };
  for (const field of PATH_FIELDS) {
    const value = next[field];
    if (typeof value === "string" && value !== "") {
      next[field] = environmentPath(environmentId, value);
    }
  }
  return next;
}

const TERMINAL_EVENTS = new Set(["pty-output", "pty-title", "pty-exit"]);
const AGGREGATED_SESSION_LISTS = new Set([
  "sessions.list",
  "sessions.archived",
]);
const NEW_SESSION_COMMANDS = new Set([
  "engine.new_session",
  "engine.new_parallel_task",
]);

function argsObject(args: unknown): Record<string, unknown> | null {
  return isRecord(args) ? args : null;
}

/** Commands are untyped on the wire; callers name the row shape they expect, as `call<T>` does. */
function asResult<T>(value: unknown): T {
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- same contract as CoreTransport.call<T>
  return value as T;
}

function concatRows(...parts: unknown[]): unknown[] {
  const rows: unknown[] = [];
  for (const part of parts) {
    if (Array.isArray(part)) rows.push(...(part as unknown[]));
  }
  return rows;
}

/**
 * Wrap the local Core so remote environments appear as part of it.
 *
 * Routing is by ownership, not by guessing paths: a session id seen in a remote list or event
 * belongs to that environment for every later command. New sessions go where the caller says via
 * `environment_id` (stripped before it reaches a Core that does not know the field).
 */
export function createFederatedCore(
  local: CoreTransport,
  registry: EnvironmentRegistry,
  terminals?: RemoteTerminals
): CoreTransport & {
  callEnvironment<T>(
    environmentId: string,
    name: string,
    args: unknown
  ): Promise<T>;
  ownerOf(session: string): string | null;
} {
  const owners = new Map<string, string>();

  const remote = (id: string): CoreTransport => {
    const transport = registry.transport(id);
    if (!transport)
      throw new Error("Remote environment is no longer configured");
    return transport;
  };

  const callEnvironment = async <T>(
    environmentId: string,
    name: string,
    args: unknown
  ): Promise<T> => {
    try {
      const result = await remote(environmentId).call<T>(name, args, null);
      registry.reportStatus(environmentId, "online");
      return result;
    } catch (error) {
      registry.reportStatus(environmentId, "offline", error);
      throw error;
    }
  };

  const tagged = (environmentId: string, sessions: unknown): unknown[] => {
    if (!Array.isArray(sessions)) return [];
    return (sessions as unknown[]).map((session): unknown => {
      const record = argsObject(session);
      if (!record || typeof record.id !== "string") return session;
      owners.set(record.id, environmentId);
      return {
        ...withEnvironmentPaths(environmentId, record),
        environment_id: environmentId,
      };
    });
  };

  const core = {
    ownerOf: (session: string) => owners.get(session) ?? null,
    callEnvironment,

    async call<T>(
      name: string,
      args: unknown,
      projectPath: string | null
    ): Promise<T> {
      const record = argsObject(args);

      // A folder that belongs to a remote machine is served by that machine.
      const remoteFolder = parseEnvironmentPath(record?.cwd);
      if (record && remoteFolder) {
        if (!registry.get(remoteFolder.environmentId)) {
          throw new Error("Remote environment is no longer configured");
        }
        if (name === "terminal.spawn") {
          if (!terminals) throw new Error("Remote terminals are unavailable");
          return asResult<T>(
            await terminals.spawn({
              environmentId: remoteFolder.environmentId,
              id: String(record.id),
              cwd: remoteFolder.path,
              rows: Number(record.rows),
              cols: Number(record.cols),
              tmuxSession:
                typeof record.tmux_session === "string"
                  ? record.tmux_session
                  : null,
              projectPath,
            })
          );
        }
        return await callEnvironment<T>(remoteFolder.environmentId, name, {
          ...record,
          cwd: remoteFolder.path,
        });
      }

      const terminalId = record?.id;
      if (
        terminals &&
        typeof terminalId === "string" &&
        terminals.owns(terminalId)
      ) {
        switch (name) {
          case "terminal.write": {
            terminals.write(terminalId, String(record?.data ?? ""));
            return asResult<T>(null);
          }
          case "terminal.resize": {
            terminals.resize(
              terminalId,
              Number(record?.rows),
              Number(record?.cols)
            );
            return asResult<T>(null);
          }
          case "terminal.kill": {
            terminals.kill(terminalId);
            return asResult<T>(null);
          }
          case "terminal.dump": {
            // The remote shell's scrollback is replayed on attach; there is no separate text dump.
            return asResult<T>("");
          }
          default: {
            break;
          }
        }
      }

      if (
        NEW_SESSION_COMMANDS.has(name) &&
        typeof record?.environment_id === "string"
      ) {
        const { environment_id: environmentId, ...rest } = record;
        return await callEnvironment<T>(environmentId, name, rest);
      }

      const session = record?.session;
      if (typeof session === "string") {
        const owner = owners.get(session);
        if (owner !== undefined && registry.get(owner)) {
          return await callEnvironment<T>(owner, name, args);
        }
        // The owning environment was removed; the id no longer routes anywhere.
        owners.delete(session);
      }

      const environments = registry.list();
      if (environments.length === 0) {
        return await local.call<T>(name, args, projectPath);
      }

      if (AGGREGATED_SESSION_LISTS.has(name)) {
        const [localResult, ...remoteResults] = await Promise.all([
          local.call<unknown>(name, args, projectPath),
          ...environments.map(async (environment): Promise<unknown[]> => {
            try {
              return tagged(
                environment.id,
                await callEnvironment<unknown>(environment.id, name, args)
              );
            } catch {
              // An offline machine must not hide the sessions that are reachable.
              return [];
            }
          }),
        ]);
        return asResult<T>(concatRows(localResult, ...remoteResults));
      }

      if (name === "sessions.previews") {
        // Rows of `[session, preview]`; ids are unique across machines.
        const [localResult, ...remoteResults] = await Promise.all([
          local.call<unknown>(name, args, projectPath),
          ...environments.map(async (environment): Promise<unknown[]> => {
            try {
              const rows = await callEnvironment<unknown>(
                environment.id,
                name,
                args
              );
              return concatRows(rows);
            } catch {
              return [];
            }
          }),
        ]);
        return asResult<T>(concatRows(localResult, ...remoteResults));
      }

      return await local.call<T>(name, args, projectPath);
    },

    listen(name: string, listener: (payload: unknown) => void): () => void {
      const stopLocal = local.listen(name, listener);
      if (terminals && TERMINAL_EVENTS.has(name)) {
        const stopRemote = terminals.subscribe(name, listener);
        return () => {
          stopRemote();
          stopLocal();
        };
      }
      if (name !== "engine-event") return stopLocal;

      const detach = new Map<string, () => void>();
      const attach = () => {
        const present = new Set(
          registry.list().map((environment) => environment.id)
        );
        for (const [id, stop] of detach) {
          if (present.has(id)) continue;
          stop();
          detach.delete(id);
        }
        for (const id of present) {
          if (detach.has(id)) continue;
          const transport = registry.transport(id);
          if (!transport) continue;
          detach.set(
            id,
            transport.listen(name, (payload) => {
              const event = argsObject(payload);
              if (typeof event?.session === "string")
                owners.set(event.session, id);
              registry.reportStatus(id, "online");
              listener(
                event?.event === "session_created"
                  ? withEnvironmentPaths(id, event)
                  : payload
              );
            })
          );
        }
      };
      attach();
      const stopWatching = registry.subscribe(attach);
      return () => {
        stopWatching();
        for (const stop of detach.values()) stop();
        detach.clear();
        stopLocal();
      };
    },
  };
  return core;
}
