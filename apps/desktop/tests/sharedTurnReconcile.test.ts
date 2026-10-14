// @ts-nocheck
import { describe, expect, test } from "bun:test";

import {
  REMOTE_TURN_PLACEHOLDER,
  applyEvent,
  isRunning,
  mergeLoadedTurns,
  newTurn,
  turnsFromTranscript,
} from "../src/session/turns";

const S = "session-1";
const canonical = (seq: number, text: string) => ({
  seq,
  role: "user",
  part: { kind: "text", text },
  created_at: 1000 + seq,
});

describe("another actor's turn on this client", () => {
  const started = {
    event: "turn_started",
    session: S,
    request_id: "a-req",
    transcript_seq: 7,
  };

  test("foreign start keeps the canonical seq on the placeholder", () => {
    const turns = applyEvent([], started);
    expect(turns.at(-1).prompt).toBe(REMOTE_TURN_PLACEHOLDER);
    expect(turns.at(-1).transcriptStartSeq).toBe(7);
  });

  test("transcript read replaces the placeholder with the real prompt once a later turn exists", () => {
    const live = applyEvent([], started);
    const loaded = turnsFromTranscript(
      [canonical(7, "from A"), canonical(9, "from B")],
      true,
      "b-req"
    );
    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged.map((turn) => turn.prompt)).toEqual(["from A", "from B"]);
  });

  test("tail placeholder takes the real prompt and still ends with the live turn", () => {
    let live = applyEvent([], started);
    live = applyEvent(live, {
      event: "turn_ended",
      session: S,
      stop_reason: "cancelled",
    });
    const loaded = turnsFromTranscript([canonical(7, "from A")], false);
    const merged = mergeLoadedTurns(loaded, live, false);
    expect(merged).toHaveLength(1);
    expect(merged[0].prompt).toBe("from A");
    expect(merged[0].stopReason).toBe("cancelled");
    expect(isRunning(merged[0])).toBe(false);
  });

  test("a duplicate/late turn_ended never adds a Working row beside Cancelled", () => {
    let turns = applyEvent([], started);
    turns = applyEvent(turns, {
      event: "turn_ended",
      session: S,
      stop_reason: "cancelled",
    });
    turns = applyEvent(turns, {
      event: "turn_ended",
      session: S,
      stop_reason: "end_turn",
    });
    expect(turns).toHaveLength(1);
    expect(turns.some((turn) => isRunning(turn))).toBe(false);
    expect(turns[0].stopReason).toBe("cancelled");
  });

  test("own optimistic turn is still accepted by its request id", () => {
    const own = newTurn("mine", "b-req");
    const turns = applyEvent([own], { ...started, request_id: "b-req" });
    expect(turns).toHaveLength(1);
    expect(turns[0].prompt).toBe("mine");
  });

  test("B's Stop closes every accepted-running row but keeps queued rows queued", () => {
    const own = { ...newTurn("mine", "b-req"), accepted: true };
    const remote = applyEvent([own], started).at(-1);
    const queued = { ...newTurn("later", "c-req"), delivery: "queued" };
    const turns = applyEvent([own, remote, queued], {
      event: "turn_ended",
      session: S,
      stop_reason: "cancelled",
    });
    expect(
      turns.filter((turn) => turn.accepted && isRunning(turn))
    ).toHaveLength(0);
    expect(isRunning(turns[2])).toBe(true);
  });
});

describe("canonical running row with an active request id", () => {
  test("agent output binds to the loaded running row instead of adding a started-elsewhere row", () => {
    const loaded = turnsFromTranscript(
      [canonical(7, "from A")],
      true,
      undefined
    );
    expect(isRunning(loaded.at(-1))).toBe(true);
    const next = applyEvent(
      loaded,
      { event: "agent_text", session: S, text: "hi", transcript_seq: 8 },
      "a-req"
    );
    expect(
      next.filter((turn) => turn.prompt === REMOTE_TURN_PLACEHOLDER)
    ).toHaveLength(0);
    expect(next).toHaveLength(1);
  });
});

