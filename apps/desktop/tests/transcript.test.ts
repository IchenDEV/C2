import { describe, expect, test } from "bun:test";

import {
  LONG_PROMPT_MAX_CHARS,
  LONG_PROMPT_MAX_LINES,
  collapsedPrompt,
  isLongPrompt,
} from "../src/session/promptPreview";
import {
  applyEvent,
  isRunning,
  mergeLoadedTurns,
  prependTranscriptTurns,
  turnsFromTranscript,
} from "../src/session/turns";

describe("persisted transcript projection", () => {
  test("ignores retired structured and string plan entries without losing the prompt", () => {
    const turns = turnsFromTranscript([
      ["user", { kind: "prompt", text: "implement", display: "implement" }],
      [
        "agent",
        {
          kind: "plan",
          entries: [
            "Inspect the workspace",
            {
              content: "Implement the panel",
              priority: "high",
              status: "in_progress",
            },
          ],
        },
      ],
    ]);

    expect(turns).toHaveLength(1);
    expect(turns[0].prompt).toBe("implement");
    expect(turns[0]).not.toHaveProperty("plan");
  });

  test("shows the canonical prompt while preserving the agent response", () => {
    const turns = turnsFromTranscript([
      [
        "user",
        {
          kind: "prompt",
          text: "canonical user-authored document",
          display: "compact prompt",
        },
      ],
      ["agent", { kind: "text", text: "first " }],
      ["agent", { kind: "text", text: "response" }],
    ]);

    expect(turns).toHaveLength(1);
    expect(turns[0].prompt).toBe("canonical user-authored document");
    expect(turns[0].text).toBe("first response");
  });

  test("keeps legacy user text compatible", () => {
    const turns = turnsFromTranscript([
      ["user", { kind: "text", text: "legacy prompt" }],
    ]);
    expect(turns[0].prompt).toBe("legacy prompt");
  });

  test("recovers private image references from a durable prompt", () => {
    const first = "11111111-1111-4111-8111-111111111111";
    const second = "22222222-2222-4222-8222-222222222222";
    const turns = turnsFromTranscript([
      [
        "user",
        {
          kind: "prompt",
          text: `Improve image rendering\n\n[attachment:${first}]\n\n[attachment:${second}]`,
          display: "Improve image rendering",
        },
      ],
    ]);

    expect(turns[0].promptImages).toEqual([{ id: first }, { id: second }]);
  });

  test("projects durable row and tool timing into a loaded turn", () => {
    const turns = turnsFromTranscript([
      {
        seq: 1,
        role: "user",
        part: { kind: "prompt", text: "inspect", display: "inspect" },
        created_at: 1000,
        started_at: 1000,
      },
      {
        seq: 2,
        role: "agent",
        part: { kind: "text", text: "checking" },
        created_at: 1500,
        started_at: 1500,
      },
      {
        seq: 3,
        role: "agent",
        part: {
          kind: "tool_call",
          id: "read-1",
          title: "Read",
          status: "completed",
        },
        created_at: 4000,
        started_at: 2000,
      },
    ]);

    expect(turns[0]).toMatchObject({ startedAt: 1000, endedAt: 4000 });
    expect(turns[0].content).toEqual([
      { kind: "text", text: "checking", transcriptSeq: 2, createdAt: 1500 },
      { kind: "tool", toolId: "read-1", transcriptSeq: 3, createdAt: 4000 },
    ]);
    expect(turns[0].tools[0]).toMatchObject({
      startedAt: 2000,
      endedAt: 4000,
    });
  });

  test("merges live output received while a running transcript is loading", () => {
    const loaded = turnsFromTranscript(
      [
        [
          "user",
          { kind: "prompt", text: "current prompt", display: "current prompt" },
        ],
        ["agent", { kind: "text", text: "first " }],
      ],
      true,
      "remote-prompt"
    );
    let live = applyEvent([], {
      event: "turn_started",
      session: "session-a",
      request_id: "remote-prompt",
    });
    live = applyEvent(live, {
      event: "agent_text",
      session: "session-a",
      message_id: "chunk-1",
      text: "first ",
    });
    live = applyEvent(live, {
      event: "agent_text",
      session: "session-a",
      message_id: "chunk-2",
      text: "response",
    });

    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged).toHaveLength(1);
    expect(merged[0].prompt).toBe("current prompt");
    expect(merged[0].text).toBe("first response");
    expect(isRunning(merged[0])).toBe(true);
  });

  test("keeps a newly started live turn separate from an older transcript snapshot", () => {
    const loaded = turnsFromTranscript([
      ["user", { kind: "prompt", text: "old prompt", display: "old prompt" }],
      ["agent", { kind: "text", text: "old answer" }],
    ]);
    let live = applyEvent([], {
      event: "turn_started",
      session: "session-a",
      request_id: "new-prompt",
    });
    live = applyEvent(live, {
      event: "agent_text",
      session: "session-a",
      message_id: "new-chunk",
      text: "new answer",
    });

    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged).toHaveLength(2);
    expect(merged[0].prompt).toBe("old prompt");
    expect(merged[0].text).toBe("old answer");
    expect(merged[1].requestId).toBe("new-prompt");
    expect(merged[1].text).toBe("new answer");
  });

  test("uses the known request identity when loading begins after TurnStarted", () => {
    const loaded = turnsFromTranscript(
      [
        [
          "user",
          { kind: "prompt", text: "running prompt", display: "running prompt" },
        ],
      ],
      true,
      "running-request"
    );
    const live = applyEvent(
      [],
      {
        event: "agent_text",
        session: "session-a",
        message_id: "chunk-1",
        text: "live answer",
      },
      "running-request"
    );

    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged).toHaveLength(1);
    expect(merged[0].requestId).toBe("running-request");
    expect(merged[0].text).toBe("live answer");
    expect(isRunning(merged[0])).toBe(true);
  });

  test("uses the known event boundary instead of content equality for repeated deltas", () => {
    const loaded = turnsFromTranscript(
      [
        ["user", { kind: "prompt", text: "repeat", display: "repeat" }],
        ["agent", { kind: "text", text: "ha" }],
        ["agent", { kind: "reasoning", text: "again" }],
      ],
      true,
      "repeat-request"
    );
    let live = applyEvent([], {
      event: "turn_started",
      session: "session-a",
      request_id: "repeat-request",
    });
    for (const text of ["ha", "ha"]) {
      live = applyEvent(live, {
        event: "agent_text",
        session: "session-a",
        message_id: "",
        text,
      });
    }
    for (const text of ["again", "again"]) {
      live = applyEvent(live, {
        event: "agent_thought",
        session: "session-a",
        text,
      });
    }

    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged).toHaveLength(1);
    expect(merged[0].text).toBe("haha");
    expect(merged[0].thoughts).toEqual(["again", "again"]);
  });

  test("skips persisted deltas that raced ahead of live delivery by count", () => {
    const loaded = turnsFromTranscript(
      [
        ["user", { kind: "prompt", text: "repeat", display: "repeat" }],
        ["agent", { kind: "text", text: "ha" }],
        ["agent", { kind: "text", text: "ha" }],
        ["agent", { kind: "reasoning", text: "again" }],
        ["agent", { kind: "reasoning", text: "again" }],
      ],
      true,
      "repeat-request"
    );
    let live = applyEvent([], {
      event: "turn_started",
      session: "session-a",
      request_id: "repeat-request",
    });
    live = applyEvent(live, {
      event: "agent_text",
      session: "session-a",
      message_id: "chunk-1",
      text: "ha",
    });
    live = applyEvent(live, {
      event: "agent_thought",
      session: "session-a",
      text: "again",
    });

    let merged = mergeLoadedTurns(loaded, live, true);
    expect(merged[0].text).toBe("haha");
    expect(merged[0].thoughts).toEqual(["again", "again"]);

    // The second equal chunks were already in the snapshot, so their delayed IPC events are
    // consumed by position. A third equal chunk is new and must remain despite identical content.
    merged = applyEvent(merged, {
      event: "agent_text",
      session: "session-a",
      message_id: "chunk-2",
      text: "ha",
    });
    merged = applyEvent(merged, {
      event: "agent_thought",
      session: "session-a",
      text: "again",
    });
    expect(merged[0].text).toBe("haha");
    expect(merged[0].thoughts).toEqual(["again", "again"]);

    merged = applyEvent(merged, {
      event: "agent_text",
      session: "session-a",
      message_id: "chunk-3",
      text: "ha",
    });
    merged = applyEvent(merged, {
      event: "agent_thought",
      session: "session-a",
      text: "again",
    });
    expect(merged[0].text).toBe("hahaha");
    expect(merged[0].thoughts).toEqual(["again", "again", "again"]);
  });

  test("prepends older turn-aligned pages once without touching the live tail", () => {
    const current = turnsFromTranscript([
      {
        seq: 20,
        role: "user",
        part: { kind: "prompt", text: "new", display: "new" },
      },
      { seq: 21, role: "agent", part: { kind: "text", text: "answer" } },
    ]);
    const older = turnsFromTranscript([
      {
        seq: 10,
        role: "user",
        part: { kind: "prompt", text: "old", display: "old" },
      },
      { seq: 11, role: "agent", part: { kind: "text", text: "earlier" } },
      // A retried page may overlap at a durable user boundary; it must not duplicate the row.
      {
        seq: 20,
        role: "user",
        part: { kind: "prompt", text: "new", display: "new" },
      },
    ]);

    const combined = prependTranscriptTurns(current, older);
    expect(combined.map((turn) => turn.transcriptStartSeq)).toEqual([10, 20]);
    expect(combined[1]).toBe(current[0]);
  });

  test("does not regress a completed snapshot tool with an older live update", () => {
    const loaded = turnsFromTranscript(
      [
        {
          seq: 10,
          role: "user",
          part: { kind: "prompt", text: "run", display: "run" },
        },
        {
          seq: 12,
          role: "agent",
          part: {
            kind: "tool_call",
            id: "tool-1",
            title: "Delegate researcher",
            status: "completed",
            tool_kind: "agent",
            agent_input: { role: "researcher" },
          },
        },
      ],
      true,
      "request-1"
    );
    let live = applyEvent([], {
      event: "turn_started",
      session: "session-a",
      request_id: "request-1",
      transcript_seq: 10,
    });
    live = applyEvent(live, {
      event: "tool_call",
      session: "session-a",
      id: "tool-1",
      title: "Delegate researcher",
      status: "pending",
      transcript_seq: 11,
    });

    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged[0].tools).toEqual([
      expect.objectContaining({
        id: "tool-1",
        status: "completed",
        kind: "agent",
        agentInput: { role: "researcher" },
        lastTranscriptSeq: 12,
      }),
    ]);
  });

  test("keeps streamed tool calls between the text fragments that surrounded them", () => {
    let turns = applyEvent([], {
      event: "turn_started",
      session: "session-a",
      request_id: "request-1",
    });
    turns = applyEvent(turns, {
      event: "agent_text",
      session: "session-a",
      message_id: "text-1",
      text: "Before tool.",
      transcript_seq: 11,
    });
    turns = applyEvent(turns, {
      event: "tool_call",
      session: "session-a",
      id: "tool-1",
      title: "Read workspace",
      status: "pending",
      transcript_seq: 12,
    });
    turns = applyEvent(turns, {
      event: "tool_call",
      session: "session-a",
      id: "tool-1",
      title: "Read workspace",
      status: "completed",
      transcript_seq: 13,
    });
    turns = applyEvent(turns, {
      event: "agent_text",
      session: "session-a",
      message_id: "text-2",
      text: "After tool.",
      transcript_seq: 14,
    });

    expect(turns[0].content).toEqual([
      {
        kind: "text",
        text: "Before tool.",
        transcriptSeq: 11,
        createdAt: expect.any(Number),
      },
      {
        kind: "tool",
        toolId: "tool-1",
        transcriptSeq: 12,
        createdAt: expect.any(Number),
      },
      {
        kind: "text",
        text: "After tool.",
        transcriptSeq: 14,
        createdAt: expect.any(Number),
      },
    ]);
    expect(turns[0].tools[0].status).toBe("completed");
  });

  test("merges a durable snapshot with live tool ordering by transcript sequence", () => {
    const loaded = turnsFromTranscript(
      [
        {
          seq: 10,
          role: "user",
          part: { kind: "prompt", text: "run", display: "run" },
        },
        { seq: 11, role: "agent", part: { kind: "text", text: "Before" } },
        {
          seq: 13,
          role: "agent",
          part: {
            kind: "tool_call",
            id: "tool-1",
            title: "Read",
            status: "completed",
          },
        },
        { seq: 14, role: "agent", part: { kind: "text", text: "After" } },
      ],
      true,
      "request-1"
    );
    let live = applyEvent([], {
      event: "turn_started",
      session: "session-a",
      request_id: "request-1",
    });
    live = applyEvent(live, {
      event: "agent_text",
      session: "session-a",
      message_id: "text-1",
      text: "Before",
      transcript_seq: 11,
    });
    live = applyEvent(live, {
      event: "tool_call",
      session: "session-a",
      id: "tool-1",
      title: "Read",
      status: "pending",
      transcript_seq: 12,
    });
    live = applyEvent(live, {
      event: "tool_call",
      session: "session-a",
      id: "tool-1",
      title: "Read",
      status: "completed",
      transcript_seq: 13,
    });
    live = applyEvent(live, {
      event: "agent_text",
      session: "session-a",
      message_id: "text-2",
      text: "After",
      transcript_seq: 14,
    });

    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged[0].content).toEqual([
      {
        kind: "text",
        text: "Before",
        transcriptSeq: 11,
        createdAt: expect.any(Number),
      },
      {
        kind: "tool",
        toolId: "tool-1",
        transcriptSeq: 12,
        createdAt: expect.any(Number),
      },
      {
        kind: "text",
        text: "After",
        transcriptSeq: 14,
        createdAt: expect.any(Number),
      },
    ]);
    expect(merged[0].tools[0].status).toBe("completed");
  });
});

