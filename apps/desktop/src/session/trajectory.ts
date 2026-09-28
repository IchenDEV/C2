import type { ToolOutput } from "../bridge";
import type { ToolEntry, Turn, TurnContentEntry } from "./turns";

export type TrajectoryKind =
  | "user"
  | "assistant"
  | "reasoning"
  | "tool"
  | "memory"
  | "error";

export type TrajectoryLane = "context" | "assistant" | "tool";

export interface TrajectoryRecord {
  id: string;
  index: number;
  kind: TrajectoryKind;
  lane: TrajectoryLane;
  turn: number;
  step: number;
  title: string;
  summary: string;
  status?: string;
  startAt: number;
  endAt: number;
  running: boolean;
  input?: unknown;
  output?: unknown;
}

function compact(value: string, max = 160): string {
  const text = value.replaceAll(/\s+/gu, " ").trim();
  if (text.length <= max) return text;
  return `${text.slice(0, max - 1).trimEnd()}…`;
}

function toolOutputSummary(outputs: readonly ToolOutput[]): string {
  for (const output of outputs) {
    if (output.type === "text" && output.text.trim())
      return compact(output.text);
    if (output.type === "image") return output.artifact.display_name;
    if (output.type === "resource_link") return output.name || output.uri;
  }
  return "";
}

function timeFor(entry: TurnContentEntry, fallback: number): number {
  return entry.createdAt != null && entry.createdAt > 0
    ? entry.createdAt
    : fallback;
}

function toolRecord(
  tool: ToolEntry,
  entry: TurnContentEntry,
  turn: Turn,
  turnNumber: number,
  step: number,
  now: number
): TrajectoryRecord {
  const fallback = timeFor(entry, turn.startedAt);
  const startAt =
    tool.startedAt != null && tool.startedAt > 0 ? tool.startedAt : fallback;
  const running = tool.endedAt === undefined && turn.endedAt === undefined;
  const endAt = Math.max(startAt, tool.endedAt ?? (running ? now : fallback));
  const output = tool.outputs ?? [];
  return {
    id: `turn:${turn.id}:tool:${tool.id}`,
    index: 0,
    kind: "tool",
    lane: "tool",
    turn: turnNumber,
    step,
    title: (tool.title || tool.kind) ?? "Tool",
    summary: toolOutputSummary(output) || tool.status,
    status: tool.status,
    startAt,
    endAt,
    running,
    input: tool.agentInput,
    output,
  };
}

/** Build a dense business-event projection. Stream chunks collapse into assistant spans. */
export function deriveTrajectory(
  turns: readonly Turn[],
  now = Date.now()
): TrajectoryRecord[] {
  const records: TrajectoryRecord[] = [];

  turns.forEach((turn, turnIndex) => {
    const turnNumber = turnIndex + 1;
    const turnEnd = Math.max(turn.startedAt, turn.endedAt ?? now);
    records.push({
      id: `turn:${turn.id}:user`,
      index: 0,
      kind: "user",
      lane: "context",
      turn: turnNumber,
      step: 0,
      title: "User",
      summary: compact(turn.prompt),
      startAt: turn.startedAt,
      endAt: turn.startedAt,
      running: false,
      input: turn.prompt,
    });

    if (turn.memory) {
      const at =
        turn.memory.created_at > 0 ? turn.memory.created_at : turn.startedAt;
      records.push({
        id: `turn:${turn.id}:memory`,
        index: 0,
        kind: "memory",
        lane: "context",
        turn: turnNumber,
        step: 0,
        title: "Memory context",
        summary: `${turn.memory.items.length} items · ~${turn.memory.estimated_tokens} tokens`,
        startAt: at,
        endAt: at,
        running: false,
        output: turn.memory,
      });
    }

    if (turn.thoughts.length > 0) {
      const reasoning = turn.thoughts.join("");
      records.push({
        id: `turn:${turn.id}:reasoning`,
        index: 0,
        kind: "reasoning",
        lane: "assistant",
        turn: turnNumber,
        step: 1,
        title: "Reasoning",
        summary: compact(reasoning),
        startAt: turn.startedAt,
        endAt: turnEnd,
        running: turn.endedAt === undefined,
        output: reasoning,
      });
    }

    const tools = new Map(turn.tools.map((tool) => [tool.id, tool]));
    let step = 1;
    let assistantSegment = 0;
    let textEntries: Extract<TurnContentEntry, { kind: "text" }>[] = [];

    const flushAssistant = (terminal = false) => {
      if (textEntries.length === 0) return;
      assistantSegment += 1;
      const text = textEntries.map((entry) => entry.text).join("");
      const startAt = timeFor(textEntries[0], turn.startedAt);
      const lastAt = timeFor(textEntries.at(-1)!, startAt);
      const running = !terminal && turn.endedAt === undefined;
      records.push({
        id: `turn:${turn.id}:assistant:${assistantSegment}`,
        index: 0,
        kind: "assistant",
        lane: "assistant",
        turn: turnNumber,
        step,
        title: "Assistant",
        summary: compact(text),
        startAt,
        endAt: Math.max(startAt, running ? now : lastAt),
        running,
        output: text,
      });
      textEntries = [];
    };

    for (const entry of turn.content) {
      if (entry.kind === "text") {
        textEntries.push(entry);
        continue;
      }
      flushAssistant(true);
      const tool = tools.get(entry.toolId);
      if (tool)
        records.push(toolRecord(tool, entry, turn, turnNumber, step, now));
      step += 1;
    }
    flushAssistant(false);

    if (turn.content.length === 0 && turn.text.trim()) {
      records.push({
        id: `turn:${turn.id}:assistant:1`,
        index: 0,
        kind: "assistant",
        lane: "assistant",
        turn: turnNumber,
        step,
        title: "Assistant",
        summary: compact(turn.text),
        startAt: turn.startedAt,
        endAt: turnEnd,
        running: turn.endedAt === undefined,
        output: turn.text,
      });
    }

    if (turn.error != null && turn.error !== "") {
      records.push({
        id: `turn:${turn.id}:error`,
        index: 0,
        kind: "error",
        lane: "assistant",
        turn: turnNumber,
        step,
        title: "Error",
        summary: compact(turn.error),
        status: "failed",
        startAt: turnEnd,
        endAt: turnEnd,
        running: false,
        output: turn.error,
      });
    }
  });

  return records.map((record, index) => ({ ...record, index: index + 1 }));
}

export function filterTrajectory(
  records: readonly TrajectoryRecord[],
  kind: TrajectoryKind | "all",
  query: string
): TrajectoryRecord[] {
  const needle = query.trim().toLocaleLowerCase();
  return records.filter((record) => {
    if (kind !== "all" && record.kind !== kind) return false;
    if (!needle) return true;
    return [
      record.title,
      record.summary,
      record.status ?? "",
      `turn ${record.turn}`,
      `step ${record.step}`,
    ]
      .join(" ")
      .toLocaleLowerCase()
      .includes(needle);
  });
}

export function formatTrajectoryDuration(milliseconds: number): string {
  const value = Math.max(0, milliseconds);
  if (value < 1000) return `${Math.round(value)} ms`;
  if (value < 10_000) return `${(value / 1000).toFixed(1)} s`;
  const roundedSeconds = Math.round(value / 1000);
  if (roundedSeconds < 60) return `${roundedSeconds} s`;
  const minutes = Math.floor(roundedSeconds / 60);
  const seconds = roundedSeconds % 60;
  return `${minutes}m ${seconds}s`;
}
