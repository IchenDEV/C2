import type {
  CoreEvent,
  ElicitationForm,
  PermissionContext,
  ResolvedWorktreeBaseline,
  SessionActivity,
  SessionInfo,
  WorktreeBaselineKind,
  WorktreeBaselineOption,
} from "../bridge";

/**
 * One thing the agent is waiting on the user for. Permissions and structured questions (ACP
 * elicitations) share the queue because they share the rule that matters: one turn, one blocked
 * agent, answered oldest first. `form` is what tells them apart at render time.
 */
export interface PermissionQueueItem {
  session: string;
  requestId: string;
  title: string;
  options: [string, string][];
  sequence?: number;
  context?: PermissionContext;
  /** Set when this is a question to answer rather than a permission to grant. */
  form?: ElicitationForm | null;
}

export const IDLE_SESSION_ACTIVITY: SessionActivity = {
  revision: 0,
  state: { kind: "idle" },
};

export function sessionActivity(
  session: Pick<SessionInfo, "activity">
): SessionActivity {
  return session.activity ?? IDLE_SESSION_ACTIVITY;
}

export function activityIsBusy(activity: SessionActivity | undefined): boolean {
  const kind = activity?.state.kind ?? "idle";
  return kind === "running" || kind === "awaiting_input";
}

/** Never let a late list snapshot roll a session back behind a newer activity event. */
export function latestActivity(
  current: SessionActivity | undefined,
  incoming: SessionActivity | undefined
): SessionActivity {
  if (!incoming) return current ?? IDLE_SESSION_ACTIVITY;
  if (!current || incoming.revision >= current.revision) return incoming;
  return current;
}

/** Rebuild the globally ordered, actionable input queue from authoritative session activity. */
export function permissionsFromSessions(
  sessions: readonly Pick<SessionInfo, "id" | "activity">[]
): PermissionQueueItem[] {
  return sessions
    .flatMap((session) => {
      const { state } = sessionActivity(session);
      if (state.kind !== "awaiting_input") return [];
      return state.pending.map((pending) => ({
        session: session.id,
        requestId: pending.input_id,
        title: pending.title,
        options: pending.options,
        sequence: pending.sequence,
        context: pending.context,
        form: pending.form,
      }));
    })
    .toSorted((left, right) => (left.sequence ?? 0) - (right.sequence ?? 0));
}

/** Project the application-wide pending-input queue onto the chat the user is viewing. */
export function pendingInputsForSession(
  queue: readonly PermissionQueueItem[],
  session: string | null
): PermissionQueueItem[] {
  if (session == null || session === "") return [];
  return queue.filter((request) => request.session === session);
}

/** Replace one session's pending inputs after an authoritative activity transition. */
export function permissionQueueAfterActivity(
  queue: readonly PermissionQueueItem[],
  session: string,
  activity: SessionActivity
): PermissionQueueItem[] {
  const pending = permissionsFromSessions([{ id: session, activity }]);
  return [
    ...queue.filter((request) => request.session !== session),
    ...pending,
  ].toSorted((left, right) => (left.sequence ?? 0) - (right.sequence ?? 0));
}

/** Keep a prompt visible until the host confirms that it accepted the answer. */
export function permissionQueueAfterAnswer(
  queue: readonly PermissionQueueItem[],
  session: string,
  requestId: string,
  accepted: boolean
): PermissionQueueItem[] {
  if (!accepted) return [...queue];
  return queue.filter(
    (request) => request.session !== session || request.requestId !== requestId
  );
}

/** Preserve concurrent permission requests while updating a repeated request in place. */
export function enqueuePermission(
  queue: PermissionQueueItem[],
  request: PermissionQueueItem
): PermissionQueueItem[] {
  const existing = queue.findIndex(
    (item) =>
      item.session === request.session && item.requestId === request.requestId
  );
  return existing === -1
    ? [...queue, request]
    : queue.map((item, index) => (index === existing ? request : item));
}

/**
 * Permission prompts are globally actionable; rendered turn state belongs to the active id, or —
 * once the column tiles — to any session currently bound to a pane. `paneSessions`, when supplied,
 * broadens only the final "is this turn content mine" check so background panes accumulate their
 * transcript; every earlier special case (creation events, correlated global failures) is unchanged.
 */
export function shouldRenderSessionEvent(
  event: CoreEvent,
  activeSession: string | null,
  awaitingCreationRequest: string | null = null,
  paneSessions: ReadonlySet<string> | null = null
): boolean {
  if (
    event.event === "permission_request" ||
    event.event === "elicitation_request"
  )
    return true;
  if (
    event.event === "session_created" ||
    event.event === "session_title_changed" ||
    event.event === "session_activity_changed" ||
    // Task snapshots carry `sessions` (all subjects), not one session; they refetch state and
    // never render as transcript rows.
    event.event === "task_snapshot_changed"
  )
    return false;
  // Session creation is broadcast to every connected client. A correlated global failure is
  // transcript-worthy only for the client that owns that request.
  if (
    event.event === "error" &&
    event.session === null &&
    event.request_id != null
  ) {
    return (
      awaitingCreationRequest !== null &&
      event.request_id === awaitingCreationRequest
    );
  }
  if (event.session === null || event.session === activeSession) return true;
  return paneSessions !== null && paneSessions.has(event.session);
}

/** Resolve the sole pane that owns a session; selection uses this to prevent duplicate bindings. */
export function paneBoundToSession(
  panes: Readonly<Record<string, { sessionId: string | null }>>,
  session: string
): string | null {
  for (const [paneId, content] of Object.entries(panes)) {
    if (content.sessionId === session) return paneId;
  }
  return null;
}