describe("long prompt preview", () => {
  test("collapses only after the upstream line or character threshold", () => {
    expect(isLongPrompt("x".repeat(LONG_PROMPT_MAX_CHARS))).toBe(false);
    expect(isLongPrompt("x".repeat(LONG_PROMPT_MAX_CHARS + 1))).toBe(true);
    expect(
      isLongPrompt(Array(LONG_PROMPT_MAX_LINES).fill("line").join("\n"))
    ).toBe(false);
    expect(
      isLongPrompt(
        Array(LONG_PROMPT_MAX_LINES + 1)
          .fill("line")
          .join("\n")
      )
    ).toBe(true);
    expect(isLongPrompt("😀".repeat(LONG_PROMPT_MAX_CHARS))).toBe(false);
    expect(isLongPrompt("😀".repeat(LONG_PROMPT_MAX_CHARS + 1))).toBe(true);
  });

  test("bounds the preview by both lines and Unicode code points", () => {
    const prompt = `${"😀".repeat(LONG_PROMPT_MAX_CHARS + 20)}\n${Array(9).fill("tail").join("\n")}`;
    const preview = collapsedPrompt(prompt);

    expect([...preview]).toHaveLength(LONG_PROMPT_MAX_CHARS);
    expect(preview).not.toContain("tail");
    expect(preview.endsWith("😀")).toBe(true);
  });
});
