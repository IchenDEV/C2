import type {
  CoreEvent,
  DocBlock,
  MemoryReceipt,
  Part,
  ToolOutput,
  TranscriptEntry,
} from "../bridge";
import { isAgentActivityTitle } from "./agentActivity";

/** Only an accepted TurnStarted may dispose mutable Composer Canvas heads. */
export function canvasIdsToPurgeAfterTurnStart(
  accepted: boolean,
  canvasIds: readonly string[],
  editorUnchanged = true
): string[] {
  return accepted && editorUnchanged
    ? [...new Set(canvasIds.filter((id) => id.length > 0))]
    : [];
}

/**
 * Unmounting is the last safe lifecycle boundary for a mutable Composer head.  A live head must
 * be tombstoned before purge; a head already tombstoned by document removal can go straight to
 * purge.  Frozen revisions remain immutable in core and are not represented by this plan.
 */
export function canvasUnmountPlan(
  hasMutableHead: boolean,
  alreadyTombstoned: boolean
): { tombstone: boolean; purge: boolean } {
  const mutable = hasMutableHead || alreadyTombstoned;
  return {
    tombstone: hasMutableHead && !alreadyTombstoned,
    purge: mutable,
  };
}

export interface CanvasFrozenRef {
  id: string;
  revision: number;
}

/** Provider-image failures are the only terminal errors that may offer a structure-only retry. */
export function isCanvasProviderImageError(message: string): boolean {
  return /provider.*image|image.*unsupported|ProviderImageUnsupported/iu.test(
    message
  );
}

/** Correlate the immutable accepted request with its explicit provider retry affordance. */
export function canvasRetryRefsForTerminal(
  kind: "error" | "success",
  message: string | undefined,
  refs: readonly CanvasFrozenRef[]
): CanvasFrozenRef[] {
  if (
    kind !== "error" ||
    message == null ||
    message === "" ||
    !isCanvasProviderImageError(message)
  )
    return [];
  return refs.map((ref) => ({ id: ref.id, revision: ref.revision }));
}

export function canvasAcceptedRequestKey(
  session: string,
  requestId: string
): string {
  return `${session}:${requestId}`;
}

/** A provider change after an accepted Canvas failure must stage a fresh session. */
export function canvasRetryTargetSession(
  activeSession: string | null,
  forceNewSession: boolean
): string | null {
  return forceNewSession ? null : activeSession;
}

/** Replace only Canvas references in a retry document; every non-Canvas block stays ordered and
 * byte-for-byte equivalent so an async provider failure never drops the user's instruction. */
export function canvasRetryDocument(
  doc: readonly DocBlock[],
  replacements: ReadonlyMap<string, CanvasFrozenRef>
): DocBlock[] {
  return doc.map((block) => {
    if (block.type !== "canvas") return block;
    const replacement = replacements.get(block.id);
    return replacement
      ? { ...block, id: replacement.id, frozen_revision: replacement.revision }
      : block;
  });
}

export interface ToolEntry {
  id: string;
  title: string;
  /** First title that explicitly identified delegated activity, retained across status updates. */
  activityTitle?: string;
  status: string;
  kind?: string | null;
  agentInput?: unknown;
  outputs?: ToolOutput[];
  startedAt?: number;
  endedAt?: number;
  /** Last durable row folded into this call; older snapshot/live updates cannot regress it. */
  lastTranscriptSeq?: number;
}

/**
 * The render-order projection of one turn. Text chunks stay as independent atoms so a tool call
 * can sit between two streamed answer fragments without splitting the durable assistant message.
 * Repeated tool updates keep one position and update the matching ToolEntry in place.
 */
export type TurnContentEntry =
  | { kind: "text"; text: string; transcriptSeq?: number; createdAt?: number }
  | {
      kind: "tool";
      toolId: string;
      transcriptSeq?: number;
      createdAt?: number;
    };

export interface PromptImage {
  id: string;
  name?: string;
  previewDataUrl?: string;
  width?: number;
  height?: number;
}