/** Only an explicit turn end or a terminal error may clear a session's running state. */
export function isTerminalSessionEvent(event: CoreEvent): boolean {
  return (
    event.event === "turn_ended" || (event.event === "error" && event.terminal)
  );
}

/** Broadcast creation events belong to this window only when the request ids agree exactly. */
export function matchesSessionCreation(
  event: Extract<CoreEvent, { event: "session_created" }>,
  requestId: string | null
): boolean {
  return requestId !== null && event.request_id === requestId;
}

/** Stable project identity for rail grouping; legacy worktrees cannot be inferred safely. */
export function sessionProjectPath(
  session: Pick<SessionInfo, "cwd" | "worktree_path" | "project_path">
): string | null {
  return (
    session.project_path ??
    (session.worktree_path === null ? session.cwd : null)
  );
}

/**
 * Resolve the source checkout for a new session without ever nesting a worktree checkout.
 * Legacy worktree records have no trustworthy source, so the caller must ask for a project.
 */
export function sessionCreationSource(
  activeProject: string | null,
  cwd: string,
  current:
    | Pick<SessionInfo, "cwd" | "worktree_path" | "project_path">
    | null
    | undefined,
  hasActiveSession = false
): string | null {
  // An active id without either a list shell or a correlated creation receipt is ambiguous: `cwd`
  // may be inside an isolated checkout, so using it would create a nested worktree.
  if (hasActiveSession && !current) return null;
  if (activeProject != null && activeProject !== "") return activeProject;
  if (current?.worktree_path !== null && current?.worktree_path !== undefined) {
    return current.project_path ?? null;
  }
  return (
    (current?.project_path != null && current.project_path !== ""
      ? current.project_path
      : null) ??
    (current?.cwd != null && current.cwd !== "" ? current.cwd : null) ??
    (cwd != null && cwd !== "" ? cwd : null) ??
    "."
  );
}

/** Carry a baseline into a new draft only when the current session records its meaning exactly. */
export function sessionCreationBaseline(
  current:
    | Pick<
        SessionInfo,
        "worktree_path" | "worktree_baseline" | "worktree_identity"
      >
    | null
    | undefined
): WorktreeBaselineKind | null | undefined {
  if (!current) return undefined;
  if (current.worktree_path === null) return null;
  // A ref+SHA proves the commit a legacy checkout started from, not that the pathname still names
  // the same directory. Do not treat it as a fully verified worktree without durable FS identity.
  if (!current.worktree_identity) return undefined;
  return current.worktree_baseline?.kind;
}

/**
 * The reason the worktree picker must stay Off, or null when it can offer something. Every
 * baseline resolving unavailable means the directory cannot host a worktree at all (usually: not
 * a git repository), so a choice would only defer the same failure to session creation.
 */
export function worktreeGatingReason(
  hasSession: boolean,
  options: WorktreeBaselineOption[],
  loading: boolean
): string | null {
  if (hasSession || loading || options.length === 0) return null;
  if (options.some((option) => option.unavailable_reason === null)) return null;
  return options[0].unavailable_reason;
}

/** Pin creation to the exact commit shown by the current baseline preview. */
export function sessionCreationBaselineSha(
  baseline: WorktreeBaselineKind | null,
  options: WorktreeBaselineOption[],
  loading: boolean
): string | null | undefined {
  if (baseline === null) return null;
  if (loading) return undefined;
  return options.find((option) => option.kind === baseline)?.resolved?.sha;
}

export type SessionCreationShell = Pick<
  SessionInfo,
  | "cwd"
  | "project_path"
  | "worktree_path"
  | "worktree_baseline"
  | "worktree_identity"
>;

/** Keep the durable creation receipt visible until the best-effort list supplies a full shell. */
export function sessionShellWithReceipt(
  activeSession: string | null,
  stored: SessionCreationShell | undefined,
  receipt: { session: string; shell: SessionCreationShell } | null
): SessionCreationShell | undefined {
  return (
    stored ?? (receipt?.session === activeSession ? receipt.shell : undefined)
  );
}

/** Derive the Composer's current-session worktree display from a list shell or live receipt. */
export function activeSessionWorktreeState(
  activeSession: string | null,
  stored: SessionCreationShell | undefined,
  receipt: { session: string; shell: SessionCreationShell } | null
): { baseline: ResolvedWorktreeBaseline | null; legacyUnknown: boolean } {
  const shell = sessionShellWithReceipt(activeSession, stored, receipt);
  if (shell?.worktree_path == null || shell?.worktree_path === "")
    return { baseline: null, legacyUnknown: false };
  return {
    baseline: shell.worktree_baseline ?? null,
    // A persisted row must carry filesystem identity. A just-created receipt has not reached the
    // list yet, so its bundled baseline is sufficient to render the creation result truthfully.
    legacyUnknown: stored
      ? !stored.worktree_identity
      : !shell.worktree_baseline,
  };
}

/** Build a trustworthy shell from the correlated creation receipt, rejecting legacy producers. */
export function sessionCreationReceipt(
  event: Extract<CoreEvent, { event: "session_created" }>
): SessionCreationShell | null {
  // New producers always persist a source project. Its absence means the event predates provenance
  // receipts; `cwd` alone cannot distinguish a normal checkout from an isolated worktree.
  if (
    event.cwd == null ||
    event.cwd === "" ||
    event.project_path == null ||
    event.project_path === ""
  )
    return null;
  return {
    cwd: event.cwd,
    project_path: event.project_path,
    worktree_path: event.worktree_path ?? null,
    worktree_baseline: event.worktree_baseline ?? null,
  };
}