describe("captured live sequence: C observes A's Run on a shared public task", () => {
  // Captured R23/R24: C receives A's `turn_started`; `scopeEventToSelf` turns A's request id into
  // null, so the placeholder has no request id. The follow-up transcript read returns A's
  // canonical running prompt at the same seq (also without a request id).
  const foreignStart = {
    event: "turn_started",
    session: S,
    request_id: null,
    transcript_seq: 7,
  };

  test("merge folds the tail placeholder into the canonical running row: exactly one running row", () => {
    const live = applyEvent([], foreignStart);
    expect(live).toHaveLength(1);
    expect(live[0].prompt).toBe(REMOTE_TURN_PLACEHOLDER);
    const loaded = turnsFromTranscript(
      [canonical(7, "from A")],
      true,
      undefined
    );
    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged.map((turn) => turn.prompt)).toEqual(["from A"]);
    expect(merged.filter((turn) => isRunning(turn))).toHaveLength(1);
  });

  test("output observed before the read lands on the canonical row, and a later Stop ends it", () => {
    let live = applyEvent([], foreignStart);
    live = applyEvent(live, {
      event: "agent_text",
      session: S,
      text: "hi",
      transcript_seq: 8,
    });
    const loaded = turnsFromTranscript(
      [canonical(7, "from A")],
      true,
      undefined
    );
    let merged = mergeLoadedTurns(loaded, live, true);
    expect(merged).toHaveLength(1);
    expect(merged[0].text).toContain("hi");
    merged = applyEvent(merged, {
      event: "turn_ended",
      session: S,
      stop_reason: "cancelled",
    });
    expect(merged.filter((turn) => isRunning(turn))).toHaveLength(0);
  });

  test("a queued canonical prompt after the foreign tail stays queued and is not dropped", () => {
    const live = applyEvent([], foreignStart);
    const loaded = turnsFromTranscript(
      [canonical(7, "from A")],
      true,
      undefined
    );
    const queued = { ...newTurn("later", "c-req"), delivery: "queued" };
    const merged = mergeLoadedTurns(loaded, [...live, queued], true);
    expect(merged.map((turn) => turn.prompt)).toEqual(["from A", "later"]);
  });
});

describe("R32: terminal status survives a canonical reload", () => {
  test("a Cancelled turn that is no longer the tail keeps its stop reason and real duration", () => {
    const live = [
      {
        ...newTurn(REMOTE_TURN_PLACEHOLDER, undefined),
        accepted: true,
        transcriptStartSeq: 7,
        stopReason: "cancelled",
        startedAt: 1_000,
        endedAt: 59_000,
      },
    ];
    // Canonical rows: A's prompt (no output, so the transcript alone says 0s) then C's continue.
    const loaded = turnsFromTranscript(
      [
        canonical(7, "from A"),
        { ...canonical(9, "continue"), created_at: 70_000 },
      ],
      false
    );
    const merged = mergeLoadedTurns(loaded, live, false);
    expect(merged.map((turn) => turn.prompt)).toEqual(["from A", "continue"]);
    expect(merged[0].stopReason).toBe("cancelled");
    expect(merged[0].endedAt - merged[0].startedAt).toBe(58_000);
    expect(merged[1].stopReason).toBeUndefined();
  });

  test("an unknown turn is not given a guessed status", () => {
    const loaded = turnsFromTranscript(
      [canonical(7, "from A"), canonical(9, "next")],
      false
    );
    const live = [
      {
        ...newTurn("x", "r"),
        accepted: true,
        transcriptStartSeq: 99,
        endedAt: 5,
        stopReason: "cancelled",
      },
    ];
    const merged = mergeLoadedTurns(loaded, live, false);
    expect(merged[0].stopReason).toBeUndefined();
  });

  test("a running canonical tail stays running across a reload while Core is running", () => {
    const loaded = turnsFromTranscript(
      [canonical(7, "from C")],
      true,
      undefined
    );
    const live = applyEvent([], {
      event: "turn_started",
      session: S,
      request_id: null,
      transcript_seq: 7,
    });
    const merged = mergeLoadedTurns(loaded, live, true);
    expect(merged).toHaveLength(1);
    expect(isRunning(merged[0])).toBe(true);
  });
});

describe("R35: own turn Cancelled by Stop keeps its duration after C continues", () => {
  // Real lifecycle: optimistic row -> turn_started (canonical seq) -> Stop -> next prompt -> reload.
  const run = () => {
    const realNow = Date.now;
    try {
      Date.now = () => 1_000;
      let live = [newTurn("from C", "c-req")];
      live = applyEvent(live, {
        event: "turn_started",
        session: S,
        request_id: "c-req",
        transcript_seq: 7,
      });
      Date.now = () => 38_000;
      live = applyEvent(live, {
        event: "turn_ended",
        session: S,
        stop_reason: "cancelled",
      });
      return live;
    } finally {
      Date.now = realNow;
    }
  };

  test("turn_started on the optimistic row records the canonical seq", () => {
    expect(run()[0].transcriptStartSeq).toBe(7);
  });

  test("a reload after the next prompt still shows Cancelled and 37s, not 0s", () => {
    const loaded = turnsFromTranscript(
      [
        canonical(7, "from C"),
        { ...canonical(9, "continue"), created_at: 70_000 },
      ],
      false
    );
    const merged = mergeLoadedTurns(loaded, run(), false);
    expect(merged.map((turn) => turn.prompt)).toEqual(["from C", "continue"]);
    expect(merged[0].stopReason).toBe("cancelled");
    expect(merged[0].endedAt - merged[0].startedAt).toBe(37_000);
  });
});