const PROMPT_ATTACHMENT_MARKER =
  /\[attachment:([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\]/giu;

function promptImagesFromCanonicalText(text: string): PromptImage[] {
  return Array.from(text.matchAll(PROMPT_ATTACHMENT_MARKER), (match) => ({
    id: match[1],
  }));
}

/**
 * One prompt → response cycle. The engine streams fragments; grouping them into turns is what makes
 * the transcript readable (previously every text chunk became its own row).
 */
export interface Turn {
  id: number;
  /** Durable user-row boundary. Present for turns reconstructed from a transcript page. */
  transcriptStartSeq?: number;
  /** Prompt correlation id when the core/client supplied one. */
  requestId?: string;
  /** False for an optimistic local row until its matching TurnStarted arrives. */
  accepted: boolean;
  /** Client/provider delivery state before this prompt becomes the active response phase. */
  delivery?: "queued" | "steer";
  queuePosition?: number;
  /** True when live deltas were observed from this turn's explicit TurnStarted boundary. */
  streamBoundaryKnown: boolean;
  prompt: string;
  /** Private prompt images rendered beside the user-authored text, never as marker strings. */
  promptImages?: PromptImage[];
  text: string;
  /** Exact ACP text chunks; merge by boundary/count, never by substring equality. */
  textDeltas: string[];
  /** Live chunks observed since the current stream boundary, including count-skipped replays. */
  observedTextDeltas: number;
  observedThoughtDeltas: number;
  /** Persisted chunks that raced ahead of IPC delivery and should be skipped by position. */
  pendingTextDeltaSkips: number;
  pendingThoughtDeltaSkips: number;
  thoughts: string[];
  tools: ToolEntry[];
  content: TurnContentEntry[];
  memory?: MemoryReceipt;
  error?: string;
  stopReason?: string;
  startedAt: number;
  endedAt?: number;
}

let nextId = 1;

export function newTurn(
  prompt: string,
  requestId?: string,
  promptImages: PromptImage[] = []
): Turn {
  return {
    id: (nextId += 1),
    requestId,
    accepted: false,
    streamBoundaryKnown: false,
    prompt,
    promptImages,
    text: "",
    textDeltas: [],
    observedTextDeltas: 0,
    observedThoughtDeltas: 0,
    pendingTextDeltaSkips: 0,
    pendingThoughtDeltaSkips: 0,
    thoughts: [],
    tools: [],
    content: [],
    startedAt: Date.now(),
  };
}

/** Immutable Set update used by App for both its synchronous ref and rendered state. */
export function withRunningSession(
  current: ReadonlySet<string>,
  session: string,
  running: boolean
): Set<string> {
  const next = new Set(current);
  if (running) next.add(session);
  else next.delete(session);
  return next;
}

/** Drop only the optimistic row owned by a cancelled creation request. */
export function withoutUnacceptedTurn(
  turns: Turn[],
  requestId: string
): Turn[] {
  return turns.filter((turn) => turn.requestId !== requestId || turn.accepted);
}

/** Project the session-level state onto a persisted tail without treating optimism as acceptance. */
export function transcriptTailState(
  sessionRunning: boolean,
  authoritativeTurnKnown: boolean,
  latestRequestId?: string
): { running: boolean; requestId?: string } {
  return {
    running: sessionRunning && authoritativeTurnKnown,
    requestId:
      sessionRunning && !authoritativeTurnKnown ? undefined : latestRequestId,
  };
}

/** Compare the submitted editor revision without depending on object key insertion order. */
export function sameDocBlocks(
  a: readonly DocBlock[],
  b: readonly DocBlock[]
): boolean {
  if (a.length !== b.length) return false;
  return a.every((left, index) => {
    const right = b[index];
    if (right == null || left.type !== right.type) return false;
    switch (left.type) {
      case "text": {
        return right.type === "text" && left.text === right.text;
      }
      case "file":
      case "image": {
        return right.type === left.type && left.path === right.path;
      }
      case "session": {
        return (
          right.type === "session" &&
          left.session_id === right.session_id &&
          left.through_seq === right.through_seq
        );
      }
      case "canvas": {
        return (
          right.type === "canvas" &&
          left.id === right.id &&
          left.frozen_revision === right.frozen_revision &&
          left.pixel_policy === right.pixel_policy
        );
      }
      case "skill": {
        if (right.type !== "skill" || left.skill_id !== right.skill_id)
          return false;
        const leftKeys = Object.keys(left.params).toSorted();
        const rightKeys = Object.keys(right.params).toSorted();
        return (
          leftKeys.length === rightKeys.length &&
          leftKeys.every(
            (key, keyIndex) =>
              key === rightKeys[keyIndex] &&
              left.params[key] === right.params[key]
          )
        );
      }
      case "appshot": {
        throw new Error('Not implemented yet: "appshot" case');
      }
      case "attachment": {
        throw new Error('Not implemented yet: "attachment" case');
      }
      case "issue": {
        throw new Error('Not implemented yet: "issue" case');
      }
    }
  });
}

/** Exact content plus editor revision prevents equal-looking post-submit edits from being erased. */
export function matchesSubmittedEditorRevision(
  current: readonly DocBlock[],
  currentRevision: number,
  submitted: readonly DocBlock[],
  submittedRevision: number
): boolean {
  return (
    currentRevision === submittedRevision && sameDocBlocks(current, submitted)
  );
}

/** Is this turn still in flight? */
export function isRunning(t: Turn | undefined): boolean {
  return !!t && t.endedAt === undefined;
}

interface ToolUpdate {
  id: string;
  title: string;
  status: string;
  kind?: string | null;
  agentInput?: unknown;
  outputs?: ToolOutput[];
  transcriptSeq?: number | null;
  startedAt?: number;
  endedAt?: number;
}

function terminalToolStatus(status: string): boolean {
  return [
    "completed",
    "failed",
    "cancelled",
    "canceled",
    "rejected",
    "denied",
  ].includes(status.trim().toLowerCase());
}

/** Upsert a streamed/persisted tool call without dropping metadata on status-only updates. */
function upsertTool(tools: ToolEntry[], update: ToolUpdate): ToolEntry[] {
  const existing = tools.find((tool) => tool.id === update.id);
  if (
    existing?.lastTranscriptSeq !== undefined &&
    update.transcriptSeq != null &&
    update.transcriptSeq <= existing.lastTranscriptSeq
  ) {
    return tools;
  }
  const entry: ToolEntry = {
    ...existing,
    id: update.id,
    title: (update.title || existing?.title) ?? update.id,
    activityTitle:
      existing?.activityTitle ??
      (isAgentActivityTitle(update.title) ? update.title : undefined),
    status: (update.status || existing?.status) ?? "pending",
    outputs: mergeToolOutputs(existing?.outputs ?? [], update.outputs ?? []),
    startedAt: existing?.startedAt ?? update.startedAt,
    endedAt: update.endedAt ?? existing?.endedAt,
  };
  if (update.kind != null) entry.kind = update.kind;
  if (update.agentInput != null) entry.agentInput = update.agentInput;
  if (update.transcriptSeq != null)
    entry.lastTranscriptSeq = update.transcriptSeq;
  return existing
    ? tools.map((tool) => (tool.id === update.id ? entry : tool))
    : [...tools, entry];
}

function mergeToolOutputs(
  current: ToolOutput[],
  incoming: ToolOutput[]
): ToolOutput[] {
  const output = [...current];
  for (const item of incoming) {
    const duplicate = output.some((existing) => {
      if (existing.type !== item.type) return false;
      if (item.type === "image" && existing.type === "image") {
        return existing.artifact.id === item.artifact.id;
      }
      if (item.type === "resource_link" && existing.type === "resource_link") {
        return existing.uri === item.uri;
      }
      return (
        item.type === "text" &&
        existing.type === "text" &&
        existing.text === item.text
      );
    });
    if (!duplicate) output.push(item);
  }
  return output;
}

function appendTextContent(
  content: readonly TurnContentEntry[],
  text: string,
  transcriptSeq?: number | null,
  createdAt?: number
): TurnContentEntry[] {
  return [
    ...content,
    {
      kind: "text" as const,
      text,
      ...(transcriptSeq == null ? {} : { transcriptSeq }),
      ...(createdAt == null ? {} : { createdAt }),
    },
  ];
}

function appendToolContent(
  content: readonly TurnContentEntry[],
  toolId: string,
  transcriptSeq?: number | null,
  createdAt?: number
): TurnContentEntry[] {
  if (
    content.some((entry) => entry.kind === "tool" && entry.toolId === toolId)
  ) {
    return [...content];
  }
  return [
    ...content,
    {
      kind: "tool" as const,
      toolId,
      ...(transcriptSeq == null ? {} : { transcriptSeq }),
      ...(createdAt == null ? {} : { createdAt }),
    },
  ];
}

/**
 * Fold a streamed event into the turn list. Text accumulates into the open turn rather than
 * appending a new row, and tool calls are upserted by id so status updates in place.
 */
export function applyEvent(
  turns: Turn[],
  ev: CoreEvent,
  activeRequestId?: string
): Turn[] {
  // Events that don't belong to a turn.
  if (
    ev.event === "provider_changed" ||
    ev.event === "session_created" ||
    ev.event === "session_title_changed" ||
    ev.event === "session_activity_changed" ||
    ev.event === "usage" ||
    ev.event === "models" ||
    ev.event === "config_options" ||
    ev.event === "session_capabilities" ||
    ev.event === "permission_request"
  ) {
    return turns;
  }

  const requestId =
    "request_id" in ev ? (ev.request_id ?? undefined) : undefined;

  // A start only accepts the optimistic row with the same correlation id. A foreign start owns a
  // separate row, so its output can never bind to a prompt this client lost the core-side race for.
  if (ev.event === "turn_started") {
    let match = -1;
    if (requestId != null && requestId !== "") {
      for (let index = turns.length - 1; index >= 0; index -= 1) {
        if (turns[index].requestId === requestId) {
          match = index;
          break;
        }
      }
    } else {
      const tail = turns.at(-1)!;
      if (isRunning(tail) && (tail.requestId == null || tail.requestId === ""))
        match = turns.length - 1;
    }
    if (match >= 0) {
      const list = [...turns];
      const accepted = {
        ...list[match],
        accepted: true,
        streamBoundaryKnown: true,
        delivery: undefined,
        queuePosition: undefined,
      };
      delete accepted.endedAt;
      list[match] = accepted;
      return list;
    }
    const remote = newTurn("(started elsewhere)", requestId);
    remote.accepted = true;
    remote.streamBoundaryKnown = true;
    return [...turns, remote];
  }

  if (ev.event === "prompt_queued") {
    const match =
      requestId != null && requestId !== ""
        ? turns.findIndex((turn) => turn.requestId === requestId)
        : -1;
    if (match < 0) return turns;
    const list = [...turns];
    list[match] = {
      ...list[match],
      delivery: "queued",
      queuePosition: ev.position,
    };
    return list;
  }

  if (ev.event === "steer_accepted") {
    const match =
      requestId != null && requestId !== ""
        ? turns.findIndex((turn) => turn.requestId === requestId)
        : -1;
    if (match < 0) return turns;
    const list = turns.map((turn, index) =>
      index !== match && isRunning(turn) && turn.accepted
        ? { ...turn, endedAt: Date.now() }
        : turn
    );
    list[match] = {
      ...list[match],
      accepted: true,
      delivery: "steer",
      streamBoundaryKnown: true,
      transcriptStartSeq: ev.transcript_seq ?? list[match].transcriptStartSeq,
    };
    return list;
  }

  if (ev.event === "error" && requestId != null && requestId !== "") {
    let match = -1;
    for (let index = turns.length - 1; index >= 0; index -= 1) {
      if (turns[index].requestId === requestId) {
        match = index;
        break;
      }
    }
    if (match >= 0) {
      const list = [...turns];
      const cur = { ...list[match], error: ev.message };
      // A non-terminal error before acceptance is a correlated rejection (normally `busy`). It
      // ends only that optimistic row; warnings after TurnStarted keep the accepted turn open.
      if (ev.terminal || !cur.accepted) cur.endedAt = Date.now();
      list[match] = cur;
      return list;
    }
    // Correlated errors with no known turn are bounded notices, never activity that can absorb
    // another client's streamed output.
    const notice = newTurn("(session notice)", requestId);
    notice.error = ev.message;
    notice.endedAt = Date.now();
    return [...turns, notice];
  }

  // A warning emitted while the session is idle is a completed notice, not a phantom running turn.
  if (ev.event === "error" && !ev.terminal && !isRunning(turns.at(-1))) {
    const notice = newTurn("(session notice)");
    notice.error = ev.message;
    notice.endedAt = Date.now();
    return [...turns, notice];
  }

  // Backward compatibility for event producers that predate `turn_started`: activity after an
  // ended transcript still starts a fresh remote turn instead of mutating the previous one.
  const list = [...turns];
  let i = -1;
  if (activeRequestId != null && activeRequestId !== "") {
    for (let index = list.length - 1; index >= 0; index -= 1) {
      if (isRunning(list[index]) && list[index].requestId === activeRequestId) {
        i = index;
        break;
      }
    }
    if (i < 0) {
      const remote = newTurn("(started elsewhere)", activeRequestId);
      remote.accepted = true;
      list.push(remote);
      i = list.length - 1;
    }
  }
  if (i < 0) {
    for (let index = list.length - 1; index >= 0; index -= 1) {
      if (isRunning(list[index]) && list[index].accepted) {
        i = index;
        break;
      }
    }
  }
  if (i < 0) {
    for (let index = list.length - 1; index >= 0; index -= 1) {
      if (isRunning(list[index])) {
        i = index;
        break;
      }
    }
  }
  if (i < 0) {
    const remote = newTurn("(started elsewhere)");
    remote.accepted = true;
    list.push(remote);
    i = list.length - 1;
  }
  const cur = { ...list[i] };
  const observedAt = Date.now();

  switch (ev.event) {
    case "memory_context": {
      cur.memory = ev.receipt;
      break;
    }
    case "agent_text": {
      cur.observedTextDeltas += 1;
      if (cur.pendingTextDeltaSkips > 0) {
        cur.pendingTextDeltaSkips -= 1;
      } else {
        cur.text += ev.text;
        cur.textDeltas = [...cur.textDeltas, ev.text];
        cur.content = appendTextContent(
          cur.content,
          ev.text,
          ev.transcript_seq,
          observedAt
        );
      }
      break;
    }
    case "agent_thought": {
      cur.observedThoughtDeltas += 1;
      if (cur.pendingThoughtDeltaSkips > 0) {
        cur.pendingThoughtDeltaSkips -= 1;
      } else {
        cur.thoughts = [...cur.thoughts, ev.text];
      }
      break;
    }
    case "tool_call": {
      cur.tools = upsertTool(cur.tools, {
        id: ev.id,
        title: ev.title,
        status: ev.status,
        kind: ev.kind,
        agentInput: ev.agent_input,
        outputs: ev.outputs,
        transcriptSeq: ev.transcript_seq,
        startedAt: observedAt,
        endedAt: terminalToolStatus(ev.status) ? observedAt : undefined,
      });
      cur.content = appendToolContent(
        cur.content,
        ev.id,
        ev.transcript_seq,
        observedAt
      );
      break;
    }
    case "turn_ended": {
      cur.stopReason = ev.stop_reason;
      cur.endedAt = Date.now();
      break;
    }
    case "error": {
      cur.error = ev.message;
      if (ev.terminal) cur.endedAt = Date.now();
      break;
    }
    case "artifact_produced": {
      // A scene-layer fact, never a transcript part: the artifact surfaces through the session's
      // artifact list, so turn projection deliberately ignores it.
      break;
    }
    case "context_window": {
      throw new Error('Not implemented yet: "context_window" case');
    }
    case "elicitation_request": {
      throw new Error('Not implemented yet: "elicitation_request" case');
    }
    case "execution_policy_changed": {
      throw new Error('Not implemented yet: "execution_policy_changed" case');
    }
    case "exit_criteria_met": {
      throw new Error('Not implemented yet: "exit_criteria_met" case');
    }
    case "hook_suggestion": {
      throw new Error('Not implemented yet: "hook_suggestion" case');
    }
    case "hook_turn_started": {
      throw new Error('Not implemented yet: "hook_turn_started" case');
    }
    case "session_cost": {
      throw new Error('Not implemented yet: "session_cost" case');
    }
    case "task_snapshot_changed": {
      throw new Error('Not implemented yet: "task_snapshot_changed" case');
    }
    case "test_signal": {
      throw new Error('Not implemented yet: "test_signal" case');
    }
    case "worktree_discarded": {
      throw new Error('Not implemented yet: "worktree_discarded" case');
    }
  }

  list[i] = cur;
  return list;
}

function mergeDeltas(
  snapshot: string[],
  live: string[],
  observedLiveCount: number,
  boundaryKnown: boolean
): { deltas: string[]; pendingSkips: number } {
  // With an explicit TurnStarted boundary, the observed `live` sequence starts at the turn boundary
  // and the persisted snapshot contains a prefix of the same sequence. Counts preserve repeated equal
  // deltas. Persistence can also race ahead of IPC delivery; `pendingSkips` consumes those future
  // replays by position. Without that boundary, append conservatively: duplicating an uncertain
  // delta is safer than deleting distinct model output merely because the text happens to be equal.
  if (!boundaryKnown)
    return { deltas: [...snapshot, ...live], pendingSkips: 0 };
  return {
    deltas: [
      ...snapshot,
      ...live.slice(Math.min(snapshot.length, live.length)),
    ],
    pendingSkips: Math.max(0, snapshot.length - observedLiveCount),
  };
}

function mergeTurnContent(
  snapshot: readonly TurnContentEntry[],
  live: readonly TurnContentEntry[],
  boundaryKnown: boolean
): TurnContentEntry[] {
  if (snapshot.length === 0) return [...live];
  if (live.length === 0) return [...snapshot];

  const durable = [...snapshot, ...live].filter(
    (entry): entry is TurnContentEntry & { transcriptSeq: number } =>
      entry.transcriptSeq !== undefined
  );
  if (durable.length > 0) {
    const bySeq = new Map<number, TurnContentEntry>();
    for (const entry of [...live, ...snapshot]) {
      if (entry.transcriptSeq === undefined) continue;
      // Snapshot state wins an equal sequence because it was read after persistence.
      bySeq.set(entry.transcriptSeq, entry);
    }
    const ordered = [...bySeq.values()].toSorted(
      (left, right) => (left.transcriptSeq ?? 0) - (right.transcriptSeq ?? 0)
    );
    const seenTools = new Set<string>();
    const deduped = ordered.filter((entry) => {
      if (entry.kind === "text") return true;
      if (seenTools.has(entry.toolId)) return false;
      seenTools.add(entry.toolId);
      return true;
    });

    // A disabled/failed store can still produce sequence-less live output. Keep it after the
    // durable edge instead of silently dropping provider output.
    let textToSkip = snapshot.filter((entry) => entry.kind === "text").length;
    const durableTools = new Set(
      deduped.flatMap((entry) => (entry.kind === "tool" ? [entry.toolId] : []))
    );
    const sequenceLess = live.filter((entry) => {
      if (entry.transcriptSeq !== undefined) return false;
      if (entry.kind === "text" && boundaryKnown && textToSkip > 0) {
        textToSkip -= 1;
        return false;
      }
      if (entry.kind === "tool") {
        if (durableTools.has(entry.toolId)) return false;
        durableTools.add(entry.toolId);
      }
      return true;
    });
    if (sequenceLess.length === 0) return deduped;
    return [...deduped, ...mergeTurnContent([], sequenceLess, boundaryKnown)];
  }

  if (!boundaryKnown) return [...snapshot, ...live];

  let textToSkip = snapshot.filter((entry) => entry.kind === "text").length;
  const seenTools = new Set(
    snapshot.flatMap((entry) => (entry.kind === "tool" ? [entry.toolId] : []))
  );
  const tail = live.filter((entry) => {
    if (entry.kind === "text" && textToSkip > 0) {
      textToSkip -= 1;
      return false;
    }
    if (entry.kind === "tool") {
      if (seenTools.has(entry.toolId)) return false;
      seenTools.add(entry.toolId);
    }
    return true;
  });
  return [...snapshot, ...tail];
}

/** Merge events received during an async transcript read without dropping or duplicating its tail. */
export function mergeLoadedTurns(
  loaded: Turn[],
  live: Turn[],
  running: boolean
): Turn[] {
  if (live.length === 0) return loaded;
  if (loaded.length === 0) return live;

  const loadedTail = loaded.at(-1)!;
  if (loadedTail.requestId == null || loadedTail.requestId === "")
    return [...loaded, ...live];
  const liveIndex = live.findIndex(
    (turn) => turn.requestId === loadedTail.requestId
  );
  if (liveIndex === -1) return [...loaded, ...live];
  const liveTurn = live[liveIndex];

  let tools = [...loadedTail.tools];
  for (const tool of liveTurn.tools) {
    tools = upsertTool(tools, {
      ...tool,
      transcriptSeq: tool.lastTranscriptSeq,
    });
  }
  const textMerge = mergeDeltas(
    loadedTail.textDeltas,
    liveTurn.textDeltas,
    liveTurn.observedTextDeltas,
    liveTurn.streamBoundaryKnown
  );
  const thoughtMerge = mergeDeltas(
    loadedTail.thoughts,
    liveTurn.thoughts,
    liveTurn.observedThoughtDeltas,
    liveTurn.streamBoundaryKnown
  );
  const merged: Turn = {
    ...loadedTail,
    requestId: liveTurn.requestId,
    accepted: loadedTail.accepted || liveTurn.accepted,
    streamBoundaryKnown: liveTurn.streamBoundaryKnown,
    text: textMerge.deltas.join(""),
    textDeltas: textMerge.deltas,
    observedTextDeltas: liveTurn.observedTextDeltas,
    observedThoughtDeltas: liveTurn.observedThoughtDeltas,
    pendingTextDeltaSkips: textMerge.pendingSkips,
    pendingThoughtDeltaSkips: thoughtMerge.pendingSkips,
    thoughts: thoughtMerge.deltas,
    tools,
    content: mergeTurnContent(
      loadedTail.content,
      liveTurn.content,
      liveTurn.streamBoundaryKnown
    ),
    error: liveTurn.error ?? loadedTail.error,
    stopReason: liveTurn.stopReason ?? loadedTail.stopReason,
    startedAt: Math.min(loadedTail.startedAt, liveTurn.startedAt),
    endedAt: running ? undefined : (liveTurn.endedAt ?? loadedTail.endedAt),
  };
  return [
    ...loaded.slice(0, -1),
    ...live.slice(0, liveIndex),
    merged,
    ...live.slice(liveIndex + 1),
  ];
}

/** Rebuild turns from a persisted transcript: each user message starts a new turn. */
export function turnsFromTranscript(
  entries: readonly (TranscriptEntry | readonly [string, Part])[],
  lastTurnRunning = false,
  lastTurnRequestId?: string,
  receipts: readonly MemoryReceipt[] = []
): Turn[] {
  const out: Turn[] = [];
  const receiptBySeq = new Map(
    receipts.map((receipt) => [receipt.user_part_seq, receipt])
  );
  const push = (
    part: Part,
    role: string,
    seq?: number,
    createdAt?: number,
    startedAt?: number
  ) => {
    if (part.kind === "plan") return;
    const at = createdAt != null && createdAt > 0 ? createdAt : Date.now();
    if (role === "user" && (part.kind === "text" || part.kind === "prompt")) {
      out.push({
        ...newTurn(
          part.text,
          undefined,
          part.kind === "prompt" ? promptImagesFromCanonicalText(part.text) : []
        ),
        transcriptStartSeq: seq,
        accepted: true,
        memory: seq === undefined ? undefined : receiptBySeq.get(seq),
        startedAt: at,
        endedAt: at,
      });
      return;
    }
    if (out.length === 0) {
      out.push({
        ...newTurn("(earlier)"),
        accepted: true,
        endedAt: Date.now(),
      });
    }
    const cur = out.at(-1)!;
    switch (part.kind) {
      case "text": {
        cur.text += part.text;
        cur.textDeltas.push(part.text);
        cur.content = appendTextContent(cur.content, part.text, seq, at);
        break;
      }
      case "prompt": {
        cur.text += part.text;
        cur.textDeltas.push(part.text);
        cur.content = appendTextContent(cur.content, part.text, seq, at);
        break;
      }
      case "reasoning": {
        cur.thoughts.push(part.text);
        break;
      }
      case "tool_call": {
        cur.tools = upsertTool(cur.tools, {
          id: part.id,
          title: part.title,
          status: part.status,
          kind: part.tool_kind,
          agentInput: part.agent_input,
          outputs: part.outputs,
          transcriptSeq: seq,
          startedAt: startedAt != null && startedAt > 0 ? startedAt : at,
          endedAt: terminalToolStatus(part.status) ? at : undefined,
        });
        cur.content = appendToolContent(cur.content, part.id, seq, at);
        break;
      }
    }
    cur.endedAt = Math.max(cur.endedAt ?? at, at);
  };
  for (const entry of entries) {
    if ("part" in entry) {
      push(
        entry.part,
        entry.role,
        entry.seq,
        entry.created_at,
        entry.started_at
      );
    } else push(entry[1], entry[0]);
  }
  if (out.length > 0 && lastTurnRequestId != null && lastTurnRequestId !== "") {
    out.at(-1)!.requestId = lastTurnRequestId;
  }
  if (lastTurnRunning && out.length > 0) delete out.at(-1)!.endedAt;
  return out;
}

/** Prepend an older, non-overlapping page without touching the live tail merge machinery. */
export function prependTranscriptTurns(current: Turn[], older: Turn[]): Turn[] {
  if (older.length === 0) return current;
  const durableStarts = new Set(
    current
      .map((turn) => turn.transcriptStartSeq)
      .filter((seq): seq is number => seq !== undefined)
  );
  return [
    ...older.filter(
      (turn) =>
        turn.transcriptStartSeq === undefined ||
        !durableStarts.has(turn.transcriptStartSeq)
    ),
    ...current,
  ];
}
